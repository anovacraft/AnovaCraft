//! Treatment effects from observational data bound to the row state
//! ([`stat_discrete::treatment_effects`]): a propensity-score model, the
//! overlap of the scores, covariate balance before and after adjustment,
//! the effect by matching, inverse-probability weighting or augmented
//! (doubly robust) weighting, and the effect within subgroups with a
//! forest plot and a test of equal effects.
//!
//! The treatment column must have exactly two values among the rows used;
//! the treated value is the one named in the request, or else the second in
//! natural order. Continuous covariates enter the propensity and outcome
//! models as they are; a categorical covariate enters as indicators of each
//! level but the first (natural order), and the balance table lists every
//! level. A covariate constant among the rows of an analysis (for example
//! the subgroup column within its own subgroups) is left out of it. Rows
//! excluded, or missing the treatment, the outcome or a covariate, are left
//! out; a row missing a subgroup value is in no subgroup.

use crate::compare::section;
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use crate::report_columns::ReportColumn;
use stat_core::{Matrix, Result};
use stat_data::{DataTable, JsonValue, MeasurementLevel, RowStateManager, natural_order};
use stat_discrete::treatment_effects::{
    BalanceRow, EffectMethod, EffectScale, Estimand, IntervalMethod, MatchingOptions, OutcomeKind, TreatmentEffectFit,
    TreatmentEffectOptions, covariate_balance, estimate_treatment_effect, heterogeneity_test,
};

const DIGITS: usize = 5;
/// Standardized mean differences below this are conventionally balanced
/// (Austin 2009).
pub const BALANCE_THRESHOLD: f64 = 0.1;

/// What to analyse.
#[derive(Debug, Clone, PartialEq)]
pub struct TreatmentEffectsRequest {
    /// Two-valued treatment column.
    pub treatment: usize,
    /// The treated value; `None` for the second in natural order.
    pub treated_level: Option<String>,
    /// Outcome column (numeric).
    pub outcome: usize,
    /// Kind of outcome.
    pub outcome_kind: OutcomeKind,
    /// Covariates (confounders measured before treatment).
    pub covariates: Vec<usize>,
    /// Matching, weighting or doubly robust (with matching settings).
    pub method: EffectMethod,
    /// ATE or ATT.
    pub estimand: Estimand,
    /// Stabilize the saved inverse-probability weights.
    pub stabilized: bool,
    /// Trimming threshold for the propensity score (0 for none).
    pub trim: f64,
    /// Sandwich or bootstrap intervals.
    pub intervals: IntervalMethod,
    /// Confidence level.
    pub level: f64,
    /// Columns whose levels give subgroup estimates.
    pub subgroups: Vec<usize>,
}

impl TreatmentEffectsRequest {
    /// Inverse-probability weighting for the ATE with the engine's defaults.
    #[must_use]
    pub fn new(treatment: usize, outcome: usize, outcome_kind: OutcomeKind, covariates: Vec<usize>) -> Self {
        let d = TreatmentEffectOptions::new(outcome_kind);
        Self {
            treatment,
            treated_level: None,
            outcome,
            outcome_kind,
            covariates,
            method: d.method,
            estimand: d.estimand,
            stabilized: d.stabilized,
            trim: d.trim,
            intervals: d.intervals,
            level: d.confidence_level,
            subgroups: Vec::new(),
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.treatment, self.outcome];
        c.extend(&self.covariates);
        c.extend(&self.subgroups);
        c
    }

    /// Pass every column index through `f`.
    #[must_use]
    pub fn map_columns(&self, f: &impl Fn(usize) -> usize) -> Self {
        Self {
            treatment: f(self.treatment),
            outcome: f(self.outcome),
            covariates: self.covariates.iter().map(|&c| f(c)).collect(),
            subgroups: self.subgroups.iter().map(|&c| f(c)).collect(),
            ..self.clone()
        }
    }

    fn options(&self) -> TreatmentEffectOptions {
        TreatmentEffectOptions {
            method: self.method,
            estimand: self.estimand,
            outcome: self.outcome_kind,
            stabilized: self.stabilized,
            trim: self.trim,
            intervals: self.intervals,
            confidence_level: self.level,
        }
    }

