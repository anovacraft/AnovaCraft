//! Bayesian analyses bound to a dataset ([`stat_bayes`]): estimation with
//! conjugate priors, Bayes factor tests, Bayesian linear models and ANOVA
//! (in [`crate::bayes_models`]) and Bayesian generalized linear and
//! hierarchical models sampled by NUTS (also there).
//!
//! Every analysis is one [`BayesRequest`] saved as
//! [`crate::SavedReport::Bayesian`] and run through
//! [`crate::ComparisonReport`], so its settings are edited in place.
//!
//! * **Estimation** (`"bayesian-estimation"`): the posterior of a mean,
//!   proportion or event rate, for one group, two groups (difference,
//!   ratio or odds ratio) or paired columns, with equal-tailed and highest
//!   density intervals, probabilities about a reference value and a region of
//!   practical equivalence, and the Savage–Dickey Bayes factor when the
//!   prior is proper.
//! * **Bayes factor tests** (`"bayes-factor"`): one-sample, paired and
//!   two-sample t (JZS Cauchy prior on the standardized effect δ), Pearson
//!   correlation (stretched beta prior on ρ) and contingency tables (Dirichlet
//!   priors under each sampling plan), with a robustness plot over the prior
//!   scale.

use crate::format::format_number;
use crate::project::{invalid, member, obj, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_bayes::bayes_factor::{self, BayesFactorTest, Direction, Sampling, TDesign, evidence_label};
use stat_bayes::conjugate::{self, BetaPrior, GammaPrior, MeanPrior, Probabilities, SampleStats};
use stat_bayes::univariate::Contrast;
use stat_bayes::{Summary, Univariate};
use stat_core::Result;
use stat_core::special::ln_beta;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::natural_order;

pub(crate) const DIGITS: usize = 5;

pub(crate) fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

/// Settings of any Bayesian analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum BayesRequest {
    /// Conjugate estimation of a mean, proportion or rate.
    Estimation(BayesEstimationRequest),
    /// Bayes factor test.
    Factor(BayesFactorRequest),
    /// Bayesian linear model or ANOVA.
    Linear(crate::bayes_models::BayesLinearRequest),
    /// Bayesian generalized linear or hierarchical model.
    Model(Box<crate::bayes_models::BayesModelRequest>),
}

impl BayesRequest {
    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match self {
            Self::Estimation(r) => r.columns(),
            Self::Factor(r) => r.columns(),
            Self::Linear(r) => r.columns(),
            Self::Model(r) => r.columns(),
        }
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        match self {
            Self::Estimation(r) => Self::Estimation(r.map_columns(f)),
            Self::Factor(r) => Self::Factor(r.map_columns(f)),
            Self::Linear(r) => Self::Linear(r.map_columns(f)),
            Self::Model(r) => Self::Model(Box::new(r.map_columns(f))),
        }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        match self {
            Self::Estimation(r) => r.to_json(table),
            Self::Factor(r) => r.to_json(table),
            Self::Linear(r) => r.to_json(table),
            Self::Model(r) => r.to_json(table),
        }
    }

    /// Read the project-file form whose `"analysis"` member is `key`.
    ///
    /// # Errors
    /// Missing columns or invalid settings.
    pub fn from_json(key: &str, v: &JsonValue, table: &DataTable) -> Result<Self> {
        Ok(match key {
            "bayesian-estimation" => Self::Estimation(BayesEstimationRequest::from_json(v, table)?),
            "bayes-factor" => Self::Factor(BayesFactorRequest::from_json(v, table)?),
            "bayesian-linear-model" => Self::Linear(crate::bayes_models::BayesLinearRequest::from_json(v, table)?),
            "bayesian-model" => Self::Model(Box::new(crate::bayes_models::BayesModelRequest::from_json(v, table)?)),
            other => return Err(invalid(format!("unknown Bayesian analysis {other:?}"))),
        })
    }

    /// Name of the analysis, for a report that could not be computed.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Estimation(_) => "Bayesian Estimation",
            Self::Factor(_) => "Bayes Factor Test",
            Self::Linear(_) => "Bayesian Linear Model",
            Self::Model(_) => "Bayesian Model",
        }
    }
}

/// A computed Bayesian report.
#[derive(Debug, Clone)]
pub struct BayesReport {
    title: String,
    sections: Vec<ReportSection>,
    /// Names of the parameters with diagnostic plots (sampled models only),
    /// for the report's settings.
    pub parameters: Vec<String>,
}

impl BayesReport {
    /// Run `request` on the included rows.
    ///
    /// # Errors
    /// Invalid columns or settings, too few rows, or the engine's errors.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &BayesRequest) -> Result<Self> {
        let (title, sections, parameters) = match request {
            BayesRequest::Estimation(r) => {
                let (t, s) = run_estimation(table, states, r)?;
                (t, s, Vec::new())
            }
            BayesRequest::Factor(r) => {
                let (t, s) = run_factor(table, states, r)?;
                (t, s, Vec::new())
            }
            BayesRequest::Linear(r) => {
                let (t, s) = crate::bayes_models::run_linear(table, states, r)?;
                (t, s, Vec::new())
            }
            BayesRequest::Model(r) => crate::bayes_models::run_model(table, states, r)?,
        };
        Ok(Self { title, sections, parameters })
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        self.sections.clone()
    }
}

// ---------------------------------------------------------------------------
// Shared reading and writing.

pub(crate) fn column_index(v: &JsonValue, key: &str, table: &DataTable) -> Result<usize> {
    let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
    table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
}

pub(crate) fn optional_column(v: &JsonValue, key: &str, table: &DataTable) -> Result<Option<usize>> {
    match v.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(_) => column_index(v, key, table).map(Some),
    }
}

pub(crate) fn column_list(v: &JsonValue, key: &str, table: &DataTable) -> Result<Vec<usize>> {
    let Some(list) = v.get(key).and_then(JsonValue::as_array) else { return Ok(Vec::new()) };
    list.iter()
        .map(|j| {
            let name = j.as_str().ok_or_else(|| invalid(format!("{key}: expected column names")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        })
        .collect()
}

pub(crate) fn col_json(table: &DataTable, i: usize) -> JsonValue {
    s(table.column(i).map_or("", |c| c.name.as_str()))
}

pub(crate) fn opt_col_json(table: &DataTable, i: Option<usize>) -> JsonValue {
    i.map_or(JsonValue::Null, |i| col_json(table, i))
}

pub(crate) fn number(v: &JsonValue, key: &str, default: f64) -> f64 {
    v.get(key).and_then(JsonValue::as_f64).unwrap_or(default)
}

pub(crate) fn read_level(v: &JsonValue, what: &str) -> Result<f64> {
    let level = number(v, "level", 0.95);
    if level > 0.0 && level < 1.0 { Ok(level) } else { Err(invalid(format!("{what}: level must lie between 0 and 1"))) }
}

pub(crate) fn column_name(table: &DataTable, i: usize) -> String {
    table.column(i).map_or_else(String::new, |c| c.name.clone())
}

/// Values of numeric column `i` on included rows, with their rows.
fn numeric_rows(table: &DataTable, states: &RowStateManager, i: usize) -> Result<(Vec<usize>, Vec<f64>)> {
    let c = table.column(i).ok_or_else(|| invalid("no such column"))?;
    let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
    let rows: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r) && v[r].is_finite()).collect();
    let values = rows.iter().map(|&r| v[r]).collect();
    Ok((rows, values))
}

/// Levels (natural order) of column `i` on `rows`, and each row's level.
fn levels_of(table: &DataTable, i: usize, rows: &[usize]) -> Result<(Vec<String>, Vec<usize>)> {
    let c = table.column(i).ok_or_else(|| invalid("no such column"))?;
    let mut levels: Vec<String> = rows.iter().map(|&r| c.level_text(r)).collect();
    levels.sort_by(|a, b| natural_order(a, b));
    levels.dedup();
    let codes = rows.iter().map(|&r| levels.iter().position(|l| *l == c.level_text(r)).unwrap_or(0)).collect();
    Ok((levels, codes))
}

pub(crate) fn pct(level: f64) -> String {
    format!("{}%", format_number(100.0 * level, 4))
}

fn summary_columns(first: &str, level: f64) -> TextTable {
    let p = pct(level);
    TextTable::new([
        first.to_owned(),
        "Mean".to_owned(),
        "Median".to_owned(),
        "SD".to_owned(),
        format!("Lower {p}"),
        format!("Upper {p}"),
        format!("HPD lower {p}"),
        format!("HPD upper {p}"),
    ])
}

fn contrast_noun(contrast: Contrast) -> &'static str {
    match contrast {
        Contrast::Difference => "difference",
        Contrast::Ratio => "ratio",
        Contrast::OddsRatio => "odds ratio",
    }
}

fn summary_row(table: &mut TextTable, name: &str, s: &Summary) {
    table.push([
        name.to_owned(),
        format_number(s.mean, DIGITS),
        format_number(s.median, DIGITS),
        format_number(s.sd, DIGITS),
        format_number(s.equal_tailed.0, DIGITS),
        format_number(s.equal_tailed.1, DIGITS),
        format_number(s.hpd.0, DIGITS),
        format_number(s.hpd.1, DIGITS),
    ]);
}

