//! Declaring an existing table as a designed experiment: which columns are
//! factors (numeric, with their low and high settings, by default the
//! smallest and largest values in the data; or categorical with two levels,
//! coded −1 and +1 in sorted order) and which are responses.
//!
//! The report describes the design the table holds: the factors and their
//! levels, the runs (distinct settings, replicated settings and the
//! pure-error degrees of freedom they give, centre runs), how correlated the
//! coded factor columns are (zero for an orthogonal design), and which
//! polynomial models the runs can estimate, with each model's D-efficiency
//! `100 · |XᵀX|^{1/p} / n` in coded units (−1 at the low setting, +1 at the
//! high; [`stat_doe::d_efficiency`]). The declaration is saved with the
//! project, so the design's description travels with the data.
//!
//! A design run with restricted randomization (a split-plot or
//! split-split-plot experiment) also names its whole-plot column and, for a
//! split-split-plot design, its subplot column, and may record the model it
//! was built for. The report then lists the plots and which factors stay
//! fixed within them, and [`split_plot_analysis`] turns the declaration into
//! the mixed model that analyses it: the model's fixed effects with random
//! intercepts for the whole plots (and the subplots within them), fitted by
//! REML so that whole-plot effects are tested against whole-plot error.

use crate::format::format_number;
use crate::project::{invalid, member, obj, s};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use crate::robust_design::column_list;
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_doe::{ModelFormula, d_efficiency, log_det_information};

/// Significant digits in report tables.
const DIGITS: usize = 5;

/// A factor column with optional settings coded −1 and +1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeclaredFactor {
    /// Numeric column.
    pub column: usize,
    /// Setting coded −1 (`None`: the smallest value in the data).
    pub low: Option<f64>,
    /// Setting coded +1 (`None`: the largest value in the data).
    pub high: Option<f64>,
}

/// A table declared as a design.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignDeclarationRequest {
    /// Factors.
    pub factors: Vec<DeclaredFactor>,
    /// Response columns.
    pub responses: Vec<usize>,
    /// Column identifying the whole plots of a design with restricted
    /// randomization (split-plot), if any.
    pub whole_plot: Option<usize>,
    /// Column identifying the subplots (split-split-plot), if any.
    pub subplot: Option<usize>,
    /// The model formula the design was built for (over the factor names,
    /// see [`ModelFormula::parse`]); empty when not recorded.
    pub model: String,
}

impl DesignDeclarationRequest {
    /// Factors with ranges from the data.
    #[must_use]
    pub fn new(factors: &[usize], responses: &[usize]) -> Self {
        Self {
            factors: factors.iter().map(|&column| DeclaredFactor { column, low: None, high: None }).collect(),
            responses: responses.to_vec(),
            whole_plot: None,
            subplot: None,
            model: String::new(),
        }
    }

    /// Columns used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.factors
            .iter()
            .map(|f| f.column)
            .chain(self.responses.iter().copied())
            .chain(self.whole_plot)
            .chain(self.subplot)
            .collect()
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        self.factors.iter_mut().for_each(|d| d.column = f(d.column));
        self.responses.iter_mut().for_each(|c| *c = f(*c));
        self.whole_plot = self.whole_plot.map(&f);
        self.subplot = self.subplot.map(&f);
        self
    }

    /// The project-file form (`"analysis": "declared-design"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let num = |v: Option<f64>| v.map_or(JsonValue::Null, JsonValue::number);
        let mut value = obj([
            ("analysis", s("declared-design")),
            (
                "factors",
                JsonValue::Array(
                    self.factors
                        .iter()
                        .map(|f| obj([("column", name(f.column)), ("low", num(f.low)), ("high", num(f.high))]))
                        .collect(),
                ),
            ),
            ("responses", JsonValue::Array(self.responses.iter().map(|&i| name(i)).collect())),
        ]);
        // Restricted randomization only when declared, so older files match.
        if let JsonValue::Object(members) = &mut value {
            if let Some(w) = self.whole_plot {
                members.push(("whole_plot".to_owned(), name(w)));
            }
            if let Some(sp) = self.subplot {
                members.push(("subplot".to_owned(), name(sp)));
            }
            if !self.model.is_empty() {
                members.push(("model".to_owned(), s(&self.model)));
            }
        }
        value
    }

    /// Read the project-file form. Factors may also be given as plain column
    /// names (ranges from the data).
    ///
    /// # Errors
    /// Missing or unknown columns.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let list = member(v, "factors")?.as_array().ok_or_else(|| invalid("factors: expected a list"))?;
        let find = |name: Option<&str>| -> Result<usize> {
            let name = name.ok_or_else(|| invalid("factors: expected column names"))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let factors = list
            .iter()
            .map(|f| match f {
                JsonValue::String(n) => Ok(DeclaredFactor { column: find(Some(n))?, low: None, high: None }),
                other => Ok(DeclaredFactor {
                    column: find(other.get("column").and_then(JsonValue::as_str))?,
                    low: other.get("low").and_then(JsonValue::as_f64),
                    high: other.get("high").and_then(JsonValue::as_f64),
                }),
            })
            .collect::<Result<_>>()?;
        let responses = if v.get("responses").is_some() { column_list(v, table, "responses")? } else { Vec::new() };
        let optional = |key: &str| -> Result<Option<usize>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(None),
                Some(j) => find(j.as_str()).map(Some),
            }
        };
        Ok(Self {
            factors,
            responses,
            whole_plot: optional("whole_plot")?,
            subplot: optional("subplot")?,
            model: v.get("model").and_then(JsonValue::as_str).unwrap_or_default().to_owned(),
        })
    }
}