    /// The project-file (and script) form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let cols = |v: &[usize]| JsonValue::Array(v.iter().map(|&c| col(c)).collect());
        let num = JsonValue::number;
        let m = match self.method {
            EffectMethod::Matching(m) => m,
            _ => MatchingOptions::default(),
        };
        let (intervals, replicates, seed) = match self.intervals {
            IntervalMethod::Sandwich => ("sandwich", 500, 1),
            IntervalMethod::Bootstrap { replicates, seed } => ("bootstrap", replicates, seed),
        };
        obj([
            ("analysis", s("treatment-effects")),
            ("treatment", col(self.treatment)),
            ("treated_level", self.treated_level.as_deref().map_or(JsonValue::Null, s)),
            ("outcome", col(self.outcome)),
            ("outcome_kind", s(self.outcome_kind.key())),
            ("covariates", cols(&self.covariates)),
            ("method", s(self.method.key())),
            ("estimand", s(self.estimand.key())),
            ("ratio", num(m.ratio as f64)),
            ("replace", JsonValue::Bool(m.replace)),
            ("caliper", m.caliper.map_or(JsonValue::Null, num)),
            ("stabilized", JsonValue::Bool(self.stabilized)),
            ("trim", num(self.trim)),
            ("intervals", s(intervals)),
            ("replicates", num(replicates as f64)),
            ("seed", num(seed as f64)),
            ("level", num(self.level)),
            ("subgroups", cols(&self.subgroups)),
        ])
    }

    /// Read the project-file form; members other than the columns are
    /// optional and keep their defaults.
    ///
    /// # Errors
    /// Missing columns and unknown names.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find =
            |name: &str| table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")));
        let col = |key: &str| {
            find(member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?)
        };
        let cols = |key: &str| -> Result<Vec<usize>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(Vec::new()),
                Some(JsonValue::Array(items)) => items
                    .iter()
                    .map(|c| find(c.as_str().ok_or_else(|| invalid(format!("{key}: expected column names")))?))
                    .collect(),
                Some(_) => Err(invalid(format!("{key}: expected a list of column names"))),
            }
        };
        let text = |key: &str| v.get(key).and_then(JsonValue::as_str);
        let num = |key: &str| v.get(key).and_then(JsonValue::as_f64);
        let flag = |key: &str, d: bool| v.get(key).and_then(JsonValue::as_bool).unwrap_or(d);
        let kind = match text("outcome_kind") {
            None => OutcomeKind::Continuous,
            Some(k) => OutcomeKind::from_key(k)
                .ok_or_else(|| invalid(format!("treatment effects: unknown outcome kind {k:?}")))?,
        };
        let mut r = Self::new(col("treatment")?, col("outcome")?, kind, cols("covariates")?);
        r.treated_level = text("treated_level").map(str::to_owned);
        let d = MatchingOptions::default();
        let matching = MatchingOptions {
            ratio: num("ratio").map_or(d.ratio, |x| (x.max(1.0)) as usize),
            replace: flag("replace", d.replace),
            caliper: match v.get("caliper") {
                None => d.caliper,
                Some(c) => c.as_f64(),
            },
        };
        r.method = match text("method").unwrap_or("weighting") {
            "matching" => EffectMethod::Matching(matching),
            "weighting" => EffectMethod::Weighting,
            "doubly-robust" => EffectMethod::DoublyRobust,
            other => return Err(invalid(format!("treatment effects: unknown method {other:?}"))),
        };
        if let Some(e) = text("estimand") {
            r.estimand =
                Estimand::from_key(e).ok_or_else(|| invalid(format!("treatment effects: unknown estimand {e:?}")))?;
        }
        r.stabilized = flag("stabilized", r.stabilized);
        r.trim = num("trim").unwrap_or(r.trim);
        r.intervals = match text("intervals").unwrap_or("sandwich") {
            "sandwich" => IntervalMethod::Sandwich,
            "bootstrap" => IntervalMethod::Bootstrap {
                replicates: num("replicates").map_or(500, |x| x.max(0.0) as usize),
                seed: num("seed").map_or(1, |x| x.max(0.0) as u64),
            },
            other => return Err(invalid(format!("treatment effects: unknown interval method {other:?}"))),
        };
        r.level = num("level").unwrap_or(r.level);
        r.subgroups = cols("subgroups")?;
        Ok(r)
    }
}

/// One covariate as read from the table.
#[derive(Debug, Clone)]
enum Covariate {
    Continuous { name: String, values: Vec<f64> },
    Categorical { name: String, labels: Vec<String> },
}