fn probability(p: f64) -> String {
    if p < 1e-4 {
        "< 0.0001".to_owned()
    } else if p > 1.0 - 1e-4 {
        "> 0.9999".to_owned()
    } else {
        format!("{p:.4}")
    }
}

/// A Bayes factor written so both large and small values read well.
pub(crate) fn format_bf(bf: f64) -> String {
    if !bf.is_finite() {
        return if bf > 0.0 { "∞".to_owned() } else { "—".to_owned() };
    }
    if bf >= 1e6 || (bf > 0.0 && bf < 1e-4) { format!("{bf:.3e}") } else { format_number(bf, 4) }
}

fn probabilities_section(what: &str, p: &Probabilities, bayes_factor: Option<f64>) -> ReportSection {
    let mut t = TextTable::new(["Statement", "Posterior probability"]);
    let r = format_number(p.reference, DIGITS);
    t.push([format!("{what} < {r}"), probability(p.below)]);
    t.push([format!("{what} > {r}"), probability(p.above)]);
    if let Some((lo, hi, q)) = p.rope {
        t.push([
            format!("{} ≤ {what} ≤ {} (practically equivalent)", format_number(lo, DIGITS), format_number(hi, DIGITS)),
            probability(q),
        ]);
    }
    let note = match bayes_factor {
        Some(bf) => format!(
            "; Bayes factor against {what} = {r}: BF₁₀ = {} (Savage–Dickey density ratio; {})",
            format_bf(bf),
            evidence_label(bf)
        ),
        None => String::new(),
    };
    section(SectionKind::BayesProbabilities, format!("Posterior Probabilities{note}"), SectionContent::Table(t))
}

/// `n + 1` points of `d`'s density between its 0.05% and 99.95% points.
fn density_curve(d: &dyn Univariate, n: usize) -> Vec<[f64; 2]> {
    let (lo, hi) = (d.quantile(0.0005), d.quantile(0.9995));
    curve_on(d, lo, hi, n)
}

/// First and last horizontal positions of a curve (`(0, 1)` when empty).
fn span(c: &[[f64; 2]]) -> (f64, f64) {
    match (c.first(), c.last()) {
        (Some(a), Some(b)) => (a[0], b[0]),
        _ => (0.0, 1.0),
    }
}

fn curve_on(d: &dyn Univariate, lo: f64, hi: f64, n: usize) -> Vec<[f64; 2]> {
    (0..=n)
        .map(|i| {
            let x = lo + (hi - lo) * i as f64 / n as f64;
            [x, d.pdf(x)]
        })
        .filter(|p| p[1].is_finite())
        .collect()
}

/// Prior (dashed) and posterior (solid) densities, with the credible
/// interval marked under the curve and the reference value labelled.
pub(crate) fn density_plot(
    x_label: &str,
    posterior: Vec<[f64; 2]>,
    prior: Option<Vec<[f64; 2]>>,
    interval: (f64, f64),
    level: f64,
    reference: Option<f64>,
) -> PlotSpec {
    let top = posterior.iter().map(|p| p[1]).fold(0.0_f64, f64::max);
    let mut segments = vec![vec![[interval.0, 0.0], [interval.1, 0.0]]];
    let mut labels = vec![((interval.0 + interval.1) / 2.0, 0.06 * top, format!("{} interval", pct(level)))];
    if let Some(r) = reference {
        segments.push(vec![[r, 0.0], [r, 1.05 * top]]);
        labels.push((r, 1.1 * top, format!("reference {}", format_number(r, 4))));
    }
    let band = prior.map(|p| {
        // Clip the prior to the posterior's horizontal range so a vague
        // prior does not stretch the axis.
        let (lo, hi) = span(&posterior);
        let clipped: Vec<[f64; 2]> = p.into_iter().filter(|q| q[0] >= lo && q[0] <= hi).collect();
        (clipped.clone(), clipped)
    });
    PlotSpec {
        x_label: x_label.to_owned(),
        y_label: "Density".to_owned(),
        line: Some(posterior),
        line_name: Some("Posterior".to_owned()),
        band_name: band.as_ref().map(|_| "Prior".to_owned()),
        band,
        segments,
        annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
        ..PlotSpec::default()
    }
}

// ---------------------------------------------------------------------------
// Estimation.

/// What is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstimatedParameter {
    /// A normal mean (with its SD).
    Mean,
    /// A proportion.
    Proportion,
    /// A Poisson event rate.
    Rate,
}

impl EstimatedParameter {
    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Proportion => "proportion",
            Self::Rate => "rate",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        [Self::Mean, Self::Proportion, Self::Rate].into_iter().find(|p| p.key() == key)
    }
}

/// The prior of an estimated parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BayesPrior {
    /// Jeffreys prior (Beta(½, ½) for a proportion, Gamma(½, 0) for a rate,
    /// `1/σ²` for a normal mean and SD).
    Default,
    /// Flat: Beta(1, 1) for a proportion, Gamma(1, 0) for a rate (for a mean
    /// the same as the default).
    Flat,
    /// Informative: for a proportion or rate, a beta or gamma prior with this
    /// mean and SD; for a normal mean, a prior mean worth `spread` observations.
    Informative {
        /// Prior mean.
        mean: f64,
        /// Prior SD (proportion, rate) or prior weight in observations (mean).
        spread: f64,
    },
}

/// Settings of a Bayesian estimation.
#[derive(Debug, Clone, PartialEq)]
pub struct BayesEstimationRequest {
    /// What is estimated.
    pub parameter: EstimatedParameter,
    /// Values (mean), outcomes or numbers of successes (proportion; numbers
    /// when `denominator` is set, otherwise labels with `event` a success), or
    /// event counts (rate).
    pub response: usize,
    /// Two-group comparison by this column (exactly two levels).
    pub group: Option<usize>,
    /// Paired means: the second column (the posterior is of the mean of
    /// `response − second`).
    pub second: Option<usize>,
    /// Trials (proportion) or exposure (rate) per row; without one each row
    /// is one trial or one unit of exposure.
    pub denominator: Option<usize>,
    /// Count of identical rows (proportion with labelled outcomes).
    pub frequency: Option<usize>,
    /// Outcome counted as a success (`None`: the second value in natural
    /// order of a two-valued column).
    pub event: Option<String>,
    /// Prior.
    pub prior: BayesPrior,
    /// Two means: assume equal variances (pooled t posterior); otherwise
    /// the Behrens–Fisher posterior.
    pub equal_variances: bool,
    /// Two proportions: difference, ratio or odds ratio.
    pub contrast: Contrast,
    /// One group: reference value for the probability statements and the
    /// Bayes factor (`None`: 0.5 for a proportion, 0 for a mean, 1 for a rate).
    pub reference: Option<f64>,
    /// Region of practical equivalence `(lower, upper)`.
    pub rope: Option<(f64, f64)>,
    /// Credible level.
    pub level: f64,
}

impl BayesEstimationRequest {
    /// Default settings for `parameter` of `response`.
    #[must_use]
    pub fn new(parameter: EstimatedParameter, response: usize) -> Self {
        Self {
            parameter,
            response,
            group: None,
            second: None,
            denominator: None,
            frequency: None,
            event: None,
            prior: BayesPrior::Default,
            equal_variances: false,
            contrast: Contrast::Difference,
            reference: None,
            rope: None,
            level: 0.95,
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.response)
            .chain(self.group)
            .chain(self.second)
            .chain(self.denominator)
            .chain(self.frequency)
            .collect()
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self {
            response: f(self.response),
            group: self.group.map(f),
            second: self.second.map(f),
            denominator: self.denominator.map(f),
            frequency: self.frequency.map(f),
            ..self.clone()
        }
    }

    /// The reference value actually used.
    #[must_use]
    pub fn reference_value(&self) -> f64 {
        self.reference.unwrap_or(match self.parameter {
            EstimatedParameter::Mean => 0.0,
            EstimatedParameter::Proportion => 0.5,
            EstimatedParameter::Rate => 1.0,
        })
    }

    fn to_json(&self, table: &DataTable) -> JsonValue {
        let (prior, prior_mean, spread) = match self.prior {
            BayesPrior::Default => ("default", JsonValue::Null, JsonValue::Null),
            BayesPrior::Flat => ("flat", JsonValue::Null, JsonValue::Null),
            BayesPrior::Informative { mean, spread } => {
                ("informative", JsonValue::number(mean), JsonValue::number(spread))
            }
        };
        obj([
            ("analysis", s("bayesian-estimation")),
            ("parameter", s(self.parameter.key())),
            ("response", col_json(table, self.response)),
            ("group", opt_col_json(table, self.group)),
            ("second", opt_col_json(table, self.second)),
            ("denominator", opt_col_json(table, self.denominator)),
            ("frequency", opt_col_json(table, self.frequency)),
            ("event", self.event.as_deref().map_or(JsonValue::Null, s)),
            ("prior", s(prior)),
            ("prior_mean", prior_mean),
            ("prior_spread", spread),
            ("equal_variances", JsonValue::Bool(self.equal_variances)),
            ("contrast", s(contrast_key(self.contrast))),
            ("reference", self.reference.map_or(JsonValue::Null, JsonValue::number)),
            (
                "rope",
                self.rope.map_or(JsonValue::Null, |(a, b)| {
                    JsonValue::Array(vec![JsonValue::number(a), JsonValue::number(b)])
                }),
            ),
            ("level", JsonValue::number(self.level)),
        ])
    }

    fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let key = member(v, "parameter")?.as_str().unwrap_or_default();
        let parameter = EstimatedParameter::from_key(key)
            .ok_or_else(|| invalid(format!("bayesian-estimation: unknown parameter {key:?}")))?;
        let prior = match v.get("prior").and_then(JsonValue::as_str).unwrap_or("default") {
            "default" => BayesPrior::Default,
            "flat" => BayesPrior::Flat,
            "informative" => BayesPrior::Informative {
                mean: number(v, "prior_mean", f64::NAN),
                spread: number(v, "prior_spread", f64::NAN),
            },
            other => return Err(invalid(format!("bayesian-estimation: unknown prior {other:?}"))),
        };
        let contrast_name = v.get("contrast").and_then(JsonValue::as_str).unwrap_or("difference");
        let contrast = contrast_from_key(contrast_name)
            .ok_or_else(|| invalid(format!("bayesian-estimation: unknown contrast {contrast_name:?}")))?;
        let rope = match v.get("rope").and_then(JsonValue::as_array) {
            Some([a, b]) => Some((a.as_f64().unwrap_or(f64::NAN), b.as_f64().unwrap_or(f64::NAN))),
            _ => None,
        };
        Ok(Self {
            parameter,
            response: column_index(v, "response", table)?,
            group: optional_column(v, "group", table)?,
            second: optional_column(v, "second", table)?,
            denominator: optional_column(v, "denominator", table)?,
            frequency: optional_column(v, "frequency", table)?,
            event: v.get("event").and_then(JsonValue::as_str).map(str::to_owned),
            prior,
            equal_variances: v.get("equal_variances").and_then(JsonValue::as_bool).unwrap_or(false),
            contrast,
            reference: v.get("reference").and_then(JsonValue::as_f64),
            rope,
            level: read_level(v, "bayesian-estimation")?,
        })
    }
}

fn contrast_key(c: Contrast) -> &'static str {
    match c {
        Contrast::Difference => "difference",
        Contrast::Ratio => "ratio",
        Contrast::OddsRatio => "odds-ratio",
    }
}

fn contrast_from_key(key: &str) -> Option<Contrast> {
    [Contrast::Difference, Contrast::Ratio, Contrast::OddsRatio].into_iter().find(|c| contrast_key(*c) == key)
}

/// Label of a contrast between `a` and `b` of `what`.
fn contrast_label(c: Contrast, what: &str, a: &str, b: &str) -> String {
    match c {
        Contrast::Difference => format!("{what} {a} − {b}"),
        Contrast::Ratio => format!("{what} ratio {a} / {b}"),
        Contrast::OddsRatio => format!("Odds ratio {a} / {b}"),
    }
}

/// Successes and trials (or events and exposure) of each group.
struct Counts {
    groups: Vec<String>,
    totals: Vec<(f64, f64)>,
    /// Rows in each group.
    rows: Vec<usize>,
    event: String,
}

fn gather_counts(table: &DataTable, states: &RowStateManager, r: &BayesEstimationRequest) -> Result<Counts> {
    let column = |i: usize| table.column(i).ok_or_else(|| invalid("no such column"));
    let rc = column(r.response)?;
    let numeric = |i: usize, what: &str| -> Result<Vec<f64>> {
        let c = column(i)?;
        c.as_numeric()
            .map(<[f64]>::to_vec)
            .ok_or_else(|| invalid(format!("the {what} column {:?} must be numeric", c.name)))
    };
    let den = r
        .denominator
        .map(|d| numeric(d, if r.parameter == EstimatedParameter::Rate { "exposure" } else { "trials" }))
        .transpose()?;
    let freq = r.frequency.map(|f| numeric(f, "frequency")).transpose()?;
    let gc = r.group.map(column).transpose()?;
    let counted = r.parameter == EstimatedParameter::Rate || den.is_some();
    let y = if counted { Some(numeric(r.response, "count")?) } else { None };
    let mut rows: Vec<usize> = (0..table.n_rows())
        .filter(|&i| {
            !states.is_excluded(i)
                && y.as_ref().is_none_or(|y| y[i].is_finite())
                && (y.is_some() || !rc.level_text(i).trim().is_empty())
                && den.as_ref().is_none_or(|d| d[i].is_finite() && d[i] > 0.0)
                && freq.as_ref().is_none_or(|f| f[i].is_finite() && f[i] >= 0.0)
                && gc.is_none_or(|g| !g.level_text(i).trim().is_empty())
        })
        .collect();
    rows.sort_unstable();
    if rows.is_empty() {
        return Err(invalid("no included rows have values in every column"));
    }
    let (groups, codes) = match r.group {
        Some(g) => {
            let (levels, codes) = levels_of(table, g, &rows)?;
            if levels.len() != 2 {
                return Err(invalid(format!(
                    "the group column {:?} must have exactly two values in the rows used; it has {}",
                    column_name(table, g),
                    levels.len()
                )));
            }
            (levels, codes)
        }
        None => (vec![String::new()], vec![0; rows.len()]),
    };
    // The success label of a labelled outcome.
    let mut event = String::new();
    if !counted {
        let mut values: Vec<String> = rows.iter().map(|&i| rc.level_text(i)).collect();
        values.sort_by(|a, b| natural_order(a, b));
        values.dedup();
        event = match &r.event {
            Some(e) if values.contains(e) => e.clone(),
            Some(e) => return Err(invalid(format!("{e:?} is not a value of {:?}", rc.name))),
            None if values.len() == 2 => values[1].clone(),
            None => {
                return Err(invalid(format!(
                    "{:?} has {} values: choose which one counts as a success",
                    rc.name,
                    values.len()
                )));
            }
        };
    }
    let mut totals = vec![(0.0, 0.0); groups.len()];
    let mut n_rows = vec![0; groups.len()];
    for (k, &i) in rows.iter().enumerate() {
        let w = freq.as_ref().map_or(1.0, |f| f[i]);
        let g = codes[k];
        n_rows[g] += 1;
        match (&y, &den) {
            (Some(y), Some(d)) => {
                totals[g].0 += w * y[i];
                totals[g].1 += w * d[i];
            }
            (Some(y), None) => {
                totals[g].0 += w * y[i];
                totals[g].1 += w;
            }
            _ => {
                if rc.level_text(i) == event {
                    totals[g].0 += w;
                }
                totals[g].1 += w;
            }
        }
    }
    Ok(Counts { groups, totals, rows: n_rows, event })
}

fn beta_prior(p: BayesPrior) -> Result<BetaPrior> {
    match p {
        BayesPrior::Default => Ok(BetaPrior::JEFFREYS),
        BayesPrior::Flat => Ok(BetaPrior::UNIFORM),
        BayesPrior::Informative { mean, spread } => BetaPrior::from_mean_sd(mean, spread),
    }
}

fn gamma_prior(p: BayesPrior) -> Result<GammaPrior> {
    match p {
        BayesPrior::Default => Ok(GammaPrior::JEFFREYS),
        BayesPrior::Flat => Ok(GammaPrior { shape: 1.0, rate: 0.0 }),
        BayesPrior::Informative { mean, spread } => GammaPrior::from_mean_sd(mean, spread),
    }
}

fn prior_text_beta(p: BetaPrior, choice: BayesPrior) -> String {
    let name = match choice {
        BayesPrior::Default => "Jeffreys",
        BayesPrior::Flat => "flat",
        BayesPrior::Informative { .. } => "informative",
    };
    format!("Beta({}, {}) ({name})", format_number(p.a, 4), format_number(p.b, 4))
}

fn prior_text_gamma(p: GammaPrior, choice: BayesPrior) -> String {
    let name = match choice {
        BayesPrior::Default => "Jeffreys, improper",
        BayesPrior::Flat => "flat, improper",
        BayesPrior::Informative { .. } => "informative",
    };
    format!("Gamma(shape {}, rate {}) ({name})", format_number(p.shape, 4), format_number(p.rate, 4))
}

fn run_estimation(
    table: &DataTable,
    states: &RowStateManager,
    r: &BayesEstimationRequest,
) -> Result<(String, Vec<ReportSection>)> {
    match r.parameter {
        EstimatedParameter::Mean => estimate_mean(table, states, r),
        EstimatedParameter::Proportion | EstimatedParameter::Rate => estimate_counts(table, states, r),
    }
}