/// A model and what the runs say about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelSupport {
    /// Model name.
    pub name: &'static str,
    /// Parameters, with the intercept.
    pub terms: usize,
    /// Whether every parameter can be estimated.
    pub estimable: bool,
    /// Runs minus parameters (when estimable).
    pub error_df: usize,
    /// D-efficiency in percent (when estimable).
    pub d_efficiency: f64,
}

/// A computed design declaration.
#[derive(Debug, Clone)]
pub struct DeclaredDesign {
    factor_names: Vec<String>,
    response_names: Vec<String>,
    /// (low, high) of each factor.
    ranges: Vec<(f64, f64)>,
    /// Level names (coded −1, +1) of each two-level categorical factor.
    labels: Vec<Option<(String, String)>>,
    /// Distinct values of each factor.
    levels: Vec<usize>,
    /// Rows with a value outside its range, per factor.
    outside: Vec<usize>,
    /// Coded runs (non-excluded rows with every factor).
    coded: Vec<Vec<f64>>,
    /// Non-missing values of each response among those rows.
    response_counts: Vec<usize>,
    n_excluded: usize,
    /// Whole plots and subplots, when declared.
    plots: Option<Plots>,
}

/// The plot structure of a declared split-plot design.
#[derive(Debug, Clone)]
struct Plots {
    whole_plot: String,
    subplot: Option<String>,
    /// Number of whole plots and their smallest and largest size.
    whole: (usize, usize, usize),
    /// The same for the subplots.
    sub: Option<(usize, usize, usize)>,
    /// Factors constant within every whole plot.
    fixed_whole: Vec<String>,
    /// Factors constant within every subplot but not every whole plot.
    fixed_sub: Vec<String>,
    /// The recorded model, and the whole-plot error df it leaves.
    model: Option<(String, i64)>,
}

/// Number of groups of `keys` (one per run) and their smallest and largest
/// size, and whether `values` is constant within each group.
fn group_summary(keys: &[Vec<String>]) -> (usize, usize, usize) {
    let mut sorted: Vec<&Vec<String>> = keys.iter().collect();
    sorted.sort();
    let mut sizes = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let j = (i..sorted.len()).find(|&j| sorted[j] != sorted[i]).unwrap_or(sorted.len());
        sizes.push(j - i);
        i = j;
    }
    (sizes.len(), sizes.iter().copied().min().unwrap_or(0), sizes.iter().copied().max().unwrap_or(0))
}

fn constant_within(keys: &[Vec<String>], values: &[f64]) -> bool {
    let mut first: std::collections::HashMap<&Vec<String>, f64> = std::collections::HashMap::new();
    keys.iter().zip(values).all(|(k, &v)| (*first.entry(k).or_insert(v) - v).abs() <= 1e-9 * v.abs().max(1.0))
}