/// The data of one analysis (all rows used, or one subgroup).
struct Prepared {
    rows: Vec<usize>,
    x: Matrix,
    names: Vec<String>,
    t: Vec<f64>,
    y: Vec<f64>,
    /// Balance variables: each continuous covariate and each level of a
    /// categorical one.
    balance_vars: Vec<(String, Vec<f64>)>,
    left_out: Vec<String>,
}

/// One subgroup's estimate.
#[derive(Debug, Clone)]
pub struct SubgroupEstimate {
    /// Subgroup column.
    pub column: String,
    /// Level.
    pub level: String,
    /// Rows `(treated, control)`.
    pub n: (usize, usize),
    /// The fit, or why it failed.
    pub fit: std::result::Result<TreatmentEffectFit, String>,
}

/// A computed treatment-effect analysis.
#[derive(Debug, Clone)]
pub struct TreatmentEffectsAnalysis {
    request: TreatmentEffectsRequest,
    treatment_name: String,
    outcome_name: String,
    treated: String,
    control: String,
    n_rows: usize,
    rows: Vec<usize>,
    names: Vec<String>,
    t: Vec<f64>,
    fit: TreatmentEffectFit,
    balance: Vec<BalanceRow>,
    left_out: Vec<String>,
    subgroups: Vec<SubgroupEstimate>,
    n_excluded: usize,
    n_missing: usize,
}