fn estimate_mean(
    table: &DataTable,
    states: &RowStateManager,
    r: &BayesEstimationRequest,
) -> Result<(String, Vec<ReportSection>)> {
    let name = column_name(table, r.response);
    let rope = r.rope;
    let level = r.level;
    let mut out = Vec::new();
    if let Some(g) = r.group {
        let (rows, y) = numeric_rows(table, states, r.response)?;
        let gc = table.column(g).ok_or_else(|| invalid("no such column"))?;
        let keep: Vec<usize> = (0..rows.len()).filter(|&k| !gc.level_text(rows[k]).trim().is_empty()).collect();
        let rows: Vec<usize> = keep.iter().map(|&k| rows[k]).collect();
        let y: Vec<f64> = keep.iter().map(|&k| y[k]).collect();
        let (levels, codes) = levels_of(table, g, &rows)?;
        if levels.len() != 2 {
            return Err(invalid(format!(
                "the group column {:?} must have exactly two values; it has {}",
                gc.name,
                levels.len()
            )));
        }
        let part = |k: usize| -> Vec<f64> { y.iter().zip(&codes).filter(|(_, c)| **c == k).map(|(v, _)| *v).collect() };
        let (a, b) = (SampleStats::of(&part(0))?, SampleStats::of(&part(1))?);
        let fit = conjugate::two_means(a, b, r.equal_variances, level, rope)?;
        let mut data = TextTable::new([gc.name.as_str(), "N", "Mean", "SD"].map(str::to_owned));
        for (l, st) in levels.iter().zip([a, b]) {
            data.push([
                l.clone(),
                format_number(st.n, 6),
                format_number(st.mean, DIGITS),
                format_number(st.sd, DIGITS),
            ]);
        }
        out.push(section(SectionKind::BayesData, "Data", SectionContent::Table(data)));
        let model = if r.equal_variances {
            "a common SD (pooled; the difference has a t posterior)"
        } else {
            "separate SDs (Behrens–Fisher posterior of the difference)"
        };
        out.push(section(
            SectionKind::BayesPrior,
            "Prior",
            SectionContent::Note(format!("Jeffreys prior p(μ, σ²) ∝ 1/σ² in each group, with {model}.")),
        ));
        let diff_label = contrast_label(Contrast::Difference, "Mean", &levels[0], &levels[1]);
        let mut t = summary_columns("Parameter", level);
        summary_row(&mut t, &format!("Mean {}", levels[0]), &fit.first.summary);
        summary_row(&mut t, &format!("Mean {}", levels[1]), &fit.second.summary);
        summary_row(&mut t, &diff_label, &fit.summary);
        out.push(section(SectionKind::BayesPosterior, "Posterior Summary", SectionContent::Table(t)));
        out.push(probabilities_section("difference", &fit.probabilities, None));
        out.push(section(
            SectionKind::BayesDensity,
            format!("Posterior of the Difference (equal-tailed {} interval marked)", pct(level)),
            SectionContent::Plot(Box::new(density_plot(
                &diff_label,
                fit.density.clone(),
                None,
                fit.summary.equal_tailed,
                level,
                Some(0.0),
            ))),
        ));
        out.push(group_posteriors_plot(&gc.name, &levels, [&fit.first.posterior, &fit.second.posterior], "Mean"));
        return Ok((format!("Bayesian Estimation: Mean of {name} by {}", gc.name), out));
    }
    let (values, label) = match r.second {
        Some(second) => {
            let (r1, y1) = numeric_rows(table, states, r.response)?;
            let (r2, y2) = numeric_rows(table, states, second)?;
            let d: Vec<f64> =
                r1.iter().zip(&y1).filter_map(|(row, a)| r2.iter().position(|x| x == row).map(|k| a - y2[k])).collect();
            (d, format!("{name} − {}", column_name(table, second)))
        }
        None => (numeric_rows(table, states, r.response)?.1, name.clone()),
    };
    let stats = SampleStats::of(&values)?;
    let prior = match r.prior {
        BayesPrior::Default | BayesPrior::Flat => MeanPrior::Jeffreys,
        BayesPrior::Informative { mean, spread } => MeanPrior::Informative { mean, weight: spread },
    };
    let reference = r.reference_value();
    let fit = conjugate::normal_mean(stats, prior, level, reference, rope)?;
    let mut data = TextTable::new(["Column", "N", "Mean", "SD"]);
    data.push([
        label.clone(),
        format_number(stats.n, 6),
        format_number(stats.mean, DIGITS),
        format_number(stats.sd, DIGITS),
    ]);
    out.push(section(
        SectionKind::BayesData,
        if r.second.is_some() { "Data (paired differences)" } else { "Data" },
        SectionContent::Table(data),
    ));
    let prior_text = match prior {
        MeanPrior::Jeffreys => "Jeffreys prior p(μ, σ²) ∝ 1/σ² (the posterior of μ is a t distribution).".to_owned(),
        MeanPrior::Informative { mean, weight } => format!(
            "μ | σ² ~ N({}, σ²/{}): a prior mean worth {} observations, with p(σ²) ∝ 1/σ².",
            format_number(mean, DIGITS),
            format_number(weight, 4),
            format_number(weight, 4)
        ),
    };
    out.push(section(SectionKind::BayesPrior, "Prior", SectionContent::Note(prior_text)));
    let mut t = summary_columns("Parameter", level);
    summary_row(&mut t, "Mean μ", &fit.mean.summary);
    summary_row(&mut t, "SD σ", &fit.sigma_summary);
    out.push(section(SectionKind::BayesPosterior, "Posterior Summary", SectionContent::Table(t)));
    out.push(probabilities_section("μ", &fit.mean.probabilities, fit.mean.bayes_factor));
    let posterior = density_curve(&fit.mean.posterior, 200);
    let prior_curve = fit.mean.prior.as_ref().map(|p| {
        let (lo, hi) = span(&posterior);
        curve_on(p, lo, hi, 200)
    });
    out.push(section(
        SectionKind::BayesDensity,
        format!("Prior and Posterior of μ (equal-tailed {} interval marked)", pct(level)),
        SectionContent::Plot(Box::new(density_plot(
            &format!("Mean of {label}"),
            posterior,
            prior_curve,
            fit.mean.summary.equal_tailed,
            level,
            Some(reference),
        ))),
    ));
    Ok((format!("Bayesian Estimation: Mean of {label}"), out))
}

/// Each group's posterior, labelled at its peak.
fn group_posteriors_plot(
    group: &str,
    levels: &[String],
    posteriors: [&dyn Univariate; 2],
    what: &str,
) -> ReportSection {
    let lo = posteriors.iter().map(|d| d.quantile(0.0005)).fold(f64::INFINITY, f64::min);
    let hi = posteriors.iter().map(|d| d.quantile(0.9995)).fold(f64::NEG_INFINITY, f64::max);
    let curves: Vec<Vec<[f64; 2]>> = posteriors.iter().map(|d| curve_on(*d, lo, hi, 200)).collect();
    let labels = curves
        .iter()
        .zip(levels)
        .filter_map(|(c, l)| c.iter().max_by(|a, b| a[1].total_cmp(&b[1])).map(|p| (p[0], 1.06 * p[1], l.clone())))
        .collect();
    let spec = PlotSpec {
        x_label: what.to_owned(),
        y_label: "Posterior density".to_owned(),
        groups: Some(crate::report::PointGroups {
            title: group.to_owned(),
            labels: levels.to_vec(),
            membership: Vec::new(),
            lines: curves,
        }),
        annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
        ..PlotSpec::default()
    };
    section(
        SectionKind::BayesGroupPosteriors,
        format!("Posterior of Each {group} (each curve labelled at its peak)"),
        SectionContent::Plot(Box::new(spec)),
    )
}

