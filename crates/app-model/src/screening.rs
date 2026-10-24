//! Screening many responses: the same model fitted to each of many response
//! columns against the same factors, with every effect's p-values adjusted
//! across the responses for the false discovery rate ([`crate::fdr`]).
//!
//! Each response uses the rows (not excluded) that have it and every factor.
//! The models, all with main effects only, are:
//!
//! * **linear model** (least squares): each effect's partial F test (the
//!   increase in the residual sum of squares when its columns are dropped);
//! * **Poisson regression** (log link): each effect's likelihood-ratio test
//!   (drop in deviance, χ² on the effect's columns);
//! * **negative binomial regression** (log link, shape `θ` estimated, as
//!   MASS `glm.nb`): each effect's likelihood-ratio test with `θ` refitted
//!   in the reduced model;
//! * **Kruskal–Wallis** (one categorical factor): the tie-corrected `H` test.
//!
//! The effect size plotted against `−log₁₀ p` is the coefficient of a
//! single-column effect (on the log scale for the count models), the range
//! of the level effects of a categorical factor with three or more levels,
//! and for Kruskal–Wallis `ε² = H/(n − 1)` (Tomczak & Tomczak 2014). For a
//! two-level factor, a table and forest plot show each response's difference
//! (second level − first) with its interval: the coefficient with its t
//! interval (linear), the rate ratio `exp(β)` with its Wald interval (count
//! models), or the Hodges–Lehmann shift with its rank-sum interval
//! (Kruskal–Wallis). With a subgroup column, every level is screened
//! separately and q-values are computed within each subgroup.

use crate::compare::{DIGITS, GroupComparisonRequest, GroupLayout, section};
use crate::fdr::{FdrMethod, IntervalRow, interval_plot};
use crate::format::{format_number, format_p_value};
use crate::model_data::{ModelData, PredictorData};
use crate::project::{SavedReport, invalid, member, obj, s};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_core::{Matrix, Result};
use stat_data::{DataTable, JsonValue, RowStateManager, natural_order};
use stat_discrete::{
    Family, GlmOptions, NegativeBinomialOptions, fit_glm, fit_negative_binomial, negative_binomial_lr_test,
};
use stat_inference::Alternative;
use stat_inference::nonparametric::rank_sum_test;
use stat_linear::{ModelDesign, OlsOptions, TermKind, effect_tests, fit_linear_model, kruskal_wallis};

/// Most volcano plots drawn per effect when screening by subgroup.
pub const MAX_SUBGROUP_PLOTS: usize = 8;

/// The model fitted to every response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScreeningModel {
    /// Least-squares linear model.
    Linear,
    /// Poisson regression (log link).
    Poisson,
    /// Negative binomial regression (log link).
    NegativeBinomial,
    /// Kruskal–Wallis rank test (one categorical factor).
    KruskalWallis,
}

impl ScreeningModel {
    /// Every model, in menu order.
    pub const ALL: [Self; 4] = [Self::Linear, Self::Poisson, Self::NegativeBinomial, Self::KruskalWallis];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear model",
            Self::Poisson => "Poisson regression (counts)",
            Self::NegativeBinomial => "Negative binomial regression (overdispersed counts)",
            Self::KruskalWallis => "Kruskal–Wallis (ranks, one factor)",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Poisson => "poisson",
            Self::NegativeBinomial => "negative-binomial",
            Self::KruskalWallis => "kruskal-wallis",
        }
    }

    fn counts(self) -> bool {
        matches!(self, Self::Poisson | Self::NegativeBinomial)
    }
}

/// Order of the rows of the results tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScreeningOrder {
    /// As the responses were chosen.
    Columns,
    /// Smallest p-value first.
    PValue,
    /// Largest absolute effect size first.
    EffectSize,
}

impl ScreeningOrder {
    /// Every order, in menu order.
    pub const ALL: [Self; 3] = [Self::Columns, Self::PValue, Self::EffectSize];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Columns => "Column order",
            Self::PValue => "Smallest p-value first",
            Self::EffectSize => "Largest effect first",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Columns => "columns",
            Self::PValue => "p-value",
            Self::EffectSize => "effect",
        }
    }
}

/// Settings of a screening report.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreeningRequest {
    /// Response columns (numeric), one model each.
    pub responses: Vec<usize>,
    /// Factors (predictor columns) of every model.
    pub factors: Vec<usize>,
    /// The model.
    pub model: ScreeningModel,
    /// Screen every level of this column separately.
    pub subgroup: Option<usize>,
    /// False-discovery-rate adjustment across the responses.
    pub fdr: FdrMethod,
    /// Rate at which q-values count as discoveries.
    pub alpha: f64,
    /// Order of the results tables.
    pub order: ScreeningOrder,
}