impl TreatmentEffectsAnalysis {
    /// Run the analysis.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a treatment without exactly two
    /// values, no covariates, and errors from
    /// [`estimate_treatment_effect`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &TreatmentEffectsRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        if request.covariates.is_empty() {
            return Err(invalid("choose at least one covariate"));
        }
        if request.covariates.contains(&request.treatment) || request.covariates.contains(&request.outcome) {
            return Err(invalid("the treatment and the outcome cannot also be covariates"));
        }
        if request.treatment == request.outcome {
            return Err(invalid("the treatment and the outcome must be different columns"));
        }
        let tc = column(request.treatment)?;
        let oc = column(request.outcome)?;
        let outcome = oc.as_numeric().ok_or_else(|| invalid(format!("the outcome {:?} must be numeric", oc.name)))?;
        let covariates: Vec<Covariate> = request
            .covariates
            .iter()
            .map(|&c| {
                let c = column(c)?;
                Ok(match (c.as_numeric(), c.level) {
                    (Some(v), MeasurementLevel::Continuous) => {
                        Covariate::Continuous { name: c.name.clone(), values: v.to_vec() }
                    }
                    _ => Covariate::Categorical {
                        name: c.name.clone(),
                        labels: (0..table.n_rows()).map(|r| c.level_text(r)).collect(),
                    },
                })
            })
            .collect::<Result<_>>()?;
        let labels: Vec<String> = (0..table.n_rows()).map(|r| tc.level_text(r).trim().to_owned()).collect();
        let present = |r: usize| {
            !labels[r].is_empty()
                && outcome[r].is_finite()
                && covariates.iter().all(|c| match c {
                    Covariate::Continuous { values, .. } => values[r].is_finite(),
                    Covariate::Categorical { labels, .. } => !labels[r].trim().is_empty(),
                })
        };
        let complete: Vec<usize> = (0..table.n_rows()).filter(|&r| present(r)).collect();
        let rows: Vec<usize> = complete.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        let mut levels: Vec<String> = rows.iter().map(|&r| labels[r].clone()).collect();
        levels.sort_by(|a, b| natural_order(a, b));
        levels.dedup();
        if levels.len() != 2 {
            return Err(invalid(format!(
                "the treatment {:?} needs exactly two values among the rows used; it has {}",
                tc.name,
                levels.len()
            )));
        }
        if let Some(level) = &request.treated_level {
            match levels.iter().position(|l| l == level.trim()) {
                Some(0) => levels.swap(0, 1),
                Some(_) => {}
                None => {
                    return Err(invalid(format!(
                        "the treated value {level:?} is not one of the values of {:?} ({} and {})",
                        tc.name, levels[0], levels[1]
                    )));
                }
            }
        }
        let (control, treated) = (levels[0].clone(), levels[1].clone());
        let t_all: Vec<f64> = labels.iter().map(|l| f64::from(*l == treated)).collect();
        let main = prepare(&rows, &covariates, &t_all, outcome)?;
        let options = request.options();
        let fit = estimate_treatment_effect(&main.x, &main.names, &main.t, &main.y, &options)?;
        let mut balance_vars = main.balance_vars.clone();
        balance_vars.push(("Propensity score".to_owned(), fit.propensity.scores.clone()));
        let balance = covariate_balance(&balance_vars, &main.t, &fit.weights, request.estimand);
        let mut subgroups = Vec::new();
        for &g in &request.subgroups {
            let gc = column(g)?;
            let glabels: Vec<String> = (0..table.n_rows()).map(|r| gc.level_text(r).trim().to_owned()).collect();
            let mut glevels: Vec<String> = rows.iter().map(|&r| glabels[r].clone()).filter(|l| !l.is_empty()).collect();
            glevels.sort_by(|a, b| natural_order(a, b));
            glevels.dedup();
            for level in glevels {
                let sub: Vec<usize> = rows.iter().copied().filter(|&r| glabels[r] == level).collect();
                let treated_n = sub.iter().filter(|&&r| t_all[r] == 1.0).count();
                let fit = prepare(&sub, &covariates, &t_all, outcome)
                    .and_then(|p| estimate_treatment_effect(&p.x, &p.names, &p.t, &p.y, &options))
                    .map_err(|e| e.to_string());
                subgroups.push(SubgroupEstimate {
                    column: gc.name.clone(),
                    level,
                    n: (treated_n, sub.len() - treated_n),
                    fit,
                });
            }
        }
        Ok(Self {
            request: request.clone(),
            treatment_name: tc.name.clone(),
            outcome_name: oc.name.clone(),
            treated,
            control,
            n_rows: table.n_rows(),
            n_excluded: complete.len() - rows.len(),
            n_missing: table.n_rows() - complete.len(),
            rows: main.rows,
            names: main.names,
            t: main.t,
            balance,
            left_out: main.left_out,
            fit,
            subgroups,
        })
    }

    /// The fit on every row used.
    #[must_use]
    pub fn fit(&self) -> &TreatmentEffectFit {
        &self.fit
    }

    /// Covariate balance (the propensity score last).
    #[must_use]
    pub fn balance(&self) -> &[BalanceRow] {
        &self.balance
    }

    /// Subgroup estimates, by subgroup column and level.
    #[must_use]
    pub fn subgroups(&self) -> &[SubgroupEstimate] {
        &self.subgroups
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Treatment Effect: {} by {}", self.outcome_name, self.treatment_name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.effects(), self.means(), self.balance_table(), self.balance_plot()];
        out.push(self.overlap_plot());
        out.push(self.propensity_table());
        if let Some(models) = self.outcome_models() {
            out.push(models);
        }
        if !self.subgroups.is_empty() {
            out.push(self.subgroup_table());
            if let Some(p) = self.forest_plot() {
                out.push(p);
            }
        }
        out
    }

    fn level_percent(&self) -> String {
        format_number(100.0 * self.request.level, 4)
    }

    fn summary(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Treatment", format!("{} = {} (treated) vs {} (control)", self.treatment_name, self.treated, self.control));
        row("Outcome", format!("{} ({})", self.outcome_name, self.request.outcome_kind.label().to_lowercase()));
        let method = match f.options.method {
            EffectMethod::Matching(m) => format!(
                "{} 1:{}, {} replacement, {}",
                f.options.method.label(),
                m.ratio,
                if m.replace { "with" } else { "without" },
                m.caliper.map_or_else(
                    || "no caliper".to_owned(),
                    |c| format!("caliper {} SD of the logit", format_number(c, 3))
                )
            ),
            EffectMethod::Weighting => format!(
                "{} ({} weights)",
                f.options.method.label(),
                if f.options.stabilized { "stabilized" } else { "unstabilized" }
            ),
            EffectMethod::DoublyRobust => f.options.method.label().to_owned(),
        };
        row("Method", method);
        row("Estimand", f.options.estimand.label().to_owned());
        row(
            "Intervals",
            match f.options.intervals {
                IntervalMethod::Sandwich if matches!(f.options.method, EffectMethod::Matching(m) if !m.replace) => {
                    "robust, clustered on matched sets".to_owned()
                }
                IntervalMethod::Sandwich if matches!(f.options.method, EffectMethod::Matching(_)) => {
                    "robust (HC1)".to_owned()
                }
                IntervalMethod::Sandwich => "sandwich, allowing for the estimated propensity score".to_owned(),
                IntervalMethod::Bootstrap { replicates, seed } => {
                    format!("bootstrap percentile, {replicates} resamples within groups (seed {seed})")
                }
            },
        );
        row("Rows (treated, control)", format!("{}, {}", f.n.0, f.n.1));
        row("Rows used (treated, control)", format!("{}, {}", f.n_used.0, f.n_used.1));
        row(
            "Effective sample size (treated, control)",
            format!("{}, {}", format_number(f.effective_n.0, 5), format_number(f.effective_n.1, 5)),
        );
        if f.n_trimmed > 0 {
            row(
                "Rows trimmed",
                format!(
                    "{} (propensity score outside {}–{})",
                    f.n_trimmed,
                    format_number(f.options.trim, 3),
                    format_number(1.0 - f.options.trim, 3)
                ),
            );
        }
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        if !self.left_out.is_empty() {
            row("Covariates left out (constant)", self.left_out.join(", "));
        }
        let worst = self.balance.iter().map(|b| b.adjusted_smd.abs()).fold(0.0, f64::max);
        row(
            "Largest |standardized difference| after adjustment",
            format!(
                "{}{}",
                format_number(worst, 3),
                if worst < BALANCE_THRESHOLD {
                    " (all below 0.1)"
                } else {
                    " (some at or above 0.1: balance not reached)"
                }
            ),
        );
        for w in &f.warnings {
            row("Warning", w.clone());
        }
        section(SectionKind::TreatmentSummary, "Summary", SectionContent::Table(t))
    }

    fn effects(&self) -> ReportSection {
        let level = self.level_percent();
        let mut t = TextTable::new([
            "Effect",
            "Estimate",
            "Std Error",
            "z",
            "p-value",
            &format!("Lower {level}%"),
            &format!("Upper {level}%"),
        ]);
        for e in &self.fit.effects {
            t.push([
                e.scale.label(self.request.outcome_kind).to_owned(),
                format_number(e.estimate, DIGITS),
                format_number(e.std_error, DIGITS),
                format_number(e.z, 4),
                format_p_value(e.p_value),
                format_number(e.interval.0, DIGITS),
                format_number(e.interval.1, DIGITS),
            ]);
        }
        let note = if self.fit.effects.iter().any(|e| e.scale.is_ratio()) {
            "; ratios' standard errors are of their logarithms"
        } else {
            ""
        };
        section(
            SectionKind::TreatmentEffect,
            format!("Treatment Effect ({}{note})", self.fit.options.estimand.short()),
            SectionContent::Table(t),
        )
    }

    fn means(&self) -> ReportSection {
        let f = &self.fit;
        let mut t = TextTable::new(["Group", "Estimated mean outcome", "Std Error", "Rows used", "Sum of weights"]);
        for (g, label, m, v, n) in [
            (1.0, &self.treated, f.mean_treated, f.covariance[0][0], f.n_used.0),
            (0.0, &self.control, f.mean_control, f.covariance[1][1], f.n_used.1),
        ] {
            let sw: f64 = (0..self.t.len()).filter(|&i| self.t[i] == g).map(|i| f.weights[i]).sum();
            t.push([
                format!("{} = {label}", self.treatment_name),
                format_number(m, DIGITS),
                format_number(v.max(0.0).sqrt(), DIGITS),
                n.to_string(),
                format_number(sw, 6),
            ]);
        }
        section(SectionKind::TreatmentMeans, "Estimated Outcome Means", SectionContent::Table(t))
    }

    fn balance_table(&self) -> ReportSection {
        let mut t = TextTable::new([
            "Covariate",
            "Mean (treated)",
            "Mean (control)",
            "Std diff",
            "Variance ratio",
            "Adjusted mean (treated)",
            "Adjusted mean (control)",
            "Adjusted std diff",
            "Adjusted variance ratio",
            "Balanced",
        ]);
        let opt = |v: Option<f64>| v.map_or_else(String::new, |x| format_number(x, 4));
        for b in &self.balance {
            t.push([
                b.name.clone(),
                format_number(b.means.0, DIGITS),
                format_number(b.means.1, DIGITS),
                format_number(b.smd, 4),
                opt(b.variance_ratio),
                format_number(b.adjusted_means.0, DIGITS),
                format_number(b.adjusted_means.1, DIGITS),
                format_number(b.adjusted_smd, 4),
                opt(b.adjusted_variance_ratio),
                if b.adjusted_smd.abs() < BALANCE_THRESHOLD { "yes" } else { "no" }.to_owned(),
            ]);
        }
        let denominator = match self.request.estimand {
            Estimand::Ate => "pooled SD",
            Estimand::Att => "treated SD",
        };
        section(
            SectionKind::CovariateBalance,
            format!("Covariate Balance (standardized differences on the unadjusted {denominator})"),
            SectionContent::Table(t),
        )
    }

    fn balance_plot(&self) -> ReportSection {
        let k = self.balance.len();
        let y = |i: usize| (k - i) as f64;
        let mut items = Vec::new();
        for (i, b) in self.balance.iter().enumerate() {
            items.push((b.smd.abs(), y(i), String::new(), 0));
            items.push((b.adjusted_smd.abs(), y(i), String::new(), 1));
        }
        let top = k as f64 + 0.5;
        let spec = PlotSpec {
            x_label: "|Standardized mean difference|".to_owned(),
            y_label: String::new(),
            annotations: Some(Annotations {
                groups: vec!["Before adjustment".to_owned(), "After adjustment".to_owned()],
                items,
                labels: Vec::new(),
            }),
            y_ticks: Some(self.balance.iter().enumerate().map(|(i, b)| (y(i), b.name.clone())).collect()),
            line: Some(vec![[BALANCE_THRESHOLD, 0.5], [BALANCE_THRESHOLD, top]]),
            line_name: Some("Threshold 0.1".to_owned()),
            segments: self
                .balance
                .iter()
                .enumerate()
                .map(|(i, b)| vec![[b.smd.abs(), y(i)], [b.adjusted_smd.abs(), y(i)]])
                .collect(),
            ..PlotSpec::default()
        };
        section(SectionKind::BalancePlot, "Balance Plot", SectionContent::Plot(Box::new(spec)))
    }

    fn overlap_plot(&self) -> ReportSection {
        let e = &self.fit.propensity.scores;
        // A fixed jitter from the row number keeps the plot the same on
        // every run.
        let jitter = |row: usize| (((row as u64).wrapping_mul(2_654_435_761) % 1000) as f64 / 1000.0 - 0.5) * 0.6;
        let points: Vec<RowPoint> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, &row)| RowPoint { row, x: e[i], y: self.t[i] + jitter(row) })
            .collect();
        let membership = self.t.iter().map(|&t| usize::from(t == 0.0)).collect();
        let spec = PlotSpec {
            x_label: format!("Propensity score: P({} = {})", self.treatment_name, self.treated),
            y_label: self.treatment_name.clone(),
            points,
            groups: Some(PointGroups {
                title: self.treatment_name.clone(),
                labels: vec![self.treated.clone(), self.control.clone()],
                membership,
                lines: Vec::new(),
            }),
            y_ticks: Some(vec![(0.0, self.control.clone()), (1.0, self.treated.clone())]),
            ..PlotSpec::default()
        };
        section(SectionKind::PropensityOverlap, "Propensity Score Overlap", SectionContent::Plot(Box::new(spec)))
    }

    fn coefficient_table(fit: &stat_discrete::GlmFit) -> TextTable {
        let mut t = TextTable::new(["Term", "Estimate", "Std Error", "z", "p-value"]);
        for c in &fit.coefficients {
            t.push([
                c.name.clone(),
                format_number(c.estimate, DIGITS),
                format_number(c.std_error, DIGITS),
                format_number(c.statistic, 4),
                format_p_value(c.p_value),
            ]);
        }
        t
    }

    fn propensity_table(&self) -> ReportSection {
        let fit = &self.fit.propensity.fit;
        section(
            SectionKind::PropensityModel,
            format!("Propensity Score Model (logistic regression of {} = {})", self.treatment_name, self.treated),
            SectionContent::Table(Self::coefficient_table(fit)),
        )
    }

    fn outcome_models(&self) -> Option<ReportSection> {
        let (m1, m0) = self.fit.outcome_models.as_ref()?;
        let mut t = TextTable::new(["Group", "Term", "Estimate", "Std Error", "z", "p-value"]);
        for (label, m) in [(&self.treated, m1.as_ref()), (&self.control, Some(m0))] {
            let Some(m) = m else { continue };
            for r in Self::coefficient_table(m).rows {
                let mut row = vec![label.clone()];
                row.extend(r);
                t.push(row);
            }
        }
        let family = match self.request.outcome_kind {
            OutcomeKind::Continuous => "normal linear",
            OutcomeKind::Binary => "logistic",
            OutcomeKind::Count => "Poisson",
        };
        Some(section(
            SectionKind::OutcomeModels,
            format!("Outcome Models ({family}, fitted in each group)"),
            SectionContent::Table(t),
        ))
    }

    /// The scale subgroups are compared on: the first (difference).
    fn subgroup_scale(&self) -> EffectScale {
        EffectScale::Difference
    }

    fn subgroup_table(&self) -> ReportSection {
        let level = self.level_percent();
        let scale = self.subgroup_scale();
        let mut t = TextTable::new([
            "Subgroup",
            "Level",
            "Treated rows",
            "Control rows",
            scale.label(self.request.outcome_kind),
            "Std Error",
            &format!("Lower {level}%"),
            &format!("Upper {level}%"),
            "p-value",
        ]);
        let mut columns: Vec<&str> = Vec::new();
        for s in &self.subgroups {
            if !columns.contains(&s.column.as_str()) {
                columns.push(&s.column);
            }
        }
        for column in &columns {
            let mut estimates = Vec::new();
            for s in self.subgroups.iter().filter(|s| s.column == *column) {
                let mut row = vec![s.column.clone(), s.level.clone(), s.n.0.to_string(), s.n.1.to_string()];
                match &s.fit {
                    Ok(fit) => {
                        let e = fit.effects.iter().find(|e| e.scale == scale).expect("difference always reported");
                        estimates.push((e.analysis_estimate(), e.std_error));
                        row.extend([
                            format_number(e.estimate, DIGITS),
                            format_number(e.std_error, DIGITS),
                            format_number(e.interval.0, DIGITS),
                            format_number(e.interval.1, DIGITS),
                            format_p_value(e.p_value),
                        ]);
                    }
                    Err(message) => {
                        row.push(format!("not estimable: {message}"));
                        row.extend(std::iter::repeat_n(String::new(), 4));
                    }
                }
                t.push(row);
            }
            let test = heterogeneity_test(&estimates).map_or_else(
                || "not available: fewer than two subgroups estimated".to_owned(),
                |(q, df, p)| format!("Q = {} on {df} df, p = {}", format_number(q, 5), format_p_value(p)),
            );
            t.push([
                column.to_string(),
                "Test of equal effects".to_owned(),
                test,
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ]);
        }
        section(
            SectionKind::SubgroupEffects,
            "Subgroup Effects (each subgroup refitted; equal effects tested by Cochran's Q)",
            SectionContent::Table(t),
        )
    }

    fn forest_plot(&self) -> Option<ReportSection> {
        let scale = self.subgroup_scale();
        let overall = self.fit.effects.iter().find(|e| e.scale == scale)?;
        let mut rows: Vec<(String, f64, (f64, f64))> =
            vec![("All rows used".to_owned(), overall.estimate, overall.interval)];
        for s in &self.subgroups {
            if let Ok(fit) = &s.fit {
                if let Some(e) = fit.effects.iter().find(|e| e.scale == scale) {
                    rows.push((format!("{}: {}", s.column, s.level), e.estimate, e.interval));
                }
            }
        }
        if rows.len() < 2 {
            return None;
        }
        let k = rows.len();
        let y = |i: usize| (k - i) as f64;
        let spec = PlotSpec {
            x_label: format!("{} ({}% interval)", scale.label(self.request.outcome_kind), self.level_percent()),
            y_label: String::new(),
            segments: rows.iter().enumerate().map(|(i, r)| vec![[r.2.0, y(i)], [r.2.1, y(i)]]).collect(),
            annotations: Some(Annotations {
                groups: vec!["All rows".to_owned(), "Subgroup".to_owned()],
                items: rows.iter().enumerate().map(|(i, r)| (r.1, y(i), String::new(), usize::from(i > 0))).collect(),
                labels: Vec::new(),
            }),
            y_ticks: Some(rows.iter().enumerate().map(|(i, r)| (y(i), r.0.clone())).collect()),
            line: Some(vec![[0.0, 0.5], [0.0, k as f64 + 0.5]]),
            line_name: Some("No effect".to_owned()),
            ..PlotSpec::default()
        };
        Some(section(SectionKind::SubgroupForest, "Subgroup Forest Plot", SectionContent::Plot(Box::new(spec))))
    }

    /// Columns the report can add: `propensity` (the score), `weight` (the
    /// analysis weight; 0 for rows left out) and, for matching without
    /// replacement, `matched_set`.
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        let spread = |v: &dyn Fn(usize) -> f64| {
            let mut all = vec![f64::NAN; self.n_rows];
            for (i, &r) in self.rows.iter().enumerate() {
                all[r] = v(i);
            }
            all
        };
        let f = &self.fit;
        let mut out = vec![
            ReportColumn::continuous(
                "propensity",
                "Save propensity scores",
                format!("Propensity {} = {}", self.treatment_name, self.treated),
                spread(&|i| f.propensity.scores[i]),
            ),
            ReportColumn::continuous(
                "weight",
                "Save weights",
                format!("{} weight ({})", self.method_short(), f.options.estimand.short()),
                spread(&|i| f.weights[i]),
            ),
        ];
        if matches!(f.options.method, EffectMethod::Matching(m) if !m.replace) {
            let mut set = ReportColumn::continuous(
                "matched_set",
                "Save matched sets",
                "Matched set".to_owned(),
                spread(&|i| f.matched_set[i].map_or(f64::NAN, |s| s as f64)),
            );
            set.level = MeasurementLevel::Nominal;
            out.push(set);
        }
        out
    }

    fn method_short(&self) -> &'static str {
        match self.fit.options.method {
            EffectMethod::Matching(_) => "Matching",
            EffectMethod::Weighting => "IPW",
            EffectMethod::DoublyRobust => "AIPW",
        }
    }

    /// Covariate names in the propensity model (indicators named
    /// `"column = level"`).
    #[must_use]
    pub fn model_terms(&self) -> &[String] {
        &self.names
    }
}