fn estimate_counts(
    table: &DataTable,
    states: &RowStateManager,
    r: &BayesEstimationRequest,
) -> Result<(String, Vec<ReportSection>)> {
    let c = gather_counts(table, states, r)?;
    let name = column_name(table, r.response);
    let proportion = r.parameter == EstimatedParameter::Proportion;
    let level = r.level;
    let what = if proportion { "Proportion" } else { "Rate" };
    let subject = if proportion {
        if c.event.is_empty() {
            match r.denominator {
                Some(d) => format!("{name} out of {}", column_name(table, d)),
                None => name.clone(),
            }
        } else {
            format!("{name} = {}", c.event)
        }
    } else {
        match r.denominator {
            Some(d) => format!("{name} per {}", column_name(table, d)),
            None => format!("{name} per row"),
        }
    };
    let mut data = TextTable::new([
        r.group.map_or_else(|| "Data".to_owned(), |g| column_name(table, g)),
        "Rows".to_owned(),
        if proportion { "Successes" } else { "Events" }.to_owned(),
        if proportion { "Trials" } else { "Exposure" }.to_owned(),
        if proportion { "Observed proportion" } else { "Observed rate" }.to_owned(),
    ]);
    for (k, g) in c.groups.iter().enumerate() {
        let (a, n) = c.totals[k];
        data.push([
            if g.is_empty() { "All rows".to_owned() } else { g.clone() },
            c.rows[k].to_string(),
            format_number(a, 8),
            format_number(n, 8),
            format_number(a / n, DIGITS),
        ]);
    }
    let mut out = vec![section(SectionKind::BayesData, "Data", SectionContent::Table(data))];
    let (prior_text, title) = if proportion {
        let p = beta_prior(r.prior)?;
        (prior_text_beta(p, r.prior), format!("Proportion of {subject}"))
    } else {
        let p = gamma_prior(r.prior)?;
        (prior_text_gamma(p, r.prior), format!("Rate of {subject}"))
    };
    let scope = if r.group.is_some() { "in each group, independently" } else { "" };
    out.push(section(
        SectionKind::BayesPrior,
        "Prior",
        SectionContent::Note(format!("{what}: {prior_text} {scope}").trim_end().to_owned()),
    ));
    if r.group.is_some() {
        let (first, second) = (c.totals[0], c.totals[1]);
        let contrast = if proportion { r.contrast } else { Contrast::Ratio };
        let (fa, fb, summary, probs, density, pa, pb) = if proportion {
            let f = conjugate::two_proportions(first, second, beta_prior(r.prior)?, contrast, level, r.rope)?;
            (
                f.first.summary,
                f.second.summary,
                f.summary,
                f.probabilities,
                f.density,
                Box::new(f.first.posterior) as Box<dyn Univariate>,
                Box::new(f.second.posterior) as Box<dyn Univariate>,
            )
        } else {
            let f = conjugate::two_rates(first, second, gamma_prior(r.prior)?, level, r.rope)?;
            (
                f.first.summary,
                f.second.summary,
                f.summary,
                f.probabilities,
                f.density,
                Box::new(f.first.posterior) as Box<dyn Univariate>,
                Box::new(f.second.posterior) as Box<dyn Univariate>,
            )
        };
        let label = contrast_label(contrast, what, &c.groups[0], &c.groups[1]);
        let mut t = summary_columns("Parameter", level);
        summary_row(&mut t, &format!("{what} {}", c.groups[0]), &fa);
        summary_row(&mut t, &format!("{what} {}", c.groups[1]), &fb);
        summary_row(&mut t, &label, &summary);
        out.push(section(SectionKind::BayesPosterior, "Posterior Summary", SectionContent::Table(t)));
        if summary.mean.is_nan() || summary.sd.is_nan() {
            out.push(section(
                SectionKind::BayesMomentNote,
                "About the Posterior Mean",
                SectionContent::Note(format!(
                    "The posterior {} of the {} does not exist (shown as —): with so few events the posterior \
                     puts too much weight near zero for the divisor. The median and the intervals are defined \
                     and describe the {}.",
                    if summary.mean.is_nan() { "mean and SD" } else { "SD" },
                    contrast_noun(contrast),
                    contrast_noun(contrast),
                )),
            ));
        }
        let short = contrast_noun(contrast);
        out.push(probabilities_section(short, &probs, None));
        out.push(section(
            SectionKind::BayesDensity,
            format!("Posterior of the {} (equal-tailed {} interval marked)", capitalize(short), pct(level)),
            SectionContent::Plot(Box::new(density_plot(
                &label,
                density,
                None,
                summary.equal_tailed,
                level,
                Some(probs.reference),
            ))),
        ));
        let gname = r.group.map(|g| column_name(table, g)).unwrap_or_default();
        out.push(group_posteriors_plot(&gname, &c.groups, [pa.as_ref(), pb.as_ref()], what));
        return Ok((format!("Bayesian Estimation: {title} by {gname}"), out));
    }
    let reference = r.reference_value();
    let (a, n) = c.totals[0];
    let (summary, probs, bf, posterior, prior) = if proportion {
        let f = conjugate::proportion(a, n, beta_prior(r.prior)?, level, reference, r.rope)?;
        let post = density_curve(&f.posterior, 200);
        let (lo, hi) = span(&post);
        let prior = f.prior.as_ref().map(|p| curve_on(p, lo.max(1e-6), hi.min(1.0 - 1e-6), 200));
        (f.summary, f.probabilities, f.bayes_factor, post, prior)
    } else {
        let f = conjugate::rate(a, n, gamma_prior(r.prior)?, level, reference, r.rope)?;
        let post = density_curve(&f.posterior, 200);
        let (lo, hi) = span(&post);
        let prior = f.prior.as_ref().map(|p| curve_on(p, lo.max(1e-12), hi, 200));
        (f.summary, f.probabilities, f.bayes_factor, post, prior)
    };
    let mut t = summary_columns("Parameter", level);
    summary_row(&mut t, what, &summary);
    out.push(section(SectionKind::BayesPosterior, "Posterior Summary", SectionContent::Table(t)));
    out.push(probabilities_section(&what.to_lowercase(), &probs, bf));
    out.push(section(
        SectionKind::BayesDensity,
        format!("Prior and Posterior (equal-tailed {} interval marked)", pct(level)),
        SectionContent::Plot(Box::new(density_plot(
            &title,
            posterior,
            prior,
            summary.equal_tailed,
            level,
            Some(reference),
        ))),
    ));
    Ok((format!("Bayesian Estimation: {title}"), out))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
}

// ---------------------------------------------------------------------------
// Bayes factor tests.

/// Which Bayes factor test.
#[derive(Debug, Clone, PartialEq)]
pub enum BayesTestKind {
    /// One-sample t test of `column` against `null`.
    OneSample {
        /// Values.
        column: usize,
        /// Hypothesized mean.
        null: f64,
    },
    /// Paired t test of `first − second`.
    Paired {
        /// First column.
        first: usize,
        /// Second column.
        second: usize,
    },
    /// Two-sample t test of `response` by a two-level `group`.
    TwoSample {
        /// Values.
        response: usize,
        /// Group column.
        group: usize,
    },
    /// Pearson correlation of `x` and `y`.
    Correlation {
        /// First column.
        x: usize,
        /// Second column.
        y: usize,
    },
    /// Association in the contingency table of two columns.
    Contingency {
        /// Row variable.
        rows: usize,
        /// Column variable.
        columns: usize,
        /// Count of identical rows.
        frequency: Option<usize>,
        /// How the table was sampled.
        sampling: Sampling,
    },
}

/// Settings of a Bayes factor test.
#[derive(Debug, Clone, PartialEq)]
pub struct BayesFactorRequest {
    /// The test.
    pub test: BayesTestKind,
    /// Prior scale: `r` of the Cauchy prior on δ (t tests), `κ` of the
    /// stretched beta prior on ρ (correlation), Dirichlet concentration `a`
    /// (contingency tables).
    pub scale: f64,
    /// Alternative hypothesis (t tests and correlations).
    pub direction: Direction,
    /// Credible level of the effect's posterior.
    pub level: f64,
}

/// Default prior scale of a t test (`√2⁄2`, "medium").
pub const DEFAULT_T_SCALE: f64 = std::f64::consts::FRAC_1_SQRT_2;

impl BayesFactorRequest {
    /// Default settings for `test`.
    #[must_use]
    pub fn new(test: BayesTestKind) -> Self {
        let scale = match test {
            BayesTestKind::OneSample { .. } | BayesTestKind::Paired { .. } | BayesTestKind::TwoSample { .. } => {
                DEFAULT_T_SCALE
            }
            BayesTestKind::Correlation { .. } | BayesTestKind::Contingency { .. } => 1.0,
        };
        Self { test, scale, direction: Direction::TwoSided, level: 0.95 }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match &self.test {
            BayesTestKind::OneSample { column, .. } => vec![*column],
            BayesTestKind::Paired { first, second } => vec![*first, *second],
            BayesTestKind::TwoSample { response, group } => vec![*response, *group],
            BayesTestKind::Correlation { x, y } => vec![*x, *y],
            BayesTestKind::Contingency { rows, columns, frequency, .. } => {
                std::iter::once(*rows).chain(std::iter::once(*columns)).chain(*frequency).collect()
            }
        }
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        let test = match &self.test {
            BayesTestKind::OneSample { column, null } => BayesTestKind::OneSample { column: f(*column), null: *null },
            BayesTestKind::Paired { first, second } => BayesTestKind::Paired { first: f(*first), second: f(*second) },
            BayesTestKind::TwoSample { response, group } => {
                BayesTestKind::TwoSample { response: f(*response), group: f(*group) }
            }
            BayesTestKind::Correlation { x, y } => BayesTestKind::Correlation { x: f(*x), y: f(*y) },
            BayesTestKind::Contingency { rows, columns, frequency, sampling } => BayesTestKind::Contingency {
                rows: f(*rows),
                columns: f(*columns),
                frequency: frequency.map(f),
                sampling: *sampling,
            },
        };
        Self { test, ..self.clone() }
    }