impl ScreeningRequest {
    /// Defaults: linear model, Benjamini–Hochberg at `α = 0.05`, smallest
    /// p-value first, no subgroups.
    #[must_use]
    pub fn new(responses: Vec<usize>, factors: Vec<usize>) -> Self {
        Self {
            responses,
            factors,
            model: ScreeningModel::Linear,
            subgroup: None,
            fdr: FdrMethod::BenjaminiHochberg,
            alpha: 0.05,
            order: ScreeningOrder::PValue,
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn all_columns(&self) -> Vec<usize> {
        self.responses.iter().chain(&self.factors).copied().chain(self.subgroup).collect()
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self {
            responses: self.responses.iter().map(|&c| f(c)).collect(),
            factors: self.factors.iter().map(|&c| f(c)).collect(),
            subgroup: self.subgroup.map(f),
            ..self.clone()
        }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        obj([
            ("analysis", s("screening")),
            ("responses", JsonValue::Array(self.responses.iter().map(|&c| col(c)).collect())),
            ("factors", JsonValue::Array(self.factors.iter().map(|&c| col(c)).collect())),
            ("model", s(self.model.key())),
            ("subgroup", self.subgroup.map_or(JsonValue::Null, col)),
            ("fdr", s(self.fdr.key())),
            ("alpha", JsonValue::number(self.alpha)),
            ("order", s(self.order.key())),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns, unknown names or an `α` outside `(0, 1)`.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find =
            |name: &str| table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")));
        let list = |key: &str| -> Result<Vec<usize>> {
            member(v, key)?
                .as_array()
                .ok_or_else(|| invalid(format!("{key}: expected a list of column names")))?
                .iter()
                .map(|c| find(c.as_str().ok_or_else(|| invalid(format!("{key}: expected column names")))?))
                .collect()
        };
        let responses = list("responses")?;
        let factors = list("factors")?;
        let subgroup = match v.get("subgroup").and_then(JsonValue::as_str) {
            Some(name) => Some(find(name)?),
            None => None,
        };
        let name = |key: &str, default: &str| v.get(key).and_then(JsonValue::as_str).unwrap_or(default).to_owned();
        let model_name = name("model", "linear");
        let model = ScreeningModel::ALL
            .into_iter()
            .find(|m| m.key() == model_name)
            .ok_or_else(|| invalid(format!("screening: unknown model {model_name:?}")))?;
        let fdr_name = name("fdr", "bh");
        let fdr =
            FdrMethod::from_key(&fdr_name).ok_or_else(|| invalid(format!("screening: unknown method {fdr_name:?}")))?;
        let order_name = name("order", "p-value");
        let order = ScreeningOrder::ALL
            .into_iter()
            .find(|o| o.key() == order_name)
            .ok_or_else(|| invalid(format!("screening: unknown order {order_name:?}")))?;
        let alpha = v.get("alpha").and_then(JsonValue::as_f64).unwrap_or(0.05);
        if !(alpha > 0.0 && alpha < 1.0) {
            return Err(invalid("screening: alpha must lie between 0 and 1"));
        }
        Ok(Self { responses, factors, model, subgroup, fdr, alpha, order })
    }
}

/// A two-level factor's difference for one response.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelDifference {
    /// `"second − first"` (or `"second / first"` for a rate ratio).
    pub label: String,
    /// Difference, rate ratio or shift.
    pub estimate: f64,
    /// 95% interval.
    pub interval: (f64, f64),
}

/// The test of one effect for one response.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectResult {
    /// Test statistic (F, likelihood-ratio χ² or H).
    pub statistic: f64,
    /// Numerator degrees of freedom.
    pub df: usize,
    /// p-value.
    pub p_value: f64,
    /// FDR-adjusted p-value (q-value) across the responses.
    pub q_value: f64,
    /// Effect size (see the module documentation).
    pub size: f64,
    /// For a two-level factor, the difference with its interval.
    pub difference: Option<LevelDifference>,
}

/// One response's screen.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseScreen {
    /// Response column.
    pub column: usize,
    /// Response name.
    pub name: String,
    /// Rows used.
    pub n: usize,
    /// One result per factor; `None` where the fit failed.
    pub effects: Vec<Option<EffectResult>>,
    /// Negative binomial shape `θ`.
    pub theta: Option<f64>,
    /// Coefficients `(name, estimate, std error, p)` (intercept first).
    pub coefficients: Vec<(String, f64, f64, f64)>,
    /// Why the model could not be fitted.
    pub error: Option<String>,
}

/// One subgroup's screen of every response.
#[derive(Debug, Clone, PartialEq)]
pub struct SubgroupScreen {
    /// Subgroup level; empty without subgroups.
    pub level: String,
    /// One per response, in request order.
    pub responses: Vec<ResponseScreen>,
    /// Storey's `π̂₀` per effect.
    pub pi0: Vec<Option<f64>>,
}

/// A computed screening report.
#[derive(Debug, Clone)]
pub struct ScreeningAnalysis {
    request: ScreeningRequest,
    factor_names: Vec<String>,
    subgroup_name: Option<String>,
    screens: Vec<SubgroupScreen>,
}

impl ScreeningAnalysis {
    /// Fit every response (in every subgroup) and adjust the p-values.
    ///
    /// # Errors
    /// No responses or factors, a response that is also a factor, a
    /// non-numeric response, or Kruskal–Wallis without exactly one
    /// categorical factor.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ScreeningRequest) -> Result<Self> {
        if request.responses.is_empty() {
            return Err(invalid("choose at least one response"));
        }
        if request.factors.is_empty() {
            return Err(invalid("choose at least one factor"));
        }
        if request.responses.iter().any(|r| request.factors.contains(r) || Some(*r) == request.subgroup) {
            return Err(invalid("a response cannot also be a factor or the subgroup column"));
        }
        if request.subgroup.is_some_and(|g| request.factors.contains(&g)) {
            return Err(invalid("the subgroup column cannot also be a factor"));
        }
        if request.model == ScreeningModel::KruskalWallis && request.factors.len() != 1 {
            return Err(invalid("Kruskal–Wallis takes exactly one categorical factor"));
        }
        for &c in &request.responses {
            let col = table.column(c).ok_or_else(|| invalid("no such column"))?;
            if col.as_numeric().is_none() {
                return Err(invalid(format!("the response {} must be numeric", col.name)));
            }
        }
        let factor_names = request
            .factors
            .iter()
            .map(|&c| table.column(c).map(|c| c.name.clone()).ok_or_else(|| invalid("no such column")))
            .collect::<Result<Vec<_>>>()?;
        let mut groups: Vec<(String, RowStateManager)> = Vec::new();
        let subgroup_name = match request.subgroup {
            None => {
                groups.push((String::new(), states.clone()));
                None
            }
            Some(g) => {
                let col = table.column(g).ok_or_else(|| invalid("no such column"))?;
                let text: Vec<String> = (0..table.n_rows()).map(|r| col.level_text(r)).collect();
                let mut levels: Vec<String> = (0..table.n_rows())
                    .filter(|&r| !states.is_excluded(r) && !text[r].trim().is_empty())
                    .map(|r| text[r].clone())
                    .collect();
                levels.sort_by(|a, b| natural_order(a, b));
                levels.dedup();
                if levels.is_empty() {
                    return Err(invalid(format!("the subgroup column {} has no values", col.name)));
                }
                for level in levels {
                    let mut rows = states.clone();
                    rows.set_excluded((0..table.n_rows()).filter(|&r| text[r] != level), true);
                    groups.push((level, rows));
                }
                Some(col.name.clone())
            }
        };
        let mut screens = Vec::with_capacity(groups.len());
        for (level, rows) in groups {
            let mut responses: Vec<ResponseScreen> =
                request.responses.iter().map(|&c| screen_response(table, &rows, request, c)).collect();
            let mut pi0 = Vec::with_capacity(request.factors.len());
            for e in 0..request.factors.len() {
                let p: Vec<f64> =
                    responses.iter().map(|r| r.effects[e].as_ref().map_or(f64::NAN, |x| x.p_value)).collect();
                let (q, pi) = request.fdr.adjust(&p)?;
                for (r, q) in responses.iter_mut().zip(q) {
                    if let Some(x) = r.effects[e].as_mut() {
                        x.q_value = q;
                    }
                }
                pi0.push(pi);
            }
            screens.push(SubgroupScreen { level, responses, pi0 });
        }
        Ok(Self { request: request.clone(), factor_names, subgroup_name, screens })
    }