impl DeclaredDesign {
    /// Code the runs of the declared design.
    ///
    /// # Errors
    /// No factors, a column used twice, a categorical factor without exactly
    /// two levels, a non-numeric response, a factor whose low and high
    /// settings coincide, or fewer than two runs.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &DesignDeclarationRequest) -> Result<Self> {
        if request.factors.is_empty() {
            return Err(invalid("declare at least one factor"));
        }
        let all = request.columns();
        if all.iter().enumerate().any(|(i, c)| all[..i].contains(c)) {
            return Err(invalid("each column can have only one role"));
        }
        let numeric = |i: usize| -> Result<(String, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
            Ok((c.name.clone(), v))
        };
        // A numeric continuous factor keeps its values; a categorical one
        // (text, or numeric codes marked nominal or ordinal) must have two
        // levels, which become 0 and 1 in sorted order.
        let mut labels: Vec<Option<(String, String)>> = Vec::new();
        let mut owned: Vec<(String, Vec<f64>)> = Vec::new();
        for f in &request.factors {
            let c = table.column(f.column).ok_or_else(|| invalid(format!("no column {}", f.column)))?;
            match c.as_numeric().filter(|_| c.level == stat_data::MeasurementLevel::Continuous) {
                Some(v) => {
                    owned.push((c.name.clone(), v.to_vec()));
                    labels.push(None);
                }
                None => {
                    let text: Vec<String> = (0..table.n_rows()).map(|r| c.level_text(r)).collect();
                    let mut distinct: Vec<&String> = text.iter().filter(|t| !t.is_empty()).collect();
                    distinct.sort_by(|a, b| stat_data::table_ops::natural_order(a, b));
                    distinct.dedup();
                    if distinct.len() != 2 {
                        return Err(invalid(format!(
                            "{} is categorical with {} levels: a categorical factor needs exactly two",
                            c.name,
                            distinct.len()
                        )));
                    }
                    let (lo, hi) = (distinct[0].clone(), distinct[1].clone());
                    let v = text
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
                    owned.push((c.name.clone(), v));
                    labels.push(Some((lo, hi)));
                }
            }
        }
        let factors: Vec<(String, &[f64])> = owned.iter().map(|(n, v)| (n.clone(), v.as_slice())).collect();
        let responses: Vec<(String, &[f64])> = request.responses.iter().map(|&r| numeric(r)).collect::<Result<_>>()?;
        let complete: Vec<usize> =
            (0..table.n_rows()).filter(|&r| factors.iter().all(|(_, v)| v[r].is_finite())).collect();
        let rows: Vec<usize> = complete.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        if rows.len() < 2 {
            return Err(invalid("fewer than two runs have every factor setting"));
        }
        let mut ranges = Vec::new();
        let mut levels = Vec::new();
        let mut outside = Vec::new();
        for ((f, (name, v)), label) in request.factors.iter().zip(&factors).zip(&labels) {
            let (lo, hi) =
                rows.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &r| (a.min(v[r]), b.max(v[r])));
            let (low, high) = if label.is_some() { (0.0, 1.0) } else { (f.low.unwrap_or(lo), f.high.unwrap_or(hi)) };
            if !(low.is_finite() && high.is_finite()) || low == high {
                return Err(invalid(format!("{name} needs different low and high settings (it may not vary)")));
            }
            let mut distinct: Vec<f64> = rows.iter().map(|&r| v[r]).collect();
            distinct.sort_by(f64::total_cmp);
            distinct.dedup();
            levels.push(distinct.len());
            let (a, b) = (low.min(high), low.max(high));
            outside.push(
                rows.iter()
                    .filter(|&&r| v[r] < a - 1e-12 * a.abs().max(1.0) || v[r] > b + 1e-12 * b.abs().max(1.0))
                    .count(),
            );
            ranges.push((low, high));
        }
        let coded = rows
            .iter()
            .map(|&r| {
                factors
                    .iter()
                    .zip(&ranges)
                    .map(|((_, v), &(low, high))| (2.0 * v[r] - (low + high)) / (high - low))
                    .collect()
            })
            .collect();
        Self {
            factor_names: factors.iter().map(|f| f.0.clone()).collect(),
            response_names: responses.iter().map(|r| r.0.clone()).collect(),
            ranges,
            labels,
            levels,
            outside,
            coded,
            response_counts: responses
                .iter()
                .map(|(_, v)| rows.iter().filter(|&&r| v[r].is_finite()).count())
                .collect(),
            n_excluded: complete.len() - rows.len(),
            plots: None,
        }
        .with_plots(table, request, &rows)
    }

    /// Read the declared whole plots and subplots of `rows`.
    fn with_plots(mut self, table: &DataTable, request: &DesignDeclarationRequest, rows: &[usize]) -> Result<Self> {
        let Some(wp) = request.whole_plot else {
            if request.subplot.is_some() {
                return Err(invalid("a subplot column needs a whole-plot column"));
            }
            return Ok(self);
        };
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let labels = |i: usize| -> Result<Vec<String>> {
            let c = column(i)?;
            let v: Vec<String> = rows.iter().map(|&r| c.level_text(r)).collect();
            if v.iter().any(|s| s.trim().is_empty()) {
                return Err(invalid(format!("{} is missing for some runs", c.name)));
            }
            Ok(v)
        };
        let whole: Vec<Vec<String>> = labels(wp)?.into_iter().map(|l| vec![l]).collect();
        // Subplots are nested in whole plots: a subplot is a (whole plot,
        // subplot label) pair, so labels may restart in each whole plot.
        let sub: Option<Vec<Vec<String>>> = request
            .subplot
            .map(|sp| labels(sp).map(|l| whole.iter().zip(l).map(|(w, s)| vec![w[0].clone(), s]).collect()))
            .transpose()?;
        let k = self.factor_names.len();
        let column_of = |j: usize| -> Vec<f64> { self.coded.iter().map(|r| r[j]).collect() };
        let fixed_whole: Vec<usize> = (0..k).filter(|&j| constant_within(&whole, &column_of(j))).collect();
        let fixed_sub: Vec<usize> = match &sub {
            Some(sub) => (0..k).filter(|&j| !fixed_whole.contains(&j) && constant_within(sub, &column_of(j))).collect(),
            None => Vec::new(),
        };
        let name = |i: usize| column(i).map(|c| c.name.clone());
        let whole_summary = group_summary(&whole);
        let model = if request.model.trim().is_empty() {
            None
        } else {
            let f = ModelFormula::parse(&request.model, &self.factor_names)?;
            let terms = f
                .terms()
                .iter()
                .filter(|t| t.powers.iter().enumerate().all(|(j, &p)| p == 0 || fixed_whole.contains(&j)))
                .count();
            Some((f.describe(&self.factor_names), whole_summary.0 as i64 - terms as i64))
        };
        self.plots = Some(Plots {
            whole_plot: name(wp)?,
            subplot: request.subplot.map(name).transpose()?,
            whole: whole_summary,
            sub: sub.as_deref().map(group_summary),
            fixed_whole: fixed_whole.iter().map(|&j| self.factor_names[j].clone()).collect(),
            fixed_sub: fixed_sub.iter().map(|&j| self.factor_names[j].clone()).collect(),
            model,
        });
        Ok(self)
    }

    /// Runs in coded units.
    #[must_use]
    pub fn coded_runs(&self) -> &[Vec<f64>] {
        &self.coded
    }

    /// Factor column names.
    #[must_use]
    pub fn factor_names(&self) -> &[String] {
        &self.factor_names
    }

    /// Settings coded −1 and +1 of each factor (0 and 1 for a categorical
    /// factor, whose level names are in [`level_labels`](Self::level_labels)).
    #[must_use]
    pub fn ranges(&self) -> &[(f64, f64)] {
        &self.ranges
    }

    /// Level names coded −1 and +1 of each categorical factor (`None` for a
    /// numeric one).
    #[must_use]
    pub fn level_labels(&self) -> &[Option<(String, String)>] {
        &self.labels
    }

    /// Main-effects, two-factor-interaction and quadratic models.
    #[must_use]
    pub fn model_support(&self) -> Vec<ModelSupport> {
        let k = self.factor_names.len();
        let n = self.coded.len();
        let mut models = vec![("Main effects", ModelFormula::main_effects(k))];
        if k > 1 {
            models.push(("Main effects and two-factor interactions", ModelFormula::interactions(k)));
        }
        models.push(("Full quadratic (response surface)", ModelFormula::quadratic(k)));
        models
            .into_iter()
            .map(|(name, f)| {
                let p = f.n_terms();
                let estimable = n >= p && log_det_information(&f, &self.coded).is_finite();
                ModelSupport {
                    name,
                    terms: p,
                    estimable,
                    error_df: if estimable { n - p } else { 0 },
                    d_efficiency: if estimable { d_efficiency(&f, &self.coded) } else { f64::NAN },
                }
            })
            .collect()
    }

    /// Distinct settings and how many runs each has.
    fn distinct_settings(&self) -> Vec<usize> {
        let mut keys: Vec<Vec<i64>> =
            self.coded.iter().map(|r| r.iter().map(|x| (x * 1e9).round() as i64).collect()).collect();
        keys.sort();
        let mut counts = Vec::new();
        let mut i = 0;
        while i < keys.len() {
            let j = (i..keys.len()).find(|&j| keys[j] != keys[i]).unwrap_or(keys.len());
            counts.push(j - i);
            i = j;
        }
        counts
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!(
            "Declared Design: {} factor{}",
            self.factor_names.len(),
            if self.factor_names.len() == 1 { "" } else { "s" }
        )
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut roles =
            TextTable::new(["Role", "Column", "Low (−1)", "High (+1)", "Levels in data", "Runs outside range"]);
        for (j, name) in self.factor_names.iter().enumerate() {
            let (low, high) = match &self.labels[j] {
                Some((lo, hi)) => (lo.clone(), hi.clone()),
                None => (format_number(self.ranges[j].0, DIGITS), format_number(self.ranges[j].1, DIGITS)),
            };
            roles.push([
                "Factor".to_owned(),
                name.clone(),
                low,
                high,
                self.levels[j].to_string(),
                self.outside[j].to_string(),
            ]);
        }
        for (name, n) in self.response_names.iter().zip(&self.response_counts) {
            roles.push(["Response".to_owned(), name.clone(), "".into(), "".into(), format!("{n} values"), "".into()]);
        }
        let counts = self.distinct_settings();
        let n = self.coded.len();
        let mut runs = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| runs.push([a.to_owned(), b]);
        row("Runs", n.to_string());
        row("Distinct factor settings", counts.len().to_string());
        row("Settings run more than once", counts.iter().filter(|&&c| c > 1).count().to_string());
        row("Pure-error degrees of freedom (from repeated settings)", (n - counts.len()).to_string());
        row(
            "Center runs (every factor midway)",
            self.coded.iter().filter(|r| r.iter().all(|x| x.abs() < 1e-9)).count().to_string(),
        );
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }

        let k = self.factor_names.len();
        let mut header = vec![String::new()];
        header.extend(self.factor_names.iter().cloned());
        let mut corr = TextTable::new(header);
        let columns: Vec<Vec<f64>> = (0..k).map(|j| self.coded.iter().map(|r| r[j]).collect()).collect();
        let mut max_abs: f64 = 0.0;
        for a in 0..k {
            let mut line = vec![self.factor_names[a].clone()];
            for b in 0..k {
                let r = correlation(&columns[a], &columns[b]);
                if a != b && r.is_finite() {
                    max_abs = max_abs.max(r.abs());
                }
                line.push(format_number(r, 3));
            }
            corr.push(line);
        }
        let mut models = TextTable::new(["Model", "Parameters", "Estimable", "Error DF", "D-efficiency (%)"]);
        for m in self.model_support() {
            models.push([
                m.name.to_owned(),
                m.terms.to_string(),
                if m.estimable { "Yes" } else { "No" }.to_owned(),
                if m.estimable { m.error_df.to_string() } else { "—".to_owned() },
                format_number(m.d_efficiency, 4),
            ]);
        }
        let note = if k < 2 {
            "One factor: no correlations between factors.".to_owned()
        } else if max_abs < 1e-9 {
            "The coded factor columns are uncorrelated: the design is orthogonal for the main effects.".to_owned()
        } else {
            format!(
                "The largest correlation between two coded factor columns is {}; correlated factors inflate the \
                 variance of their estimated effects.",
                format_number(max_abs, 3)
            )
        };
        let mut out = vec![
            section(SectionKind::DeclaredRoles, "Factors and Responses", SectionContent::Table(roles)),
            section(SectionKind::DeclaredRuns, "Runs", SectionContent::Table(runs)),
        ];
        out.extend(self.plot_sections());
        out.extend([
            section(SectionKind::DeclaredCorrelations, "Correlations of Coded Factors", SectionContent::Table(corr)),
            section(SectionKind::DeclaredNote, "Orthogonality", SectionContent::Note(note)),
            section(SectionKind::DeclaredModels, "Models the Runs Support", SectionContent::Table(models)),
        ]);
        out
    }

    /// The whole plots (and subplots) and the analysis they call for.
    fn plot_sections(&self) -> Vec<ReportSection> {
        let Some(p) = &self.plots else { return Vec::new() };
        let mut t = TextTable::new(["Stratum", "Column", "Plots", "Runs per plot", "Factors held fixed within"]);
        let sizes =
            |(_, lo, hi): (usize, usize, usize)| if lo == hi { lo.to_string() } else { format!("{lo} to {hi}") };
        let list = |v: &[String]| if v.is_empty() { "—".to_owned() } else { v.join(", ") };
        t.push([
            "Whole plots".to_owned(),
            p.whole_plot.clone(),
            p.whole.0.to_string(),
            sizes(p.whole),
            list(&p.fixed_whole),
        ]);
        if let (Some(name), Some(sub)) = (&p.subplot, p.sub) {
            t.push(["Subplots".to_owned(), name.clone(), sub.0.to_string(), sizes(sub), list(&p.fixed_sub)]);
        }
        let random = match &p.subplot {
            Some(sp) => format!("random intercepts for {} and for {sp} within {}", p.whole_plot, p.whole_plot),
            None => format!("a random intercept for {}", p.whole_plot),
        };
        let mut note = format!(
            "Runs in the same whole plot share its random effect, so analyse the responses with a mixed model: the \
             design's fixed effects with {random}, fitted by REML (Model > Mixed model, filled in for this design). \
             Effects of factors held fixed within whole plots are then tested against whole-plot error."
        );
        if let Some((model, df)) = &p.model {
            note.push_str(&format!(" Model: {model}; {df} degrees of freedom for whole-plot error."));
            if *df <= 0 {
                note.push_str(" With none, whole-plot effects cannot be tested.");
            }
        }
        vec![
            section(SectionKind::DeclaredPlots, "Restricted Randomization", SectionContent::Table(t)),
            section(SectionKind::DeclaredPlots, "Analysis of a Split-Plot Design", SectionContent::Note(note)),
        ]
    }
}