    fn to_json(&self, table: &DataTable) -> JsonValue {
        let c = |i: usize| col_json(table, i);
        let (kind, a, b, extra) = match &self.test {
            BayesTestKind::OneSample { column, null } => {
                ("one-sample", c(*column), JsonValue::Null, JsonValue::number(*null))
            }
            BayesTestKind::Paired { first, second } => ("paired", c(*first), c(*second), JsonValue::Null),
            BayesTestKind::TwoSample { response, group } => ("two-sample", c(*response), c(*group), JsonValue::Null),
            BayesTestKind::Correlation { x, y } => ("correlation", c(*x), c(*y), JsonValue::Null),
            BayesTestKind::Contingency { rows, columns, sampling, .. } => {
                ("contingency", c(*rows), c(*columns), s(sampling_key(*sampling)))
            }
        };
        let frequency = match &self.test {
            BayesTestKind::Contingency { frequency, .. } => opt_col_json(table, *frequency),
            _ => JsonValue::Null,
        };
        obj([
            ("analysis", s("bayes-factor")),
            ("test", s(kind)),
            ("first", a),
            ("second", b),
            (if kind == "contingency" { "sampling" } else { "null" }, extra),
            ("frequency", frequency),
            ("scale", JsonValue::number(self.scale)),
            ("direction", s(direction_key(self.direction))),
            ("level", JsonValue::number(self.level)),
        ])
    }

    fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let first = column_index(v, "first", table)?;
        let second = || column_index(v, "second", table);
        let kind = member(v, "test")?.as_str().unwrap_or_default();
        let test = match kind {
            "one-sample" => BayesTestKind::OneSample { column: first, null: number(v, "null", 0.0) },
            "paired" => BayesTestKind::Paired { first, second: second()? },
            "two-sample" => BayesTestKind::TwoSample { response: first, group: second()? },
            "correlation" => BayesTestKind::Correlation { x: first, y: second()? },
            "contingency" => {
                let name = v.get("sampling").and_then(JsonValue::as_str).unwrap_or("joint-multinomial");
                BayesTestKind::Contingency {
                    rows: first,
                    columns: second()?,
                    frequency: optional_column(v, "frequency", table)?,
                    sampling: sampling_from_key(name)
                        .ok_or_else(|| invalid(format!("bayes-factor: unknown sampling {name:?}")))?,
                }
            }
            other => return Err(invalid(format!("bayes-factor: unknown test {other:?}"))),
        };
        let name = v.get("direction").and_then(JsonValue::as_str).unwrap_or("two-sided");
        let direction =
            direction_from_key(name).ok_or_else(|| invalid(format!("bayes-factor: unknown direction {name:?}")))?;
        let default = Self::new(test.clone()).scale;
        let scale = number(v, "scale", default);
        if !(scale > 0.0 && scale.is_finite()) {
            return Err(invalid("bayes-factor: the prior scale must be positive"));
        }
        Ok(Self { test, scale, direction, level: read_level(v, "bayes-factor")? })
    }
}

/// Project-file key of a sampling plan.
#[must_use]
pub fn sampling_key(s: Sampling) -> &'static str {
    match s {
        Sampling::Poisson => "poisson",
        Sampling::JointMultinomial => "joint-multinomial",
        Sampling::FixedRows => "fixed-rows",
        Sampling::FixedColumns => "fixed-columns",
    }
}

fn sampling_from_key(key: &str) -> Option<Sampling> {
    [Sampling::Poisson, Sampling::JointMultinomial, Sampling::FixedRows, Sampling::FixedColumns]
        .into_iter()
        .find(|s| sampling_key(*s) == key)
}

/// Plain description of a sampling plan.
#[must_use]
pub fn sampling_label(s: Sampling) -> &'static str {
    match s {
        Sampling::Poisson => "Nothing fixed (Poisson counts)",
        Sampling::JointMultinomial => "Grand total fixed (joint multinomial)",
        Sampling::FixedRows => "Row totals fixed (independent multinomial rows)",
        Sampling::FixedColumns => "Column totals fixed (independent multinomial columns)",
    }
}

fn direction_key(d: Direction) -> &'static str {
    match d {
        Direction::TwoSided => "two-sided",
        Direction::Greater => "greater",
        Direction::Less => "less",
    }
}

fn direction_from_key(key: &str) -> Option<Direction> {
    [Direction::TwoSided, Direction::Greater, Direction::Less].into_iter().find(|d| direction_key(*d) == key)
}

fn hypothesis(d: Direction, what: &str) -> String {
    match d {
        Direction::TwoSided => format!("H₁: {what} ≠ 0 against H₀: {what} = 0"),
        Direction::Greater => format!("H₊: {what} > 0 against H₀: {what} = 0"),
        Direction::Less => format!("H₋: {what} < 0 against H₀: {what} = 0"),
    }
}

fn bf_table(test: &BayesFactorTest, what: &str, scale_name: &str) -> TextTable {
    let mut t = TextTable::new(["Hypotheses", "BF₁₀", "BF₀₁", "ln BF₁₀", "Evidence", scale_name]);
    t.push([
        hypothesis(test.direction, what),
        format_bf(test.bf10),
        format_bf(1.0 / test.bf10),
        format_number(test.ln_bf10, DIGITS),
        evidence_label(test.bf10).to_owned(),
        format_number(test.scale, 4),
    ]);
    t
}

/// `log₁₀ BF` against the prior scale, with the chosen scale marked and
/// the conventional evidence bands labelled.
fn robustness_plot(points: &[(f64, f64)], chosen: f64, chosen_ln_bf: f64, scale_name: &str) -> PlotSpec {
    let line: Vec<[f64; 2]> = points.iter().map(|&(x, l)| [x, l / std::f64::consts::LN_10]).collect();
    let (x0, x1) = (points.first().map_or(0.0, |p| p.0), points.last().map_or(1.0, |p| p.0));
    let lines: Vec<(f64, &str)> = vec![
        (-1.0, "1/10"),
        (-(3.0f64).log10(), "1/3"),
        (0.0, "1"),
        ((3.0f64).log10(), "3"),
        (1.0, "10"),
        (2.0, "100"),
    ];
    let lo = line.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min).min(-0.6);
    let hi = line.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max).max(0.6);
    let ticks: Vec<(f64, String)> =
        lines.iter().filter(|(v, _)| *v >= lo - 0.3 && *v <= hi + 0.3).map(|(v, l)| (*v, (*l).to_owned())).collect();
    let segments = ticks.iter().map(|(v, _)| vec![[x0, *v], [x1, *v]]).collect();
    let y = chosen_ln_bf / std::f64::consts::LN_10;
    PlotSpec {
        x_label: scale_name.to_owned(),
        y_label: "BF₁₀ (log scale)".to_owned(),
        line: Some(line),
        line_name: Some("BF₁₀ by prior scale".to_owned()),
        y_ticks: Some(ticks),
        segments,
        annotations: Some(Annotations {
            groups: vec!["Chosen scale".to_owned()],
            items: vec![(chosen, y, format!("{} = {}", scale_name, format_number(chosen, 3)), 0)],
            labels: Vec::new(),
        }),
        ..PlotSpec::default()
    }
}