    /// The screens, one per subgroup (one without subgroups).
    #[must_use]
    pub fn screens(&self) -> &[SubgroupScreen] {
        &self.screens
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ScreeningRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let by = self.subgroup_name.as_ref().map_or_else(String::new, |g| format!(", by {g}"));
        format!(
            "Screening: {} responses ~ {} ({}{by})",
            self.request.responses.len(),
            self.factor_names.join(" + "),
            self.request.model.label()
        )
    }

    /// Responses `(column, name, smallest q-value over effects and
    /// subgroups)`, most significant first.
    #[must_use]
    pub fn ranked_responses(&self) -> Vec<(usize, String, f64)> {
        let mut out: Vec<(usize, String, f64)> = self
            .request
            .responses
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let q = self
                    .screens
                    .iter()
                    .flat_map(|s| s.responses[i].effects.iter().flatten().map(|e| e.q_value))
                    .filter(|q| !q.is_nan())
                    .fold(f64::NAN, f64::min);
                (c, self.screens[0].responses[i].name.clone(), q)
            })
            .collect();
        out.sort_by(|a, b| nan_last(a.2, b.2));
        out
    }

    /// The full report of one response: a linear model, a Poisson GLM, a
    /// comparison of groups (Kruskal–Wallis) or, for negative binomial
    /// regression, this screen of that response alone (whose report then
    /// lists every coefficient).
    #[must_use]
    pub fn response_report(&self, column: usize) -> SavedReport {
        let r = &self.request;
        match r.model {
            ScreeningModel::Linear => SavedReport::Linear {
                spec: crate::LinearModelSpec {
                    response: column,
                    predictors: r.factors.clone(),
                    effects: Vec::new(),
                    splines: Vec::new(),
                },
                options: crate::ReportOptions::default(),
            },
            ScreeningModel::Poisson => SavedReport::Glm {
                request: crate::GlmRequest {
                    response: column,
                    predictors: r.factors.clone(),
                    family: Family::Poisson,
                    link: None,
                    event: None,
                    effects: Vec::new(),
                    splines: Vec::new(),
                    slices: None,
                    slopes: None,
                },
                profilers: crate::ProfilerOptions::default(),
            },
            ScreeningModel::KruskalWallis => SavedReport::Groups {
                request: GroupComparisonRequest::new(GroupLayout::Stacked { response: column, group: r.factors[0] }),
            },
            ScreeningModel::NegativeBinomial => {
                SavedReport::Screening { request: ScreeningRequest { responses: vec![column], ..r.clone() } }
            }
        }
    }

    fn significant(&self, e: &EffectResult) -> bool {
        e.q_value <= self.request.alpha
    }

    fn size_name(&self) -> &'static str {
        match self.request.model {
            ScreeningModel::Linear => "Effect size",
            ScreeningModel::Poisson | ScreeningModel::NegativeBinomial => "Effect size (log scale)",
            ScreeningModel::KruskalWallis => "ε² (rank effect size)",
        }
    }

    fn stat_name(&self) -> &'static str {
        match self.request.model {
            ScreeningModel::Linear => "F",
            ScreeningModel::Poisson | ScreeningModel::NegativeBinomial => "LR χ²",
            ScreeningModel::KruskalWallis => "H",
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        for e in 0..self.factor_names.len() {
            out.push(self.results_table(e));
            out.extend(self.volcanoes(e));
            out.extend(self.differences(e));
        }
        if self.request.responses.len() == 1 {
            out.extend(self.coefficients());
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let r = &self.request;
        let mut t = TextTable::new(["Setting", "Value"]);
        t.push(["Model".to_owned(), format!("{} with main effects", r.model.label())]);
        t.push(["Factors".to_owned(), self.factor_names.join(", ")]);
        if let Some(g) = &self.subgroup_name {
            t.push(["Subgroups".to_owned(), format!("{} ({} levels, screened separately)", g, self.screens.len())]);
        }
        let fitted =
            self.screens.iter().map(|s| s.responses.iter().filter(|x| x.error.is_none()).count()).sum::<usize>();
        t.push([
            "Responses fitted".to_owned(),
            format!("{fitted} of {}", self.request.responses.len() * self.screens.len()),
        ]);
        t.push([
            "Adjustment".to_owned(),
            format!("{} across responses, per effect (false discovery rate)", r.fdr.label()),
        ]);
        for s in &self.screens {
            for (e, name) in self.factor_names.iter().enumerate() {
                let found =
                    s.responses.iter().filter(|x| x.effects[e].as_ref().is_some_and(|x| self.significant(x))).count();
                let within = if s.level.is_empty() { String::new() } else { format!(" in {}", s.level) };
                let pi0 = s.pi0[e].map_or_else(String::new, |p| format!("; π̂₀ = {}", format_number(p, 4)));
                t.push([format!("{name}{within}: q ≤ {}", format_number(r.alpha, 4)), format!("{found}{pi0}")]);
            }
        }
        section(SectionKind::ScreeningSummary, "Summary", SectionContent::Table(t))
    }

    fn ordered(&self, s: &SubgroupScreen, e: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..s.responses.len()).collect();
        let p = |i: usize| s.responses[i].effects[e].as_ref().map_or(f64::NAN, |x| x.p_value);
        let size = |i: usize| s.responses[i].effects[e].as_ref().map_or(f64::NAN, |x| -x.size.abs());
        match self.request.order {
            ScreeningOrder::Columns => {}
            ScreeningOrder::PValue => order.sort_by(|&a, &b| nan_last(p(a), p(b))),
            ScreeningOrder::EffectSize => order.sort_by(|&a, &b| nan_last(size(a), size(b))),
        }
        order
    }

    fn results_table(&self, e: usize) -> ReportSection {
        let r = &self.request;
        let mut head: Vec<String> = Vec::new();
        if let Some(g) = &self.subgroup_name {
            head.push(g.clone());
        }
        head.extend(
            ["Response", "N", self.size_name(), self.stat_name(), "DF", "p-value", "q-value", "−log₁₀ p", "Discovery"]
                .map(String::from),
        );
        let mut t = TextTable::new(head);
        for s in &self.screens {
            for i in self.ordered(s, e) {
                let x = &s.responses[i];
                let mut row = Vec::new();
                if self.subgroup_name.is_some() {
                    row.push(s.level.clone());
                }
                row.extend([x.name.clone(), x.n.to_string()]);
                match (&x.effects[e], &x.error) {
                    (Some(v), _) => row.extend([
                        format_number(v.size, DIGITS),
                        format_number(v.statistic, DIGITS),
                        v.df.to_string(),
                        format_p_value(v.p_value),
                        format_p_value(v.q_value),
                        format_number(-v.p_value.max(1e-300).log10(), 4),
                        if self.significant(v) { "yes".to_owned() } else { "no".to_owned() },
                    ]),
                    (None, err) => {
                        row.push(format!(
                            "not available: {}",
                            err.as_deref().unwrap_or("the effect could not be tested")
                        ));
                        row.extend((0..6).map(|_| String::new()));
                    }
                }
                t.push(row);
            }
        }
        section(
            SectionKind::ScreeningResults,
            format!("{} by Response ({}; {})", self.factor_names[e], r.order.label().to_lowercase(), self.size_note(e)),
            SectionContent::Table(t),
        )
    }

    fn size_note(&self, e: usize) -> String {
        let continuous = self
            .screens
            .iter()
            .flat_map(|s| &s.responses)
            .find_map(|x| x.effects[e].as_ref())
            .is_some_and(|x| x.df == 1 && x.difference.is_none());
        match self.request.model {
            ScreeningModel::KruskalWallis => "ε² = H/(n − 1)".to_owned(),
            _ if continuous => "effect size: slope".to_owned(),
            ScreeningModel::Linear => {
                "effect size: difference from the first level, or the range of level effects".to_owned()
            }
            _ => "effect size: log rate ratio, or the range of level effects".to_owned(),
        }
    }

    fn volcanoes(&self, e: usize) -> Vec<ReportSection> {
        let mut out = Vec::new();
        for s in self.screens.iter().take(MAX_SUBGROUP_PLOTS) {
            out.push(self.volcano(s, e));
        }
        if self.screens.len() > MAX_SUBGROUP_PLOTS {
            out.push(section(
                SectionKind::ScreeningVolcano,
                format!("Volcano plots of {}", self.factor_names[e]),
                SectionContent::Note(format!(
                    "Plots are drawn for the first {MAX_SUBGROUP_PLOTS} subgroups only; the table lists every subgroup."
                )),
            ));
        }
        out
    }

    fn volcano(&self, s: &SubgroupScreen, e: usize) -> ReportSection {
        let r = &self.request;
        let ok: Vec<(&str, &EffectResult)> =
            s.responses.iter().filter_map(|x| x.effects[e].as_ref().map(|v| (x.name.as_str(), v))).collect();
        let mut sig: Vec<(&str, &EffectResult)> = ok.iter().copied().filter(|(_, v)| self.significant(v)).collect();
        sig.sort_by(|a, b| a.1.p_value.total_cmp(&b.1.p_value));
        let named: Vec<&str> = sig.iter().take(25).map(|(n, _)| *n).collect();
        let y = |v: &EffectResult| -v.p_value.max(1e-300).log10();
        let items = ok
            .iter()
            .map(|(n, v)| {
                let label = if named.contains(n) { (*n).to_owned() } else { String::new() };
                (v.size, y(v), label, usize::from(self.significant(v)))
            })
            .collect();
        let x_lo = ok.iter().map(|(_, v)| v.size).fold(0.0_f64, f64::min);
        let x_hi = ok.iter().map(|(_, v)| v.size).fold(0.0_f64, f64::max);
        let y_max = ok.iter().map(|(_, v)| y(v)).fold(0.0_f64, f64::max);
        let threshold = -r.alpha.log10();
        let pad = 0.05 * (x_hi - x_lo).max(f64::MIN_POSITIVE);
        let within = if s.level.is_empty() { String::new() } else { format!(" in {}", s.level) };
        let spec = PlotSpec {
            x_label: self.size_name().to_owned(),
            y_label: "−log₁₀ p".to_owned(),
            line: Some(vec![[x_lo - pad, threshold], [x_hi + pad, threshold]]),
            line_name: Some(format!("p = {} before adjustment", format_number(r.alpha, 4))),
            segments: if r.model == ScreeningModel::KruskalWallis {
                Vec::new()
            } else {
                vec![vec![[0.0, 0.0], [0.0, y_max.max(threshold) * 1.05]]]
            },
            annotations: Some(Annotations {
                groups: vec![
                    "Not a discovery".to_owned(),
                    format!("q ≤ {} ({})", format_number(r.alpha, 4), r.fdr.label()),
                ],
                items,
                labels: Vec::new(),
            }),
            ..PlotSpec::default()
        };
        section(
            SectionKind::ScreeningVolcano,
            format!("Volcano Plot of {}{within} (one point per response; discoveries named)", self.factor_names[e]),
            SectionContent::Plot(Box::new(spec)),
        )
    }

    fn differences(&self, e: usize) -> Vec<ReportSection> {
        let label = self
            .screens
            .iter()
            .flat_map(|s| &s.responses)
            .find_map(|x| x.effects[e].as_ref().and_then(|v| v.difference.as_ref()).map(|d| d.label.clone()));
        let Some(label) = label else { return Vec::new() };
        let (what, reference) = match self.request.model {
            ScreeningModel::Linear => ("Difference", 0.0),
            ScreeningModel::Poisson | ScreeningModel::NegativeBinomial => ("Rate ratio", 1.0),
            ScreeningModel::KruskalWallis => ("Shift (Hodges–Lehmann)", 0.0),
        };
        let mut head: Vec<String> = Vec::new();
        if let Some(g) = &self.subgroup_name {
            head.push(g.clone());
        }
        head.extend(["Response", what, "Lower 95%", "Upper 95%", "q-value"].map(String::from));
        let mut t = TextTable::new(head);
        let mut rows = Vec::new();
        for s in &self.screens {
            for i in self.ordered(s, e) {
                let x = &s.responses[i];
                let Some(v) = &x.effects[e] else { continue };
                let Some(d) = &v.difference else { continue };
                let mut row = Vec::new();
                if self.subgroup_name.is_some() {
                    row.push(s.level.clone());
                }
                row.extend([
                    x.name.clone(),
                    format_number(d.estimate, DIGITS),
                    format_number(d.interval.0, DIGITS),
                    format_number(d.interval.1, DIGITS),
                    format_p_value(v.q_value),
                ]);
                t.push(row);
                let name = if s.level.is_empty() { x.name.clone() } else { format!("{} ({})", x.name, s.level) };
                rows.push(IntervalRow {
                    label: name,
                    estimate: d.estimate,
                    interval: d.interval,
                    group: usize::from(self.significant(v)),
                });
            }
        }
        let factor = &self.factor_names[e];
        let mut out = vec![section(
            SectionKind::ScreeningDifferences,
            format!("{what}s of {factor}, {label}, by Response (95% intervals)"),
            SectionContent::Table(t),
        )];
        if !rows.is_empty() {
            out.push(interval_plot(
                SectionKind::ScreeningForest,
                format!("Forest Plot of {factor} ({label}; discoveries marked)"),
                format!("{what}, {label}"),
                &rows,
                vec!["Not a discovery".to_owned(), format!("q ≤ {}", format_number(self.request.alpha, 4))],
                Some((
                    reference,
                    if reference == 1.0 { "No difference (ratio 1)" } else { "No difference" }.to_owned(),
                )),
            ));
        }
        out
    }

    fn coefficients(&self) -> Vec<ReportSection> {
        let mut out = Vec::new();
        for s in &self.screens {
            let x = &s.responses[0];
            if x.coefficients.is_empty() {
                continue;
            }
            let mut t = TextTable::new(["Term", "Estimate", "Std Error", "p-value"]);
            for (name, b, se, p) in &x.coefficients {
                t.push([name.clone(), format_number(*b, DIGITS), format_number(*se, DIGITS), format_p_value(*p)]);
            }
            if let Some(theta) = x.theta {
                t.push(["Shape θ".to_owned(), format_number(theta, DIGITS), String::new(), String::new()]);
            }
            let within = if s.level.is_empty() { String::new() } else { format!(" in {}", s.level) };
            let scale = if self.request.model.counts() { " (log scale)" } else { "" };
            out.push(section(
                SectionKind::ScreeningResults,
                format!("Coefficients of {}{within}{scale}", x.name),
                SectionContent::Table(t),
            ));
        }
        out
    }
}