/// The mixed model that analyses a declared split-plot design: the first
/// response on the factors with the recorded model's terms (main effects
/// when none is recorded or it does not parse), a random intercept for each
/// whole plot and, for a split-split-plot design, for each subplot within its
/// whole plot; normal family, REML. `None` without a whole-plot column or a
/// response.
#[must_use]
pub fn split_plot_analysis(table: &DataTable, request: &DesignDeclarationRequest) -> Option<crate::SavedReport> {
    let whole_plot = request.whole_plot?;
    let response = *request.responses.first()?;
    let predictors: Vec<usize> = request.factors.iter().map(|f| f.column).collect();
    let names: Vec<String> =
        predictors.iter().map(|&c| table.column(c).map_or(String::new(), |c| c.name.clone())).collect();
    let effects = ModelFormula::parse(&request.model, &names)
        .map(|f| {
            f.terms()
                .iter()
                .skip(1)
                .map(|t| t.powers.iter().enumerate().filter(|(_, p)| **p > 0).map(|(j, &p)| (j, p)).collect())
                .collect()
        })
        .unwrap_or_default();
    let mut random = vec![crate::RandomEffectSpec::intercept(whole_plot)];
    if let Some(sp) = request.subplot {
        random.push(crate::RandomEffectSpec { within: vec![whole_plot], ..crate::RandomEffectSpec::intercept(sp) });
    }
    Some(crate::SavedReport::Mixed {
        request: crate::MixedRequest {
            response,
            predictors,
            effects,
            random,
            family: crate::MixedFamily::Normal,
            trials: None,
            offset: None,
            reml: true,
            quadrature_points: 1,
            slices: None,
            slopes: None,
            frequency: None,
            weight: None,
            robust: false,
            residual_groups: None,
            df_method: stat_mixed::DfMethod::Satterthwaite,
            order: Vec::new(),
        },
        profilers: Default::default(),
    })
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let sab: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let saa: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let sbb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
    if saa > 0.0 && sbb > 0.0 { sab / (saa * sbb).sqrt() } else { f64::NAN }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doe::{DesignKind, DesignRequest, create_design};
    use stat_data::Column;
    use stat_doe::Factor;

    /// A 2² factorial with three centre runs, created by the design tools and
    /// then declared: hand-checkable properties (orthogonal; 2 centre-point
    /// pure-error df; main effects + interaction estimable with D-efficiency
    /// 100 · (det XᵀX)^{1/4}/7 = 100 · (4·4·4·7)^{1/4}/7 for X with columns
    /// 1, A, B, AB over 4 corners and 3 centres: XᵀX = diag(7, 4, 4, 4)).
    #[test]
    fn declared_factorial_with_centre_points() {
        let mut req = DesignRequest::new(DesignKind::TwoLevelFactorial, 2);
        req.factors = vec![Factor::new("Temp", 150.0, 190.0), Factor::new("Time", 10.0, 30.0)];
        req.center_points = Some(3);
        req.randomize = Some(2);
        let (t, _) = create_design(&req).unwrap();
        let r =
            DesignDeclarationRequest::new(&[t.find("Temp").unwrap(), t.find("Time").unwrap()], &[t.find("Y").unwrap()]);
        let d = DeclaredDesign::run(&t, &RowStateManager::new(t.n_rows()), &r).unwrap();
        assert_eq!(d.coded_runs().len(), 7);
        let m = d.model_support();
        assert_eq!(m.len(), 3);
        assert!(m[0].estimable && m[1].estimable && !m[2].estimable, "no quadratic from 2 levels + center");
        assert_eq!((m[1].terms, m[1].error_df), (4, 3));
        assert!((m[1].d_efficiency - 100.0 * (4.0f64 * 4.0 * 4.0 * 7.0).powf(0.25) / 7.0).abs() < 1e-9);
        let s = d.sections();
        let SectionContent::Table(runs) = &s[1].content else { panic!() };
        assert_eq!(runs.rows[3][1], "2", "two pure-error df from three center runs");
        assert_eq!(runs.rows[4][1], "3");
        let SectionContent::Note(note) = &s[3].content else { panic!() };
        assert!(note.contains("orthogonal"));
        let SectionContent::Table(roles) = &s[0].content else { panic!() };
        assert_eq!(roles.rows[0][2], "150");
        assert_eq!(roles.rows[2][0], "Response");
        let back = DesignDeclarationRequest::from_json(&r.to_json(&t), &t).unwrap();
        assert_eq!(back, r);
    }

    /// A two-level text factor is coded −1/+1 in sorted order; one with three
    /// levels is refused.
    #[test]
    fn two_level_categorical_factor() {
        let text = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let t = DataTable::new(
            "d",
            vec![
                Column::numeric("A", vec![1.0, 2.0, 1.0, 2.0]),
                Column::text("Resin", text(&["B", "B", "A", "A"])),
                Column::text("Three", text(&["x", "y", "z", "x"])),
            ],
        )
        .unwrap();
        let states = RowStateManager::new(4);
        let d = DeclaredDesign::run(&t, &states, &DesignDeclarationRequest::new(&[0, 1], &[])).unwrap();
        let coded: Vec<f64> = d.coded_runs().iter().map(|r| r[1]).collect();
        assert_eq!(coded, [1.0, 1.0, -1.0, -1.0]);
        let SectionContent::Table(roles) = &d.sections()[0].content else { panic!() };
        assert_eq!((roles.rows[1][2].as_str(), roles.rows[1][3].as_str()), ("A", "B"));
        let SectionContent::Note(note) = &d.sections()[3].content else { panic!() };
        assert!(note.contains("orthogonal"), "the 2² is orthogonal: {note}");
        let e = DeclaredDesign::run(&t, &states, &DesignDeclarationRequest::new(&[2], &[])).unwrap_err();
        assert!(e.to_string().contains("3 levels"), "{e}");
    }

    /// A declared split-split-plot design: the plots, the factors fixed
    /// within them, the whole-plot error df of the recorded model, the
    /// project-file round trip and the mixed model it calls for.
    #[test]
    fn declared_split_split_plot() {
        let wp = vec![1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0];
        let sp = vec![1.0, 1.0, 2.0, 2.0, 1.0, 1.0, 2.0, 2.0];
        let t = DataTable::new(
            "d",
            vec![
                Column::numeric("WP", wp),
                Column::numeric("SP", sp),
                Column::numeric("A", vec![-1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0]),
                Column::numeric("B", vec![-1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, -1.0]),
                Column::numeric("C", vec![-1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0]),
                Column::numeric("Y", vec![1.0; 8]),
            ],
        )
        .unwrap();
        let mut r = DesignDeclarationRequest::new(&[2, 3, 4], &[5]);
        r.whole_plot = Some(0);
        r.subplot = Some(1);
        r.model = "A + B + C + A*B".into();
        let d = DeclaredDesign::run(&t, &RowStateManager::new(8), &r).unwrap();
        let s = d.sections();
        let plots = s.iter().find(|s| s.title == "Restricted Randomization").expect("plot section");
        let SectionContent::Table(table) = &plots.content else { panic!() };
        assert_eq!(table.rows[0], ["Whole plots", "WP", "2", "4", "A"]);
        assert_eq!(table.rows[1], ["Subplots", "SP", "4", "2", "B"], "subplot labels restart in each whole plot");
        let note = s.iter().find(|s| s.title == "Analysis of a Split-Plot Design").unwrap();
        let SectionContent::Note(note) = &note.content else { panic!() };
        assert!(note.contains("0 degrees of freedom for whole-plot error"), "{note}");
        assert_eq!(DesignDeclarationRequest::from_json(&r.to_json(&t), &t).unwrap(), r);
        let Some(crate::SavedReport::Mixed { request, .. }) = split_plot_analysis(&t, &r) else { panic!() };
        assert_eq!(request.response, 5);
        assert_eq!(request.effects, vec![vec![(0, 1)], vec![(1, 1)], vec![(2, 1)], vec![(0, 1), (1, 1)]]);
        assert_eq!(request.random.len(), 2);
        assert_eq!((request.random[1].group, request.random[1].within.clone()), (1, vec![0]));
        // Without a whole-plot column there is nothing to analyse this way.
        assert!(split_plot_analysis(&t, &DesignDeclarationRequest::new(&[2], &[5])).is_none());
        r.whole_plot = None;
        assert!(DeclaredDesign::run(&t, &RowStateManager::new(8), &r).is_err(), "subplots need whole plots");
    }

    #[test]
    fn explicit_ranges_and_errors() {
        let t = DataTable::new(
            "d",
            vec![
                Column::numeric("A", vec![1.0, 2.0, 3.0, 2.0]),
                Column::numeric("B", vec![5.0, 5.0, 6.0, 7.0]),
                Column::text("C", vec!["a".into(); 4]),
            ],
        )
        .unwrap();
        let states = RowStateManager::new(4);
        let mut r = DesignDeclarationRequest::new(&[0, 1], &[]);
        r.factors[0].low = Some(0.0);
        r.factors[0].high = Some(2.0);
        let d = DeclaredDesign::run(&t, &states, &r).unwrap();
        assert_eq!(d.coded_runs()[0], vec![0.0, -1.0]);
        assert_eq!(d.outside[0], 1, "A = 3 is above the declared high setting");
        let SectionContent::Note(note) = &d.sections()[3].content else { panic!() };
        assert!(note.contains("largest correlation"));
        assert!(
            DeclaredDesign::run(&t, &states, &DesignDeclarationRequest::new(&[2], &[])).is_err(),
            "a text factor with one level"
        );
        assert!(DeclaredDesign::run(&t, &states, &DesignDeclarationRequest::new(&[0, 0], &[])).is_err());
        assert!(DeclaredDesign::run(&t, &states, &DesignDeclarationRequest::new(&[], &[1])).is_err());
        let json = stat_data::parse_json(r#"{"analysis": "declared-design", "factors": ["A", "B"]}"#).unwrap();
        assert_eq!(
            DesignDeclarationRequest::from_json(&json, &t).unwrap(),
            DesignDeclarationRequest::new(&[0, 1], &[])
        );
    }
}