fn run_factor(
    table: &DataTable,
    states: &RowStateManager,
    r: &BayesFactorRequest,
) -> Result<(String, Vec<ReportSection>)> {
    let level = r.level;
    let name = |i: usize| column_name(table, i);
    let t_stats = |x: &[f64], null: f64| -> Result<(f64, SampleStats)> {
        let st = SampleStats::of(x)?;
        if st.sd.is_nan() || st.sd <= 0.0 {
            return Err(invalid("the values do not vary"));
        }
        Ok(((st.mean - null) / (st.sd / st.n.sqrt()), st))
    };
    let mut out = Vec::new();
    let (title, test, what, scale_name) = match &r.test {
        BayesTestKind::OneSample { column, null } => {
            let (_, y) = numeric_rows(table, states, *column)?;
            let (t, st) = t_stats(&y, *null)?;
            let mut data = TextTable::new(["Column", "N", "Mean", "SD", "Hypothesized mean", "t"]);
            data.push([
                name(*column),
                format_number(st.n, 6),
                format_number(st.mean, DIGITS),
                format_number(st.sd, DIGITS),
                format_number(*null, DIGITS),
                format_number(t, DIGITS),
            ]);
            out.push(section(SectionKind::BayesData, "Data", SectionContent::Table(data)));
            let test = bayes_factor::t_test(t, TDesign::OneSample { n: st.n }, r.scale, r.direction, level)?;
            (format!("Bayes Factor t Test: {}", name(*column)), test, "δ", "Prior scale r")
        }
        BayesTestKind::Paired { first, second } => {
            let (r1, y1) = numeric_rows(table, states, *first)?;
            let (r2, y2) = numeric_rows(table, states, *second)?;
            let d: Vec<f64> =
                r1.iter().zip(&y1).filter_map(|(row, a)| r2.iter().position(|x| x == row).map(|k| a - y2[k])).collect();
            let (t, st) = t_stats(&d, 0.0)?;
            let mut data = TextTable::new(["Difference", "Pairs", "Mean", "SD", "t"]);
            data.push([
                format!("{} − {}", name(*first), name(*second)),
                format_number(st.n, 6),
                format_number(st.mean, DIGITS),
                format_number(st.sd, DIGITS),
                format_number(t, DIGITS),
            ]);
            out.push(section(SectionKind::BayesData, "Data (paired differences)", SectionContent::Table(data)));
            let test = bayes_factor::t_test(t, TDesign::OneSample { n: st.n }, r.scale, r.direction, level)?;
            (format!("Bayes Factor Paired t Test: {} − {}", name(*first), name(*second)), test, "δ", "Prior scale r")
        }
        BayesTestKind::TwoSample { response, group } => {
            let (rows, y) = numeric_rows(table, states, *response)?;
            let gc = table.column(*group).ok_or_else(|| invalid("no such column"))?;
            let keep: Vec<usize> = (0..rows.len()).filter(|&k| !gc.level_text(rows[k]).trim().is_empty()).collect();
            let rows: Vec<usize> = keep.iter().map(|&k| rows[k]).collect();
            let y: Vec<f64> = keep.iter().map(|&k| y[k]).collect();
            let (levels, codes) = levels_of(table, *group, &rows)?;
            if levels.len() != 2 {
                return Err(invalid(format!(
                    "the group column {:?} must have exactly two values; it has {}",
                    gc.name,
                    levels.len()
                )));
            }
            let part =
                |k: usize| -> Vec<f64> { y.iter().zip(&codes).filter(|(_, c)| **c == k).map(|(v, _)| *v).collect() };
            let (a, b) = (SampleStats::of(&part(0))?, SampleStats::of(&part(1))?);
            let df = a.n + b.n - 2.0;
            let sp = (((a.n - 1.0) * a.sd * a.sd + (b.n - 1.0) * b.sd * b.sd) / df).sqrt();
            if sp.is_nan() || sp <= 0.0 {
                return Err(invalid("the values do not vary"));
            }
            let t = (a.mean - b.mean) / (sp * (1.0 / a.n + 1.0 / b.n).sqrt());
            let mut data = TextTable::new([gc.name.as_str(), "N", "Mean", "SD"].map(str::to_owned));
            for (l, st) in levels.iter().zip([a, b]) {
                data.push([
                    l.clone(),
                    format_number(st.n, 6),
                    format_number(st.mean, DIGITS),
                    format_number(st.sd, DIGITS),
                ]);
            }
            out.push(section(
                SectionKind::BayesData,
                format!(
                    "Data (pooled SD {}; t = {} on {} DF)",
                    format_number(sp, DIGITS),
                    format_number(t, DIGITS),
                    format_number(df, 6)
                ),
                SectionContent::Table(data),
            ));
            let test = bayes_factor::t_test(t, TDesign::TwoSample { n1: a.n, n2: b.n }, r.scale, r.direction, level)?;
            let title = format!("Bayes Factor t Test: {} by {}", name(*response), gc.name);
            (title, test, "δ", "Prior scale r")
        }
        BayesTestKind::Correlation { x, y } => {
            let (rx, vx) = numeric_rows(table, states, *x)?;
            let (ry, vy) = numeric_rows(table, states, *y)?;
            let pairs: Vec<(f64, f64)> = rx
                .iter()
                .zip(&vx)
                .filter_map(|(row, a)| ry.iter().position(|q| q == row).map(|k| (*a, vy[k])))
                .collect();
            let n = pairs.len() as f64;
            if n < 3.0 {
                return Err(invalid("a correlation needs at least three complete pairs"));
            }
            let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
            let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
            for (a, b) in &pairs {
                sxy += (a - mx) * (b - my);
                sxx += (a - mx).powi(2);
                syy += (b - my).powi(2);
            }
            let rr = sxy / (sxx * syy).sqrt();
            if !rr.is_finite() {
                return Err(invalid("a column does not vary"));
            }
            let mut data = TextTable::new(["Columns", "Pairs", "Pearson r"]);
            data.push([format!("{} and {}", name(*x), name(*y)), format_number(n, 6), format_number(rr, DIGITS)]);
            out.push(section(SectionKind::BayesData, "Data", SectionContent::Table(data)));
            let test = bayes_factor::correlation(rr, n, r.scale, r.direction, level)?;
            (format!("Bayes Factor Correlation: {} and {}", name(*x), name(*y)), test, "ρ", "Prior width κ")
        }
        BayesTestKind::Contingency { rows, columns, frequency, sampling } => {
            return contingency(table, states, r, *rows, *columns, *frequency, *sampling);
        }
    };
    out.push(section(
        SectionKind::BayesFactor,
        "Bayes Factor",
        SectionContent::Table(bf_table(&test, what, scale_name)),
    ));
    let prior_note = if matches!(r.test, BayesTestKind::Correlation { .. }) {
        format!(
            "ρ ~ stretched Beta(1/κ, 1/κ) on (−1, 1) with κ = {} (κ = 1: uniform), under H₁. Ly, Verhagen & \
             Wagenmakers (2016).",
            format_number(r.scale, 4)
        )
    } else {
        format!(
            "δ ~ Cauchy(0, r) with r = {} under H₁ (JZS prior: a Cauchy on the standardized effect, Jeffreys on \
             the variance). Rouder et al. (2009).",
            format_number(r.scale, 4)
        )
    };
    out.push(section(SectionKind::BayesPrior, "Prior", SectionContent::Note(prior_note)));
    let mut t = summary_columns("Parameter", level);
    summary_row(&mut t, what, &test.summary);
    out.push(section(
        SectionKind::BayesPosterior,
        format!("Posterior of {what} (under the two-sided alternative)"),
        SectionContent::Table(t),
    ));
    let posterior = test.posterior.curve();
    let prior: Vec<[f64; 2]> = if what == "ρ" {
        let a = 1.0 / r.scale;
        let ln_norm = (2.0 * a - 1.0) * std::f64::consts::LN_2 + ln_beta(a, a);
        posterior.iter().map(|p| [p[0], ((a - 1.0) * (1.0 - p[0] * p[0]).ln() - ln_norm).exp()]).collect()
    } else {
        let sc = r.scale;
        posterior.iter().map(|p| [p[0], 1.0 / (std::f64::consts::PI * sc * (1.0 + (p[0] / sc).powi(2)))]).collect()
    };
    out.push(section(
        SectionKind::BayesDensity,
        format!("Prior and Posterior of {what} (equal-tailed {} interval marked)", pct(level)),
        SectionContent::Plot(Box::new(density_plot(
            what,
            posterior,
            Some(prior),
            test.summary.equal_tailed,
            level,
            Some(0.0),
        ))),
    ));
    out.push(section(
        SectionKind::BayesRobustness,
        format!("Robustness: BF₁₀ for a range of prior scales (the chosen {} marked)", scale_name.to_lowercase()),
        SectionContent::Plot(Box::new(robustness_plot(&test.robustness, test.scale, test.ln_bf10, scale_name))),
    ));
    Ok((title, out))
}