/// Fit one response.
fn screen_response(
    table: &DataTable,
    rows: &RowStateManager,
    request: &ScreeningRequest,
    column: usize,
) -> ResponseScreen {
    let name = table.column(column).map_or_else(String::new, |c| c.name.clone());
    let k = request.factors.len();
    let mut out = ResponseScreen {
        column,
        name,
        n: 0,
        effects: vec![None; k],
        theta: None,
        coefficients: Vec::new(),
        error: None,
    };
    let fitted = ModelData::gather(table, rows, column, &request.factors, false).and_then(|data| {
        out.n = data.rows_used.len();
        match request.model {
            ScreeningModel::KruskalWallis => kruskal(&data).map(|e| (vec![Some(e)], None, Vec::new())),
            _ => model_effects(&data, request.model),
        }
    });
    match fitted {
        Ok((effects, theta, coefficients)) => {
            out.effects = effects;
            out.theta = theta;
            out.coefficients = coefficients;
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    out
}

type Fitted = (Vec<Option<EffectResult>>, Option<f64>, Vec<(String, f64, f64, f64)>);

/// The size of an effect whose coefficients are `b` (and the two-level
/// difference, from `interval`).
fn size_of(design: &ModelDesign, t: usize, b: &[f64]) -> f64 {
    match &design.terms()[t].kind {
        TermKind::Categorical { levels } if levels.len() > 2 => {
            let hi = b.iter().copied().fold(0.0_f64, f64::max);
            let lo = b.iter().copied().fold(0.0_f64, f64::min);
            hi - lo
        }
        _ => b.first().copied().unwrap_or(f64::NAN),
    }
}

fn two_levels(design: &ModelDesign, t: usize) -> Option<(String, String)> {
    match &design.terms()[t].kind {
        TermKind::Categorical { levels } if levels.len() == 2 => Some((levels[0].clone(), levels[1].clone())),
        _ => None,
    }
}

fn model_effects(data: &ModelData, model: ScreeningModel) -> Result<Fitted> {
    let (design, x, y) = data.design()?;
    let k = design.effects().len();
    let reduced_x = |columns: &std::ops::Range<usize>| -> Result<Matrix> {
        let keep: Vec<&[f64]> = (0..x.ncols()).filter(|j| !columns.contains(j)).map(|j| x.col(j)).collect();
        if keep.is_empty() { Ok(Matrix::zeros(x.nrows(), 0)) } else { Matrix::from_columns(&keep) }
    };
    let mut effects = Vec::with_capacity(k);
    match model {
        ScreeningModel::Linear => {
            let options = OlsOptions { predictor_names: design.column_names().to_vec(), ..OlsOptions::default() };
            let fit = fit_linear_model(&x, &y, &options)?;
            let tests = effect_tests(&design, &x, &y, &fit, &options)?;
            let params = fit.parameters();
            for (t, (term, test)) in design.effects().iter().zip(&tests).enumerate() {
                let b: Vec<f64> = term.columns.clone().map(|j| params[j + 1].estimate).collect();
                let difference = two_levels(&design, t).map(|(a, b2)| {
                    let p = &params[term.columns.start + 1];
                    LevelDifference {
                        label: format!("{b2} − {a}"),
                        estimate: p.estimate,
                        interval: (p.ci_lower, p.ci_upper),
                    }
                });
                effects.push(test.f_statistic.zip(test.p_value).map(|(f, p)| EffectResult {
                    statistic: f,
                    df: test.df,
                    p_value: p,
                    q_value: f64::NAN,
                    size: size_of(&design, t, &b),
                    difference,
                }));
            }
            let coefficients = params
                .iter()
                .map(|p| (p.name.clone(), p.estimate, p.std_error, p.p_value.unwrap_or(f64::NAN)))
                .collect();
            Ok((effects, None, coefficients))
        }
        ScreeningModel::Poisson => {
            let options = GlmOptions { names: design.column_names().to_vec(), ..GlmOptions::new(Family::Poisson) };
            let fit = fit_glm(&x, &y, &options)?;
            for (t, term) in design.effects().iter().enumerate() {
                let rx = reduced_x(&term.columns)?;
                let reduced = if rx.ncols() == 0 {
                    fit.null_deviance
                } else {
                    fit_glm(&rx, &y, &GlmOptions::new(Family::Poisson))?.deviance
                };
                let df = term.columns.len();
                let chi = (reduced - fit.deviance).max(0.0);
                let p = ChiSquared::new(df as f64)?.sf(chi)?;
                let b: Vec<f64> = term.columns.clone().map(|j| fit.coefficients[j + 1].estimate).collect();
                let difference = two_levels(&design, t).map(|(a, b2)| {
                    let c = &fit.coefficients[term.columns.start + 1];
                    LevelDifference {
                        label: format!("{b2} / {a}"),
                        estimate: c.estimate.exp(),
                        interval: (c.confidence_interval.0.exp(), c.confidence_interval.1.exp()),
                    }
                });
                effects.push(Some(EffectResult {
                    statistic: chi,
                    df,
                    p_value: p,
                    q_value: f64::NAN,
                    size: size_of(&design, t, &b),
                    difference,
                }));
            }
            let coefficients =
                fit.coefficients.iter().map(|c| (c.name.clone(), c.estimate, c.std_error, c.p_value)).collect();
            Ok((effects, None, coefficients))
        }
        ScreeningModel::NegativeBinomial => {
            let options = NegativeBinomialOptions { names: design.column_names().to_vec(), ..Default::default() };
            let fit = fit_negative_binomial(&x, &y, &options)?;
            for (t, term) in design.effects().iter().enumerate() {
                let rx = reduced_x(&term.columns)?;
                let test = fit_negative_binomial(&rx, &y, &NegativeBinomialOptions::default())
                    .and_then(|reduced| negative_binomial_lr_test(&fit, &reduced));
                let b: Vec<f64> = term.columns.clone().map(|j| fit.coefficients[j + 1].estimate).collect();
                let difference = two_levels(&design, t).map(|(a, b2)| {
                    let c = &fit.coefficients[term.columns.start + 1];
                    LevelDifference {
                        label: format!("{b2} / {a}"),
                        estimate: c.estimate.exp(),
                        interval: (c.interval.0.exp(), c.interval.1.exp()),
                    }
                });
                effects.push(test.ok().map(|(chi, df, p)| EffectResult {
                    statistic: chi,
                    df,
                    p_value: p,
                    q_value: f64::NAN,
                    size: size_of(&design, t, &b),
                    difference,
                }));
            }
            let coefficients =
                fit.coefficients.iter().map(|c| (c.name.clone(), c.estimate, c.std_error, c.p_value)).collect();
            Ok((effects, Some(fit.theta), coefficients))
        }
        ScreeningModel::KruskalWallis => unreachable!("handled by kruskal"),
    }
}

fn kruskal(data: &ModelData) -> Result<EffectResult> {
    let PredictorData::Categorical(labels) = &data.predictors[0] else {
        return Err(invalid(format!(
            "Kruskal–Wallis needs a categorical factor; {} is continuous",
            data.predictor_names[0]
        )));
    };
    let mut levels: Vec<String> = data.rows_used.iter().map(|&r| labels[r].clone()).collect();
    levels.sort_by(|a, b| natural_order(a, b));
    levels.dedup();
    if levels.len() < 2 {
        return Err(invalid("the factor has fewer than two levels in these rows"));
    }
    let groups: Vec<Vec<f64>> = levels
        .iter()
        .map(|l| data.rows_used.iter().filter(|&&r| &labels[r] == l).map(|&r| data.y_all[r]).collect())
        .collect();
    let test = kruskal_wallis(&groups)?;
    let n = data.rows_used.len() as f64;
    let difference = if levels.len() == 2 {
        let shift = rank_sum_test(&groups[1], &groups[0], 0.0, 0.95, Alternative::TwoSided)?;
        Some(LevelDifference {
            label: format!("{} − {}", levels[1], levels[0]),
            estimate: shift.estimate,
            interval: shift.confidence_interval,
        })
    } else {
        None
    };
    Ok(EffectResult {
        statistic: test.statistic,
        df: levels.len() - 1,
        p_value: test.p_value,
        q_value: f64::NAN,
        size: test.statistic / (n - 1.0),
        difference,
    })
}

/// Ascending order with NaN last.
fn nan_last(a: f64, b: f64) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => a.total_cmp(&b),
    }
}