/// Build the design of `rows`: continuous covariates as they are and
/// indicators of each categorical level but the first; constant columns
/// are left out.
fn prepare(rows: &[usize], covariates: &[Covariate], t_all: &[f64], outcome: &[f64]) -> Result<Prepared> {
    let mut columns: Vec<(String, Vec<f64>)> = Vec::new();
    let mut balance_vars = Vec::new();
    let mut left_out = Vec::new();
    for c in covariates {
        match c {
            Covariate::Continuous { name, values } => {
                let v: Vec<f64> = rows.iter().map(|&r| values[r]).collect();
                balance_vars.push((name.clone(), v.clone()));
                columns.push((name.clone(), v));
            }
            Covariate::Categorical { name, labels } => {
                let mut levels: Vec<&str> = rows.iter().map(|&r| labels[r].trim()).collect();
                levels.sort_by(|a, b| natural_order(a, b));
                levels.dedup();
                for (k, level) in levels.iter().enumerate() {
                    let v: Vec<f64> = rows.iter().map(|&r| f64::from(labels[r].trim() == *level)).collect();
                    let label = format!("{name} = {level}");
                    balance_vars.push((label.clone(), v.clone()));
                    if k > 0 {
                        columns.push((label, v));
                    }
                }
                if levels.len() < 2 {
                    left_out.push(name.clone());
                }
            }
        }
    }
    let mut kept = Vec::new();
    for (name, v) in columns {
        let first = v.first().copied().unwrap_or_default();
        if v.iter().all(|&x| x == first) {
            if !left_out.contains(&name) {
                left_out.push(name);
            }
        } else {
            kept.push((name, v));
        }
    }
    balance_vars.retain(|(_, v)| v.iter().any(|&x| x != v[0]));
    if kept.is_empty() {
        return Err(invalid("every covariate is constant among the rows used"));
    }
    let names: Vec<String> = kept.iter().map(|(n, _)| n.clone()).collect();
    let x = Matrix::from_columns(&kept.iter().map(|(_, v)| v.as_slice()).collect::<Vec<_>>())?;
    Ok(Prepared {
        rows: rows.to_vec(),
        x,
        names,
        t: rows.iter().map(|&r| t_all[r]).collect(),
        y: rows.iter().map(|&r| outcome[r]).collect(),
        balance_vars,
        left_out,
    })
}

#[cfg(test)]
mod tests;