#[allow(clippy::too_many_arguments)]
fn contingency(
    table: &DataTable,
    states: &RowStateManager,
    r: &BayesFactorRequest,
    row_col: usize,
    col_col: usize,
    frequency: Option<usize>,
    sampling: Sampling,
) -> Result<(String, Vec<ReportSection>)> {
    if row_col == col_col {
        return Err(invalid("choose two different columns"));
    }
    let rc = table.column(row_col).ok_or_else(|| invalid("no such column"))?;
    let cc = table.column(col_col).ok_or_else(|| invalid("no such column"))?;
    let freq = match frequency {
        Some(f) => Some(
            table
                .column(f)
                .and_then(|c| c.as_numeric())
                .ok_or_else(|| invalid("the frequency column must be numeric"))?
                .to_vec(),
        ),
        None => None,
    };
    let rows: Vec<usize> = (0..table.n_rows())
        .filter(|&i| {
            !states.is_excluded(i)
                && !rc.level_text(i).trim().is_empty()
                && !cc.level_text(i).trim().is_empty()
                && freq.as_ref().is_none_or(|f| f[i].is_finite() && f[i] >= 0.0)
        })
        .collect();
    let (rl, rcode) = levels_of(table, row_col, &rows)?;
    let (cl, ccode) = levels_of(table, col_col, &rows)?;
    if rl.len() < 2 || cl.len() < 2 {
        return Err(invalid("each column needs at least two values"));
    }
    let mut counts = vec![vec![0.0; cl.len()]; rl.len()];
    for (k, &i) in rows.iter().enumerate() {
        counts[rcode[k]][ccode[k]] += freq.as_ref().map_or(1.0, |f| f[i]);
    }
    let mut data = TextTable::new(
        std::iter::once(format!("{} \\ {}", rc.name, cc.name)).chain(cl.iter().cloned()).chain(["Total".to_owned()]),
    );
    for (i, l) in rl.iter().enumerate() {
        let total: f64 = counts[i].iter().sum();
        data.push(
            std::iter::once(l.clone())
                .chain(counts[i].iter().map(|v| format_number(*v, 8)))
                .chain([format_number(total, 8)]),
        );
    }
    let mut out = vec![section(SectionKind::BayesData, "Counts", SectionContent::Table(data))];
    let mut t = TextTable::new(["Sampling plan", "BF₁₀", "BF₀₁", "ln BF₁₀", "Evidence", ""]);
    let mut chosen = (0.0, 0.0);
    for plan in [Sampling::FixedRows, Sampling::FixedColumns, Sampling::JointMultinomial, Sampling::Poisson] {
        let ln = bayes_factor::ln_contingency(&counts, plan, r.scale)?;
        let bf = ln.exp();
        if plan == sampling {
            chosen = (bf, ln);
        }
        t.push([
            sampling_label(plan).to_owned(),
            format_bf(bf),
            format_bf(1.0 / bf),
            format_number(ln, DIGITS),
            evidence_label(bf).to_owned(),
            if plan == sampling { "chosen" } else { "" }.to_owned(),
        ]);
    }
    out.push(section(
        SectionKind::BayesFactor,
        format!(
            "Bayes Factor: association against independence (chosen plan: BF₁₀ = {}, {})",
            format_bf(chosen.0),
            evidence_label(chosen.0)
        ),
        SectionContent::Table(t),
    ));
    out.push(section(
        SectionKind::BayesPrior,
        "Prior",
        SectionContent::Note(format!(
            "Dirichlet priors with concentration a = {} on the cell probabilities under association, and matching \
             priors on the margins under independence (Gunel & Dickey 1974; Jamil et al. 2017).",
            format_number(r.scale, 4)
        )),
    ));
    let grid: Vec<f64> = (0..=40).map(|i| 0.1 * 50f64.powf(f64::from(i) / 40.0)).collect();
    let mut points = Vec::new();
    for a in grid {
        points.push((a, bayes_factor::ln_contingency(&counts, sampling, a)?));
    }
    out.push(section(
        SectionKind::BayesRobustness,
        "Robustness: BF₁₀ for a range of prior concentrations (the chosen a marked)",
        SectionContent::Plot(Box::new(robustness_plot(&points, r.scale, chosen.1, "Prior concentration a"))),
    ));
    Ok((format!("Bayes Factor Contingency Table: {} by {}", rc.name, cc.name), out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn data() -> DataTable {
        let y = vec![5.1, 4.8, 6.0, 5.5, 5.9, 6.3, 4.9, 5.2, 6.8, 7.1, 6.4, 7.5, 6.9, 7.7, 6.6, 7.0];
        let g: Vec<String> = (0..16).map(|i| if i < 8 { "A" } else { "B" }.to_owned()).collect();
        let x: Vec<f64> = (0..16).map(|i| f64::from(i) * 0.5 + if i % 3 == 0 { 1.0 } else { 0.0 }).collect();
        let outcome: Vec<String> = (0..16).map(|i| if i % 3 == 0 { "No" } else { "Yes" }.to_owned()).collect();
        let events: Vec<f64> = (0..16).map(|i| f64::from(i % 4)).collect();
        let years: Vec<f64> = (0..16).map(|i| 1.0 + f64::from(i % 3)).collect();
        DataTable::new(
            "t",
            vec![
                Column::numeric("Y", y),
                Column::text("Group", g),
                Column::numeric("X", x),
                Column::text("Outcome", outcome),
                Column::numeric("Events", events),
                Column::numeric("Years", years),
            ],
        )
        .unwrap()
    }

    fn run(t: &DataTable, r: BayesRequest) -> BayesReport {
        let rep = BayesReport::run(t, &RowStateManager::new(t.n_rows()), &r).unwrap();
        // Round trip through the project-file form.
        let v = r.to_json(t);
        let key = v.get("analysis").and_then(JsonValue::as_str).unwrap().to_owned();
        assert_eq!(BayesRequest::from_json(&key, &v, t).unwrap(), r);
        rep
    }

    #[test]
    fn estimation_reports() {
        let t = data();
        let r = run(&t, BayesRequest::Estimation(BayesEstimationRequest::new(EstimatedParameter::Mean, 0)));
        assert_eq!(r.title(), "Bayesian Estimation: Mean of Y");
        let s = r.sections();
        let SectionContent::Table(post) = &s[2].content else { panic!() };
        // Jeffreys: the posterior mean of μ is the sample mean.
        let mean =
            [5.1, 4.8, 6.0, 5.5, 5.9, 6.3, 4.9, 5.2, 6.8, 7.1, 6.4, 7.5, 6.9, 7.7, 6.6, 7.0].iter().sum::<f64>() / 16.0;
        assert_eq!(post.rows[0][1], format_number(mean, DIGITS));
        let two = BayesEstimationRequest { group: Some(1), ..BayesEstimationRequest::new(EstimatedParameter::Mean, 0) };
        let r = run(&t, BayesRequest::Estimation(two));
        assert!(r.title().ends_with("by Group"));
        let mut prop = BayesEstimationRequest::new(EstimatedParameter::Proportion, 3);
        prop.prior = BayesPrior::Flat;
        prop.rope = Some((0.45, 0.55));
        let r = run(&t, BayesRequest::Estimation(prop.clone()));
        assert_eq!(r.title(), "Bayesian Estimation: Proportion of Outcome = Yes");
        let SectionContent::Table(post) = &r.sections()[2].content else { panic!() };
        // 10 of 16 with Beta(1, 1): posterior Beta(11, 7), mean 11/18.
        assert_eq!(post.rows[0][1], format_number(11.0 / 18.0, DIGITS));
        prop.group = Some(1);
        prop.contrast = Contrast::OddsRatio;
        prop.rope = None;
        let r = run(&t, BayesRequest::Estimation(prop));
        assert!(r.sections().iter().any(|s| s.kind == SectionKind::BayesGroupPosteriors));
        let mut rate = BayesEstimationRequest::new(EstimatedParameter::Rate, 4);
        rate.denominator = Some(5);
        rate.prior = BayesPrior::Informative { mean: 1.0, spread: 0.5 };
        let r = run(&t, BayesRequest::Estimation(rate));
        assert_eq!(r.title(), "Bayesian Estimation: Rate of Events per Years");
        assert!(r.sections()[3].title.contains("Savage–Dickey"));
        let paired =
            BayesEstimationRequest { second: Some(2), ..BayesEstimationRequest::new(EstimatedParameter::Mean, 0) };
        let r = run(&t, BayesRequest::Estimation(paired));
        assert_eq!(r.title(), "Bayesian Estimation: Mean of Y − X");
    }

    #[test]
    fn bayes_factor_reports() {
        let t = data();
        for test in [
            BayesTestKind::OneSample { column: 0, null: 6.0 },
            BayesTestKind::Paired { first: 0, second: 2 },
            BayesTestKind::TwoSample { response: 0, group: 1 },
            BayesTestKind::Correlation { x: 0, y: 2 },
            BayesTestKind::Contingency { rows: 1, columns: 3, frequency: None, sampling: Sampling::FixedRows },
        ] {
            let mut r = BayesFactorRequest::new(test);
            r.direction = if matches!(r.test, BayesTestKind::Contingency { .. }) {
                Direction::TwoSided
            } else {
                Direction::Greater
            };
            let rep = run(&t, BayesRequest::Factor(r));
            assert!(rep.sections().iter().any(|s| s.kind == SectionKind::BayesRobustness), "{}", rep.title());
            assert!(rep.sections().iter().any(|s| s.kind == SectionKind::BayesFactor));
        }
        // The two-sample BF matches the engine on the pooled t.
        let rep =
            run(&t, BayesRequest::Factor(BayesFactorRequest::new(BayesTestKind::TwoSample { response: 0, group: 1 })));
        let SectionContent::Table(bf) = &rep.sections()[1].content else { panic!() };
        assert!(bf.rows[0][0].starts_with("H₁"));
        assert_eq!(rep.title(), "Bayes Factor t Test: Y by Group");
    }

    /// No events in the second group: the rate ratio's posterior mean and SD
    /// do not exist, so they show as "—" with a note, not as a large number.
    #[test]
    fn zero_events_leave_the_ratio_mean_undefined() {
        let events = vec![1.0, 2.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0];
        let g: Vec<String> = (0..8).map(|i| if i < 4 { "A" } else { "B" }.to_owned()).collect();
        let t = DataTable::new(
            "z",
            vec![Column::numeric("Events", events), Column::text("Group", g), Column::numeric("Years", vec![1.0; 8])],
        )
        .unwrap();
        let mut rate = BayesEstimationRequest::new(EstimatedParameter::Rate, 0);
        rate.denominator = Some(2);
        rate.group = Some(1);
        let r = run(&t, BayesRequest::Estimation(rate));
        let s = r.sections();
        let post = s.iter().find(|x| x.kind == SectionKind::BayesPosterior).unwrap();
        let SectionContent::Table(post) = &post.content else { panic!() };
        let ratio = post.rows.last().unwrap();
        assert_eq!((ratio[1].as_str(), ratio[3].as_str()), ("—", "—"), "{ratio:?}");
        assert_ne!(ratio[2], "—", "the median is defined");
        assert!(s.iter().any(|x| x.kind == SectionKind::BayesMomentNote));
    }
}
