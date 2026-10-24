//! A gauge R&R study bound to a dataset and its row state.
//!
//! The study is laid out in "long" form: one row per measurement, with a column
//! identifying the part, a column identifying the operator, and a numeric
//! measurement column (plus, for an expanded study, a column per extra factor).
//! Part, operator and factor columns may be text or numeric; their distinct
//! values become the levels, in order of first appearance. Excluded rows and
//! rows with a missing value in any column the study uses are left out (which
//! can unbalance the design; the crossed and nested studies then explain why
//! they cannot be computed, and the expanded study handles it).
//!
//! The study layout ([`GaugeStudyDesign`]) chooses the method: crossed and
//! nested studies use the ANOVA method, expanded studies REML (see
//! [`crate::gauge_design`]).

use crate::format::format_number;
use crate::gauge_design::{GaugeFactorNesting, GaugeStudyDesign, GaugeStudyKind};
use crate::report::{
    PlotPane, PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_quality::gauge::{GaugeAnovaRow, VarianceComponent};
use stat_quality::{
    ComponentRole, ExpandedGaugeOptions, ExpandedGaugeStudy, ExpandedTotal, GaugeFactor, GaugeObservation,
    GaugeOptions, GaugeSource, GaugeStudy, gauge_rr_anova, gauge_rr_expanded, gauge_rr_nested,
};

const DIGITS: usize = 6;
/// Significance level above which the interaction is pooled, when pooling is on.
pub const POOLING_LEVEL: f64 = 0.05;
/// Confidence level of the expanded study's variance intervals.
pub const INTERVAL_LEVEL: f64 = 0.95;

/// Which columns hold the study, and analysis settings.
#[derive(Debug, Clone, PartialEq)]
pub struct GaugeStudyRequest {
    /// Part identifier column.
    pub part: usize,
    /// Operator identifier column.
    pub operator: usize,
    /// Numeric measurement column.
    pub measurement: usize,
    /// Specification width `USL − LSL`, for %Tolerance.
    pub tolerance: Option<f64>,
    /// Pool the interaction into error when its p-value exceeds [`POOLING_LEVEL`]
    /// (crossed studies only).
    pub pool_interaction: bool,
    /// Multiplier converting σ to study variation (usually 6).
    pub study_multiplier: f64,
    /// Crossed, nested or expanded layout, extra factors and run order.
    pub design: GaugeStudyDesign,
}

impl GaugeStudyRequest {
    /// A crossed study of `measurement` by `part` and `operator` with the
    /// usual settings (6σ study variation, no tolerance, no pooling).
    #[must_use]
    pub fn new(part: usize, operator: usize, measurement: usize) -> Self {
        Self {
            part,
            operator,
            measurement,
            tolerance: None,
            pool_interaction: false,
            study_multiplier: 6.0,
            design: GaugeStudyDesign::default(),
        }
    }

    /// Every column the study refers to.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        [self.part, self.operator, self.measurement].into_iter().chain(self.design.columns()).collect()
    }
}

/// User-toggleable parts of a gauge report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GaugeReportOptions {
    /// Show the ANOVA table (crossed and nested studies).
    pub show_anova: bool,
    /// Show measurements plotted by part.
    pub show_part_plot: bool,
    /// Show measurements plotted by operator.
    pub show_operator_plot: bool,
    /// Show the gauge run chart (each part's measurements in run order).
    pub show_run_chart: bool,
}

impl Default for GaugeReportOptions {
    fn default() -> Self {
        Self { show_anova: true, show_part_plot: true, show_operator_plot: false, show_run_chart: false }
    }
}

/// The fitted study.
#[derive(Debug, Clone)]
enum GaugeFit {
    /// Crossed or nested, by the ANOVA method.
    Anova(Box<GaugeStudy>),
    /// Expanded, by REML.
    Expanded(Box<ExpandedGaugeStudy>),
}

/// A computed gauge study plus the data needed to report and plot it.
#[derive(Debug, Clone)]
pub struct GaugeAnalysis {
    request: GaugeStudyRequest,
    measurement_name: String,
    part_name: String,
    operator_name: String,
    fit: GaugeFit,
    part_levels: Vec<String>,
    operator_levels: Vec<String>,
    /// Parts are identified within operators (nested layouts).
    nested_parts: bool,
    /// (row, part index, operator index, value) for every observation used.
    observations: Vec<(usize, usize, usize, f64)>,
    /// Run-order key of each observation (the run-order column, or the row).
    run_keys: Vec<f64>,
    /// Extra factors: name, number of levels used.
    factor_summaries: Vec<(String, usize)>,
    n_excluded: usize,
    n_missing: usize,
    exclusion_revision: u64,
}

/// Level index of `label` in `levels`, adding it when new.
fn level_of(levels: &mut Vec<String>, label: String) -> usize {
    match levels.iter().position(|l| *l == label) {
        Some(i) => i,
        None => {
            levels.push(label);
            levels.len() - 1
        }
    }
}

impl GaugeAnalysis {
    /// Run the study described by `request`, honouring row exclusion.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for invalid or repeated columns, a
    /// non-numeric measurement or run-order column, an unbalanced or too
    /// small crossed or nested design, or an expanded model that cannot be
    /// fitted.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &GaugeStudyRequest) -> Result<Self> {
        let (p, o, m) = (request.part, request.operator, request.measurement);
        if p == o || p == m || o == m {
            return Err(invalid("part, operator and measurement must be three different columns"));
        }
        let design = &request.design;
        let expanded = design.kind == GaugeStudyKind::Expanded;
        let extras: &[crate::gauge_design::GaugeExtraFactor] = if expanded { &design.factors } else { &[] };
        let mut used = vec![p, o, m];
        for f in extras {
            if used.contains(&f.column) {
                return Err(invalid(
                    "each extra factor must be a different column from the part, operator, measurement and other factors",
                ));
            }
            used.push(f.column);
        }
        let col = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let (part_col, oper_col, meas_col) = (col(p)?, col(o)?, col(m)?);
        let values = meas_col
            .as_numeric()
            .ok_or_else(|| invalid(format!("measurement column {:?} is not numeric", meas_col.name)))?;
        let run_values = match design.run_order {
            Some(c) => Some(col(c)?.as_numeric().ok_or_else(|| {
                invalid(format!("run-order column {:?} is not numeric", col(c).map_or("", |x| x.name.as_str())))
            })?),
            None => None,
        };
        let extra_cols = extras.iter().map(|f| col(f.column)).collect::<Result<Vec<_>>>()?;
        let nested_parts = design.kind == GaugeStudyKind::Nested || (expanded && design.parts_within_operators);

        let mut operator_levels: Vec<String> = Vec::new();
        let mut part_keys: Vec<(usize, String)> = Vec::new();
        let mut extra_levels: Vec<Vec<String>> = vec![Vec::new(); extras.len()];
        let mut extra_codes: Vec<Vec<usize>> = vec![Vec::new(); extras.len()];
        let mut observations = Vec::new();
        let mut run_keys = Vec::new();
        let (mut n_excluded, mut n_missing) = (0, 0);
        for r in 0..table.n_rows() {
            let (pl, ol, v) = (part_col.level_text(r), oper_col.level_text(r), values[r]);
            let el: Vec<String> = extra_cols.iter().map(|c| c.level_text(r)).collect();
            let run = run_values.map_or(r as f64, |rv| rv[r]);
            if pl.is_empty() || ol.is_empty() || !v.is_finite() || !run.is_finite() || el.iter().any(String::is_empty) {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                let oi = level_of(&mut operator_levels, ol);
                let key = (if nested_parts { oi } else { 0 }, pl);
                let pi = part_keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                    part_keys.push(key);
                    part_keys.len() - 1
                });
                for (k, label) in el.into_iter().enumerate() {
                    let c = level_of(&mut extra_levels[k], label);
                    extra_codes[k].push(c);
                }
                observations.push((r, pi, oi, v));
                run_keys.push(run);
            }
        }
        // A part label reused by several operators is shown with its operator.
        let part_levels: Vec<String> = part_keys
            .iter()
            .map(|(oi, label)| {
                let shared = part_keys.iter().filter(|(_, l)| l == label).count() > 1;
                if shared { format!("{label} ({})", operator_levels[*oi]) } else { label.clone() }
            })
            .collect();
        let gauge_options = GaugeOptions {
            study_multiplier: request.study_multiplier,
            tolerance: request.tolerance,
            pool_interaction_above: (request.pool_interaction && design.kind == GaugeStudyKind::Crossed)
                .then_some(POOLING_LEVEL),
        };
        let obs: Vec<GaugeObservation> =
            observations.iter().map(|&(_, part, operator, value)| GaugeObservation { part, operator, value }).collect();
        let balance_hint = |e: StatError, what: &str| match e {
            StatError::InvalidArgument { message } if message.contains("balanced") => invalid(format!(
                "{message}. {what}; excluded rows and missing values count against this. The expanded study type \
                 handles unbalanced data."
            )),
            other => other,
        };
        let fit = match design.kind {
            GaugeStudyKind::Crossed => {
                GaugeFit::Anova(Box::new(gauge_rr_anova(&obs, &gauge_options).map_err(|e| {
                    balance_hint(e, "Every part must be measured the same number of times by every operator")
                })?))
            }
            GaugeStudyKind::Nested => {
                GaugeFit::Anova(Box::new(gauge_rr_nested(&obs, &gauge_options).map_err(|e| {
                    balance_hint(e, "Every operator must measure the same number of parts, each equally often")
                })?))
            }
            GaugeStudyKind::Expanded => {
                let mut factors = vec![
                    GaugeFactor {
                        name: part_col.name.clone(),
                        levels: part_levels.clone(),
                        codes: observations.iter().map(|o| o.1).collect(),
                        random: true,
                        nested_in: if design.parts_within_operators { vec![1] } else { Vec::new() },
                    },
                    GaugeFactor {
                        name: oper_col.name.clone(),
                        levels: operator_levels.clone(),
                        codes: observations.iter().map(|o| o.2).collect(),
                        random: true,
                        nested_in: Vec::new(),
                    },
                ];
                for (k, f) in extras.iter().enumerate() {
                    factors.push(GaugeFactor {
                        name: extra_cols[k].name.clone(),
                        levels: extra_levels[k].clone(),
                        codes: extra_codes[k].clone(),
                        random: f.random,
                        nested_in: match f.nesting {
                            GaugeFactorNesting::Crossed => Vec::new(),
                            GaugeFactorNesting::WithinPart => vec![0],
                            GaugeFactorNesting::WithinOperator => vec![1],
                        },
                    });
                }
                if observations.is_empty() {
                    return Err(invalid("no complete, included rows to analyse"));
                }
                let y: Vec<f64> = observations.iter().map(|o| o.3).collect();
                let options = ExpandedGaugeOptions {
                    gauge: gauge_options,
                    confidence_level: INTERVAL_LEVEL,
                    interactions: design.interactions,
                };
                GaugeFit::Expanded(Box::new(gauge_rr_expanded(&y, &factors, 0, &options)?))
            }
        };
        let factor_summaries = extra_cols.iter().zip(&extra_levels).map(|(c, l)| (c.name.clone(), l.len())).collect();
        Ok(Self {
            request: request.clone(),
            measurement_name: meas_col.name.clone(),
            part_name: part_col.name.clone(),
            operator_name: oper_col.name.clone(),
            fit,
            part_levels,
            operator_levels,
            nested_parts,
            observations,
            run_keys,
            factor_summaries,
            n_excluded,
            n_missing,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &GaugeStudyRequest {
        &self.request
    }

    /// The computed crossed or nested (ANOVA-method) study; `None` for an
    /// expanded study (see [`expanded`](Self::expanded)).
    #[must_use]
    pub fn study(&self) -> Option<&GaugeStudy> {
        match &self.fit {
            GaugeFit::Anova(s) => Some(s.as_ref()),
            GaugeFit::Expanded(_) => None,
        }
    }

    /// The computed expanded (REML) study, if this is one.
    #[must_use]
    pub fn expanded(&self) -> Option<&ExpandedGaugeStudy> {
        match &self.fit {
            GaugeFit::Anova(_) => None,
            GaugeFit::Expanded(s) => Some(s),
        }
    }

    /// `true` if row exclusion changed since the study was computed.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match self.request.design.kind {
            GaugeStudyKind::Crossed => "Gauge R&R",
            GaugeStudyKind::Nested => "Nested gauge R&R",
            GaugeStudyKind::Expanded => "Expanded gauge R&R",
        };
        format!("{kind}: {}", self.measurement_name)
    }

    /// Report sections, honouring `options`.
    #[must_use]
    pub fn sections(&self, options: &GaugeReportOptions) -> Vec<ReportSection> {
        let mut out = match &self.fit {
            GaugeFit::Anova(study) => {
                let mut out = vec![self.components_table(study), self.summary_table(study)];
                if options.show_anova {
                    out.push(self.anova_table(study));
                }
                out
            }
            GaugeFit::Expanded(study) => {
                let mut out = vec![self.expanded_components_table(study), self.intervals_table(study)];
                if !study.fixed_tests.is_empty() {
                    out.push(self.fixed_table(study));
                }
                out.push(self.expanded_summary(study));
                out
            }
        };
        if options.show_run_chart {
            out.push(self.run_chart());
        }
        if options.show_part_plot {
            out.push(self.measurement_plot(true));
        }
        if options.show_operator_plot {
            out.push(self.measurement_plot(false));
        }
        out
    }

    fn component_columns(&self) -> Vec<String> {
        let k = format_number(self.request.study_multiplier, 3);
        let mut cols = vec![
            "Source".to_owned(),
            "Variance".into(),
            "% Contribution".into(),
            "Std Dev".into(),
            format!("Study Var ({k}σ)"),
            "% Study Var".into(),
        ];
        if self.request.tolerance.is_some() {
            cols.push("% Tolerance".into());
        }
        cols
    }

    fn component_row(&self, name: &str, v: &VarianceComponent) -> Vec<String> {
        let mut row = vec![
            name.to_owned(),
            format_number(v.variance, DIGITS),
            percent(v.percent_contribution),
            format_number(v.std_dev, DIGITS),
            format_number(v.study_variation, DIGITS),
            percent(v.percent_study_variation),
        ];
        if self.request.tolerance.is_some() {
            row.push(v.percent_tolerance.map_or_else(|| "—".to_owned(), percent));
        }
        row
    }

    fn components_table(&self, study: &GaugeStudy) -> ReportSection {
        let mut t = TextTable::new(self.component_columns());
        let c = &study.components;
        let nested = self.request.design.kind == GaugeStudyKind::Nested;
        let mut rows: Vec<(&str, &VarianceComponent)> = vec![
            ("Total gauge R&R", &c.gauge_rr),
            ("  Repeatability", &c.repeatability),
            ("  Reproducibility", &c.reproducibility),
            ("    Operator", &c.operator),
        ];
        if !nested {
            rows.push(("    Part × operator", &c.interaction));
        }
        rows.push((if nested { "Part-to-part (within operator)" } else { "Part-to-part" }, &c.part));
        rows.push(("Total variation", &c.total));
        for (name, v) in rows {
            t.push(self.component_row(name, v));
        }
        section(SectionKind::VarianceComponents, "Variance Components", SectionContent::Table(t))
    }

    fn summary_table(&self, s: &GaugeStudy) -> ReportSection {
        let nested = self.request.design.kind == GaugeStudyKind::Nested;
        let mut t = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        if nested {
            row("Design", "nested: each part measured by one operator".to_owned());
        }
        row(&format!("Parts ({})", self.part_name), s.parts.to_string());
        row(&format!("Operators ({})", self.operator_name), s.operators.to_string());
        if nested {
            row("Parts per operator", (s.parts / s.operators.max(1)).to_string());
            row("Measurements per part", s.replicates.to_string());
        } else {
            row("Measurements per part per operator", s.replicates.to_string());
        }
        row("Number of distinct categories", s.distinct_categories.map_or_else(|| "—".to_owned(), |n| n.to_string()));
        if let Some(tol) = self.request.tolerance {
            row("Tolerance (USL − LSL)", format_number(tol, DIGITS));
        }
        if !nested {
            row(
                "Interaction",
                if s.interaction_pooled {
                    format!("pooled into repeatability (p > {POOLING_LEVEL})")
                } else {
                    "kept in the model".to_owned()
                },
            );
        }
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        section(SectionKind::GaugeSummary, "Study Summary", SectionContent::Table(t))
    }

    fn anova_table(&self, study: &GaugeStudy) -> ReportSection {
        let mut t = TextTable::new(["Source", "DF", "Sum of Squares", "Mean Square", "F", "p-value"]);
        for r in &study.anova {
            t.push(anova_row(r, &self.part_name, &self.operator_name));
        }
        section(SectionKind::GaugeAnova, "Analysis of Variance", SectionContent::Table(t))
    }

    fn expanded_components_table(&self, s: &ExpandedGaugeStudy) -> ReportSection {
        let mut t = TextTable::new(self.component_columns());
        t.push(self.component_row("Total gauge R&R", &s.gauge_rr.summary));
        t.push(self.component_row("  Repeatability", &s.repeatability.summary));
        t.push(self.component_row("  Reproducibility", &s.reproducibility.summary));
        for c in s.components.iter().filter(|c| c.role == ComponentRole::Reproducibility) {
            t.push(self.component_row(&format!("    {}", c.source), &c.summary));
        }
        t.push(self.component_row("Part-to-part", &s.part.summary));
        let within: Vec<_> = s.components.iter().filter(|c| c.role == ComponentRole::WithinPart).collect();
        if !within.is_empty() {
            // Within-part variation is product variation (AIAG MSA): shown
            // under part-to-part, outside gauge R&R.
            for c in s.components.iter().filter(|c| c.role == ComponentRole::Part) {
                t.push(self.component_row("  Between parts", &c.summary));
            }
            t.push(self.component_row("  Within part", &s.within_part.summary));
            for c in within {
                t.push(self.component_row(&format!("    {}", c.source), &c.summary));
            }
        }
        t.push(self.component_row("Total variation", &s.total.summary));
        section(SectionKind::VarianceComponents, "Variance Components", SectionContent::Table(t))
    }

    fn intervals_table(&self, s: &ExpandedGaugeStudy) -> ReportSection {
        let pct = format_number(100.0 * INTERVAL_LEVEL, 3);
        let mut t = TextTable::new([
            "Source".to_owned(),
            "Levels".into(),
            "Variance".into(),
            "Std Error".into(),
            format!("Lower {pct}%"),
            format!("Upper {pct}%"),
        ]);
        let opt = |v: Option<f64>| v.map_or_else(|| "—".to_owned(), |x| format_number(x, DIGITS));
        let mut push = |name: &str, levels: String, v: f64, se: Option<f64>, iv: Option<(f64, f64)>| {
            t.push([
                name.to_owned(),
                levels,
                format_number(v, DIGITS),
                opt(se),
                opt(iv.map(|i| i.0)),
                opt(iv.map(|i| i.1)),
            ]);
        };
        for c in &s.components {
            push(&c.source, c.levels.to_string(), c.summary.variance, c.std_error, c.interval);
        }
        let has_within = s.components.iter().any(|c| c.role == ComponentRole::WithinPart);
        let mut totals: Vec<(&str, &ExpandedTotal)> =
            vec![("Total gauge R&R", &s.gauge_rr), ("Reproducibility", &s.reproducibility)];
        if has_within {
            totals.push(("Within part", &s.within_part));
        }
        totals.extend([("Part-to-part", &s.part), ("Total variation", &s.total)]);
        for (name, v) in totals {
            push(name, "—".to_owned(), v.summary.variance, v.std_error, v.interval);
        }
        section(SectionKind::GaugeComponentIntervals, "Variance Component Intervals", SectionContent::Table(t))
    }

    fn fixed_table(&self, s: &ExpandedGaugeStudy) -> ReportSection {
        let mut t = TextTable::new(["Source", "Num DF", "Den DF", "F", "p-value"]);
        for f in &s.fixed_tests {
            t.push([
                f.source.clone(),
                f.num_df.to_string(),
                f.den_df.map_or_else(|| "—".to_owned(), |d| format_number(d, 4)),
                format_number(f.f_statistic, 4),
                crate::format::format_p_value(f.p_value),
            ]);
        }
        section(SectionKind::GaugeFixedEffects, "Tests of Fixed Factors", SectionContent::Table(t))
    }

    fn expanded_summary(&self, s: &ExpandedGaugeStudy) -> ReportSection {
        let d = &self.request.design;
        let mut t = TextTable::new(["Item", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Design", "expanded, fitted by REML".to_owned());
        row(&format!("Parts ({})", self.part_name), self.part_levels.len().to_string());
        row(&format!("Operators ({})", self.operator_name), self.operator_levels.len().to_string());
        row("Parts nested within operators", if d.parts_within_operators { "yes" } else { "no" }.to_owned());
        for ((name, levels), f) in self.factor_summaries.iter().zip(&d.factors) {
            let role = if f.random { "random" } else { "fixed" };
            let place = match f.nesting {
                GaugeFactorNesting::Crossed => "crossed",
                GaugeFactorNesting::WithinPart => "nested within parts",
                GaugeFactorNesting::WithinOperator => "nested within operators",
            };
            row(&format!("Factor {name}"), format!("{levels} levels, {role}, {place}"));
        }
        row("Two-factor interactions", if d.interactions { "included" } else { "not included" }.to_owned());
        row("Measurements used", s.n_obs.to_string());
        row("Number of distinct categories", s.distinct_categories.map_or_else(|| "—".to_owned(), |n| n.to_string()));
        if let Some(tol) = self.request.tolerance {
            row("Tolerance (USL − LSL)", format_number(tol, DIGITS));
        }
        row("−2 log restricted likelihood", format_number(s.minus_two_log_likelihood, DIGITS));
        if !s.converged {
            row("Convergence", "the optimizer did not converge".to_owned());
        }
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        for w in &s.warnings {
            row("Note", w.clone());
        }
        section(SectionKind::GaugeSummary, "Study Summary", SectionContent::Table(t))
    }

    /// Gauge run chart: one pane per part (in part order); within a pane the
    /// part's measurements in run order (the run-order column, else row
    /// order), coloured and shaped by operator, with a thin line at the
    /// part's mean. Repeatability shows as scatter within a pane,
    /// reproducibility as operators sitting apart, and drift as a trend across
    /// a pane.
    fn run_chart(&self) -> ReportSection {
        let n_parts = self.part_levels.len();
        let mut by_part: Vec<Vec<usize>> = vec![Vec::new(); n_parts];
        for (k, &(_, part, _, _)) in self.observations.iter().enumerate() {
            by_part[part].push(k);
        }
        for list in &mut by_part {
            list.sort_by(|&a, &b| {
                self.run_keys[a].total_cmp(&self.run_keys[b]).then(self.observations[a].0.cmp(&self.observations[b].0))
            });
        }
        let stride = by_part.iter().map(Vec::len).max().unwrap_or(0) as f64 + 1.0;
        let start = |p: usize| p as f64 * stride;
        let mut points = Vec::with_capacity(self.observations.len());
        let mut membership = Vec::with_capacity(self.observations.len());
        let mut segments = Vec::new();
        let mut panes = Vec::new();
        let mut x_ticks = Vec::new();
        for (p, list) in by_part.iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            for (j, &k) in list.iter().enumerate() {
                let (row, _, operator, y) = self.observations[k];
                points.push(RowPoint { row, x: start(p) + (j + 1) as f64, y });
                membership.push(operator);
            }
            let (lo, hi) = (start(p) + 0.5, start(p) + list.len() as f64 + 0.5);
            let mean = list.iter().map(|&k| self.observations[k].3).sum::<f64>() / list.len() as f64;
            segments.push(vec![[lo, mean], [hi, mean]]);
            panes.push(PlotPane { label: self.part_levels[p].clone(), x_min: lo, x_max: hi });
            x_ticks.push(((lo + hi) / 2.0, (p + 1).to_string()));
        }
        let order = match self.request.design.run_order {
            Some(_) => "run order",
            None => "row order",
        };
        let spec = PlotSpec {
            x_label: format!(
                "{} ({}); measurements in {order} within each",
                self.part_name,
                level_key(&self.part_levels)
            ),
            y_label: self.measurement_name.clone(),
            points,
            groups: Some(PointGroups {
                title: self.operator_name.clone(),
                labels: self.operator_levels.clone(),
                membership,
                lines: vec![Vec::new(); self.operator_levels.len()],
            }),
            x_ticks: Some(x_ticks),
            panes,
            segments,
            ..PlotSpec::default()
        };
        section(SectionKind::GaugeRunChartByPart, "Gauge Run Chart", SectionContent::Plot(Box::new(spec)))
    }

    /// Measurements by part or by operator. Points are grouped — colored and
    /// shaped — by operator in both.
    ///
    /// * **By part:** operators side by side within each part, with the part
    ///   numbers (1…p) labelled on the axis; one line joins the part means (each
    ///   the mean of all of that part's measurements).
    /// * **By operator:** a variability chart with one pane per operator. Within a
    ///   pane the parts sit at their part numbers (1…p, labelled on the axis), and
    ///   the operator's own line joins its part means (each the mean of that
    ///   part's replicates). Differences between the panes' lines show
    ///   reproducibility; scatter about each line shows repeatability. In a
    ///   nested layout each pane holds only that operator's own parts, numbered
    ///   1, 2, … within the pane.
    fn measurement_plot(&self, by_part: bool) -> ReportSection {
        let (n_parts, n_ops) = (self.part_levels.len(), self.operator_levels.len());
        // Running sums per (part, operator) cell and per part.
        let mut cell = vec![(0.0, 0usize); n_parts * n_ops];
        let mut per_part = vec![(0.0, 0usize); n_parts];
        for &(_, part, operator, y) in &self.observations {
            let c = &mut cell[part * n_ops + operator];
            c.0 += y;
            c.1 += 1;
            per_part[part].0 += y;
            per_part[part].1 += 1;
        }
        let mean = |(s, n): (f64, usize)| s / n.max(1) as f64;
        let membership: Vec<usize> = self.observations.iter().map(|&(_, _, operator, _)| operator).collect();
        let groups = |lines: Vec<Polyline>| PointGroups {
            title: self.operator_name.clone(),
            labels: self.operator_levels.clone(),
            membership: membership.clone(),
            lines,
        };

        let spec = if by_part {
            // Operators spread across a 0.5-wide band around each part number.
            let offset = |o: usize| if n_ops <= 1 { 0.0 } else { -0.25 + 0.5 * o as f64 / (n_ops - 1) as f64 };
            let points = self
                .observations
                .iter()
                .map(|&(row, part, operator, y)| RowPoint { row, x: (part + 1) as f64 + offset(operator), y })
                .collect();
            PlotSpec {
                x_time: None,
                y_time: None,
                x_label: format!(
                    "{} ({}); {}s side by side",
                    self.part_name,
                    level_key(&self.part_levels),
                    self.operator_name
                ),
                y_label: self.measurement_name.clone(),
                points,
                line: Some((0..n_parts).map(|p| [(p + 1) as f64, mean(per_part[p])]).collect()),
                line_name: Some(format!("{} means", self.part_name)),
                band: None,
                groups: Some(groups(vec![Vec::new(); n_ops])),
                x_ticks: Some((1..=n_parts).map(|p| (p as f64, p.to_string())).collect()),
                panes: Vec::new(),
                annotations: None,
                segments: Vec::new(),
                ..PlotSpec::default()
            }
        } else {
            // Parts shown in each operator's pane, in part order: all parts for
            // a crossed layout, the operator's own parts for a nested one.
            let pane_parts: Vec<Vec<usize>> = (0..n_ops)
                .map(|o| (0..n_parts).filter(|&p| !self.nested_parts || cell[p * n_ops + o].1 > 0).collect())
                .collect();
            let width = pane_parts.iter().map(Vec::len).max().unwrap_or(0);
            // Pane `o` holds positions o·(w+1) + 1 … o·(w+1) + w; the one-unit gap
            // between panes is where the divider is drawn.
            let stride = (width + 1) as f64;
            let pos = |part: usize, operator: usize| {
                pane_parts[operator].iter().position(|&q| q == part).map(|j| operator as f64 * stride + (j + 1) as f64)
            };
            let points = self
                .observations
                .iter()
                .filter_map(|&(row, part, operator, y)| pos(part, operator).map(|x| RowPoint { row, x, y }))
                .collect();
            let lines = (0..n_ops)
                .map(|o| {
                    pane_parts[o]
                        .iter()
                        .filter(|&&p| cell[p * n_ops + o].1 > 0)
                        .filter_map(|&p| pos(p, o).map(|x| [x, mean(cell[p * n_ops + o])]))
                        .collect()
                })
                .collect();
            let x_ticks = (0..n_ops)
                .flat_map(|o| {
                    pane_parts[o].iter().enumerate().map(move |(j, &p)| {
                        let label = if self.nested_parts { (j + 1).to_string() } else { (p + 1).to_string() };
                        (o as f64 * stride + (j + 1) as f64, label)
                    })
                })
                .collect();
            let panes = self
                .operator_levels
                .iter()
                .enumerate()
                .map(|(o, label)| PlotPane {
                    label: format!("{} {label}", self.operator_name),
                    x_min: o as f64 * stride + 0.5,
                    x_max: o as f64 * stride + pane_parts[o].len().max(1) as f64 + 0.5,
                })
                .collect();
            let x_label = if self.nested_parts {
                format!("{} number within each {} pane (that operator's own parts)", self.part_name, self.operator_name)
            } else {
                format!(
                    "{} number within each {} pane ({})",
                    self.part_name,
                    self.operator_name,
                    level_key(&self.part_levels)
                )
            };
            PlotSpec {
                x_time: None,
                y_time: None,
                x_label,
                y_label: self.measurement_name.clone(),
                points,
                line: None,
                band: None,
                groups: Some(groups(lines)),
                x_ticks: Some(x_ticks),
                panes,
                annotations: None,
                segments: Vec::new(),
                ..PlotSpec::default()
            }
        };
        let (kind, title) = if by_part {
            (SectionKind::MeasurementsByPart, "Measurements by Part")
        } else {
            (SectionKind::MeasurementsByOperator, "Measurements by Operator")
        };
        section(kind, title, SectionContent::Plot(Box::new(spec)))
    }
}

/// "1 = first … n = last" for a list of levels.
fn level_key(levels: &[String]) -> String {
    match (levels.first(), levels.last()) {
        (Some(a), Some(b)) if levels.len() > 1 => format!("1 = {a} … {} = {b}", levels.len()),
        (Some(a), _) => format!("1 = {a}"),
        _ => String::new(),
    }
}

fn anova_row(r: &GaugeAnovaRow, part: &str, operator: &str) -> Vec<String> {
    let source = match r.source {
        GaugeSource::Part => format!("Part ({part})"),
        GaugeSource::Operator => format!("Operator ({operator})"),
        GaugeSource::Interaction => "Part × operator".to_owned(),
        GaugeSource::PartWithinOperator => format!("Part ({part}) within operator"),
        GaugeSource::Repeatability => "Repeatability".to_owned(),
        GaugeSource::Total => "Total".to_owned(),
    };
    vec![
        source,
        r.df.to_string(),
        format_number(r.sum_of_squares, DIGITS),
        r.mean_square.map_or_else(String::new, |v| format_number(v, DIGITS)),
        r.f_statistic.map_or_else(String::new, |v| format_number(v, 4)),
        r.p_value.map_or_else(String::new, crate::format::format_p_value),
    ]
}

fn percent(v: f64) -> String {
    if v.is_finite() { format!("{v:.2}") } else { "—".to_owned() }
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    fn request(t: &DataTable) -> GaugeStudyRequest {
        GaugeStudyRequest::new(t.find("Part").unwrap(), t.find("Operator").unwrap(), t.find("Measurement").unwrap())
    }

    #[test]
    fn sample_study_runs_and_reports() {
        let t = sample::gauge_study();
        let states = RowStateManager::new(t.n_rows());
        let a = GaugeAnalysis::run(&t, &states, &request(&t)).unwrap();
        assert_eq!((a.study().unwrap().parts, a.study().unwrap().operators, a.study().unwrap().replicates), (10, 3, 3));
        assert_eq!(a.title(), "Gauge R&R: Measurement");
        let s = a.sections(&GaugeReportOptions::default());
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::VarianceComponents,
                SectionKind::GaugeSummary,
                SectionKind::GaugeAnova,
                SectionKind::MeasurementsByPart
            ]
        );
        let SectionContent::Table(vc) = &s[0].content else { panic!() };
        assert_eq!(vc.rows.len(), 7);
        assert_eq!(vc.cell(6, "% Contribution"), Some("100.00"));
        assert_eq!(vc.cell(6, "% Study Var"), Some("100.00"));
        // The simulated gauge is decent but not perfect.
        let grr: f64 = vc.cell(0, "% Study Var").unwrap().parse().unwrap();
        assert!((5.0..60.0).contains(&grr), "{grr}");
        let SectionContent::Plot(p) = &s[3].content else { panic!() };
        assert_eq!(p.points.len(), 90);
        let g = p.groups.as_ref().expect("points are grouped by operator");
        assert_eq!(g.labels, vec!["A", "B", "C"]);
        assert_eq!(g.membership.len(), 90);
        assert!(g.lines.iter().all(Vec::is_empty), "no per-operator lines by part");
        // One line through the 10 part means, at the part positions.
        let ticks = p.x_ticks.as_ref().expect("part-number ticks");
        assert_eq!(ticks.len(), 10);
        assert_eq!(ticks[0], (1.0, "1".to_owned()));
        assert_eq!(ticks[9], (10.0, "10".to_owned()));
        assert!(p.panes.is_empty());
        let line = p.line.as_ref().expect("a line through the part means");
        assert_eq!(line.len(), 10);
        let t_meas = t.column(t.find("Measurement").unwrap()).unwrap().as_numeric().unwrap();
        let part_of = |r: usize| t.column(0).unwrap().level_text(r);
        let p01: Vec<f64> = (0..90).filter(|&r| part_of(r) == "P01").map(|r| t_meas[r]).collect();
        assert_eq!(p01.len(), 9, "3 operators × 3 replicates");
        assert_eq!(line[0][0], 1.0);
        assert!((line[0][1] - p01.iter().sum::<f64>() / 9.0).abs() < 1e-12);
        // Group membership matches each point's operator.
        for (pt, &m) in p.points.iter().zip(&g.membership) {
            assert_eq!(t.column(1).unwrap().level_text(pt.row), g.labels[m]);
        }
    }

    #[test]
    fn levels_follow_first_appearance_and_points_carry_rows() {
        let t = sample::gauge_study();
        let a = GaugeAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &request(&t)).unwrap();
        assert_eq!(a.part_levels[0], "P01");
        assert_eq!(a.operator_levels, vec!["A", "B", "C"]);
        let s = a.sections(&GaugeReportOptions { show_operator_plot: true, ..Default::default() });
        let SectionContent::Plot(by_op) = &s.last().unwrap().content else { panic!() };
        assert_eq!(s.last().unwrap().kind, SectionKind::MeasurementsByOperator);
        // One pane per operator, parts 1..10 inside each, separated by a gap.
        assert!(by_op.line.is_none());
        assert_eq!(by_op.panes.len(), 3);
        assert_eq!(by_op.panes[1].label, "Operator B");
        assert!(by_op.panes.windows(2).all(|w| w[0].x_max < w[1].x_min), "panes do not overlap");
        let ticks = by_op.x_ticks.as_ref().expect("part-number ticks");
        assert_eq!(ticks.len(), 30);
        assert_eq!(ticks[0], (1.0, "1".to_owned()));
        assert_eq!(ticks[10], (12.0, "1".to_owned()), "second pane starts after a one-unit gap");
        // Every point sits on its pane's tick for its part.
        let part_col = t.column(t.find("Part").unwrap()).unwrap();
        for p in &by_op.points {
            let op = t.column(t.find("Operator").unwrap()).unwrap().level_text(p.row);
            let o = a.operator_levels.iter().position(|l| *l == op).unwrap();
            let part = a.part_levels.iter().position(|l| *l == part_col.level_text(p.row)).unwrap();
            assert_eq!(p.x, (o * 11 + part + 1) as f64);
            let pane = &by_op.panes[o];
            assert!(pane.x_min < p.x && p.x < pane.x_max);
        }
        // Variability-chart lines: one per operator through its 10 part means.
        let g = by_op.groups.as_ref().unwrap();
        assert_eq!(g.lines.len(), 3);
        let meas = t.column(t.find("Measurement").unwrap()).unwrap().as_numeric().unwrap();
        for (o, op) in ["A", "B", "C"].iter().enumerate() {
            let line = &g.lines[o];
            assert_eq!(line.len(), 10);
            assert_eq!(line[6][0], (o * 11 + 7) as f64, "P07 position in pane {op}");
            let reps: Vec<f64> = (0..90)
                .filter(|&r| part_col.level_text(r) == "P07" && t.column(1).unwrap().level_text(r) == *op)
                .map(|r| meas[r])
                .collect();
            assert_eq!(reps.len(), 3);
            assert!((line[6][1] - reps.iter().sum::<f64>() / 3.0).abs() < 1e-12, "P07 mean for operator {op}");
        }
    }

    #[test]
    fn tolerance_column_and_pooling() {
        let t = sample::gauge_study();
        let states = RowStateManager::new(t.n_rows());
        let req = GaugeStudyRequest { tolerance: Some(1.0), pool_interaction: true, ..request(&t) };
        let a = GaugeAnalysis::run(&t, &states, &req).unwrap();
        let s = a.sections(&GaugeReportOptions::default());
        let SectionContent::Table(vc) = &s[0].content else { panic!() };
        assert!(vc.columns.contains(&"% Tolerance".to_owned()));
        let SectionContent::Table(summary) = &s[1].content else { panic!() };
        let pooled = a.study().unwrap().interaction_pooled;
        assert_eq!(summary.rows.iter().any(|r| r[1].starts_with("pooled")), pooled);
    }

    #[test]
    fn exclusion_unbalances_with_a_helpful_message() {
        let t = sample::gauge_study();
        let mut states = RowStateManager::new(t.n_rows());
        let a = GaugeAnalysis::run(&t, &states, &request(&t)).unwrap();
        states.set_excluded([5], true);
        assert!(a.is_stale(&states));
        let err = GaugeAnalysis::run(&t, &states, &request(&t)).unwrap_err().to_string();
        assert!(err.contains("balanced") && err.contains("excluded rows"), "{err}");
    }

    #[test]
    fn rejects_bad_requests() {
        let t = sample::gauge_study();
        let s = RowStateManager::new(t.n_rows());
        let r = request(&t);
        assert!(GaugeAnalysis::run(&t, &s, &GaugeStudyRequest { operator: r.part, ..r.clone() }).is_err());
        assert!(
            GaugeAnalysis::run(&t, &s, &GaugeStudyRequest { measurement: r.part, part: r.measurement, ..r }).is_err()
        );
    }

    fn weld() -> (DataTable, GaugeStudyRequest) {
        let t = sample::weld_pull_strength();
        let mut r =
            GaugeStudyRequest::new(t.find("Part").unwrap(), t.find("Operator").unwrap(), t.find("Strength").unwrap());
        r.design = GaugeStudyDesign::nested();
        (t, r)
    }

    /// Batches numbered 1–5 within each operator are 15 different parts in a
    /// nested study, and the report drops the interaction.
    #[test]
    fn nested_study_identifies_parts_within_operators() {
        let (t, r) = weld();
        let a = GaugeAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &r).unwrap();
        assert_eq!(a.title(), "Nested gauge R&R: Strength");
        assert_eq!(a.part_levels.len(), 15);
        assert_eq!(a.part_levels[0], "1 (A)");
        let s = a.sections(&GaugeReportOptions { show_operator_plot: true, ..Default::default() });
        let SectionContent::Table(vc) = &s[0].content else { panic!() };
        assert_eq!(vc.rows.len(), 6);
        assert!(vc.rows.iter().all(|row| !row[0].contains('×')));
        let SectionContent::Table(anova) = &s[2].content else { panic!() };
        assert_eq!(anova.rows[1][0], "Part (Part) within operator");
        assert_eq!(anova.rows[1][1], "12");
        // Each operator's pane holds only their five batches, numbered 1–5.
        let SectionContent::Plot(by_op) = &s.last().unwrap().content else { panic!() };
        assert_eq!(by_op.x_ticks.as_ref().unwrap().len(), 15);
        assert_eq!(by_op.x_ticks.as_ref().unwrap()[5], (7.0, "1".to_owned()));
        assert!(by_op.groups.as_ref().unwrap().lines.iter().all(|l| l.len() == 5));
        // A lost coupon unbalances the nested ANOVA; the message points to the expanded study.
        let mut states = RowStateManager::new(t.n_rows());
        states.set_excluded([0], true);
        let e = GaugeAnalysis::run(&t, &states, &r).unwrap_err().to_string();
        assert!(e.contains("balanced") && e.contains("expanded study type"), "{e}");
    }

    /// The expanded study with parts within operators reproduces the
    /// balanced nested ANOVA estimates (all positive here), and handles a
    /// dropped row that the ANOVA method rejects.
    #[test]
    fn expanded_nested_matches_anova_and_allows_missing_rows() {
        let (t, r) = weld();
        let states = RowStateManager::new(t.n_rows());
        let nested = GaugeAnalysis::run(&t, &states, &r).unwrap();
        let mut er = r.clone();
        er.design =
            GaugeStudyDesign { kind: GaugeStudyKind::Expanded, parts_within_operators: true, ..Default::default() };
        let ex = GaugeAnalysis::run(&t, &states, &er).unwrap();
        let (a, b) = (&nested.study().unwrap().components, ex.expanded().unwrap());
        assert!(a.operator.variance > 0.0 && a.part.variance > 0.0);
        assert!((b.part.summary.variance - a.part.variance).abs() < 1e-4 * a.part.variance);
        assert!((b.gauge_rr.summary.variance - a.gauge_rr.variance).abs() < 1e-4 * a.gauge_rr.variance);
        let mut dropped = RowStateManager::new(t.n_rows());
        dropped.set_excluded([4], true);
        assert!(GaugeAnalysis::run(&t, &dropped, &r).is_err());
        let e = GaugeAnalysis::run(&t, &dropped, &er).unwrap();
        assert_eq!(e.expanded().unwrap().n_obs, 44);
        let s = e.sections(&GaugeReportOptions::default());
        let summary = s.iter().find(|x| x.kind == SectionKind::GaugeSummary).unwrap();
        let SectionContent::Table(summary) = &summary.content else { panic!() };
        assert!(summary.rows.iter().any(|row| row[0] == "Rows excluded" && row[1] == "1"));
    }

    #[test]
    fn expanded_fixed_factor_is_tested() {
        let t = sample::bore_diameter();
        let mut r =
            GaugeStudyRequest::new(t.find("Part").unwrap(), t.find("Operator").unwrap(), t.find("Diameter").unwrap());
        r.design = GaugeStudyDesign {
            kind: GaugeStudyKind::Expanded,
            factors: vec![crate::GaugeExtraFactor {
                column: t.find("Gauge").unwrap(),
                random: false,
                nesting: GaugeFactorNesting::Crossed,
            }],
            ..Default::default()
        };
        let a = GaugeAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &r).unwrap();
        let s = a.sections(&GaugeReportOptions::default());
        let fixed = s.iter().find(|x| x.kind == SectionKind::GaugeFixedEffects).expect("fixed-factor tests");
        let SectionContent::Table(ft) = &fixed.content else { panic!() };
        assert_eq!(ft.rows.len(), 1);
        assert_eq!(ft.rows[0][0], "Gauge");
        let SectionContent::Table(iv) = &s[1].content else { panic!() };
        assert!(iv.rows.iter().all(|row| row[0] != "Gauge"), "a fixed factor has no variance component");
        assert!(iv.columns.contains(&"Lower 95%".to_owned()));
        // The same factor nested in a random one is refused.
        r.design.factors[0].nesting = GaugeFactorNesting::WithinOperator;
        assert!(GaugeAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &r).is_err());
    }

    /// A random factor nested within parts is product variation: listed
    /// under part-to-part, not in gauge R&R.
    #[test]
    fn within_part_factor_is_product_variation() {
        let (mut y, mut p, mut l, mut o) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for part in 0..5_usize {
            for loc in 0..3_usize {
                for op in 0..2_usize {
                    for rep in 0..2_usize {
                        p.push(format!("P{part}"));
                        l.push(format!("L{loc}"));
                        o.push(format!("O{op}"));
                        let noise = ((part * 13 + loc * 7 + op * 3 + rep * 5) as f64).sin() * 0.2;
                        let shape = [0.0, 0.9, -0.6, 0.4, 0.7][part] * loc as f64;
                        y.push(10.0 + 2.0 * part as f64 + shape + 0.3 * op as f64 + noise);
                    }
                }
            }
        }
        let t = DataTable::new(
            "loc",
            vec![
                stat_data::Column::text("Part", p),
                stat_data::Column::text("Operator", o),
                stat_data::Column::text("Location", l),
                stat_data::Column::numeric("y", y),
            ],
        )
        .unwrap();
        let mut r = GaugeStudyRequest::new(0, 1, 3);
        r.design = GaugeStudyDesign {
            kind: GaugeStudyKind::Expanded,
            factors: vec![crate::GaugeExtraFactor { column: 2, random: true, nesting: GaugeFactorNesting::WithinPart }],
            ..Default::default()
        };
        let a = GaugeAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &r).unwrap();
        let e = a.expanded().unwrap();
        let s = a.sections(&GaugeReportOptions::default());
        let SectionContent::Table(vc) = &s[0].content else { panic!() };
        let names: Vec<&str> = vc.rows.iter().map(|r| r[0].as_str()).collect();
        let at = |n: &str| names.iter().position(|x| *x == n).unwrap_or_else(|| panic!("no {n} in {names:?}"));
        assert!(at("Part-to-part") < at("  Within part") && at("  Within part") < at("    Location(Part)"));
        assert!(at("  Between parts") < at("  Within part"));
        assert!(at("    Location(Part)") < at("Total variation"));
        assert!(at("  Reproducibility") < at("    Operator × Location(Part)"));
        assert!(at("    Operator × Location(Part)") < at("Part-to-part"));
        let within = e.components.iter().find(|c| c.source == "Location(Part)").unwrap().summary.variance;
        assert!(within > 0.0);
        assert!((e.within_part.summary.variance - within).abs() < 1e-12);
        let SectionContent::Table(iv) = &s[1].content else { panic!() };
        assert!(iv.rows.iter().any(|row| row[0] == "Within part"));
    }

    /// Run chart: one pane per part, the part's readings in run order inside it.
    #[test]
    fn run_chart_orders_each_part_by_run() {
        let t = sample::bore_diameter();
        let mut r =
            GaugeStudyRequest::new(t.find("Part").unwrap(), t.find("Operator").unwrap(), t.find("Diameter").unwrap());
        r.design.run_order = Some(t.find("Run").unwrap());
        // Crossed but unbalanced: use the expanded fit.
        r.design.kind = GaugeStudyKind::Expanded;
        let a = GaugeAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &r).unwrap();
        let s = a.sections(&GaugeReportOptions { show_run_chart: true, show_part_plot: false, ..Default::default() });
        let chart = s.last().unwrap();
        assert_eq!(chart.kind, SectionKind::GaugeRunChartByPart);
        let SectionContent::Plot(p) = &chart.content else { panic!() };
        assert_eq!(p.panes.len(), 8);
        assert_eq!(p.segments.len(), 8, "a mean line per part");
        assert_eq!(p.points.len(), 89);
        let run = t.column(t.find("Run").unwrap()).unwrap().as_numeric().unwrap();
        let part = t.column(t.find("Part").unwrap()).unwrap();
        for w in p.points.windows(2) {
            if part.level_text(w[0].row) == part.level_text(w[1].row) {
                assert!(w[0].x < w[1].x && run[w[0].row] < run[w[1].row]);
            }
        }
        let g = p.groups.as_ref().unwrap();
        assert_eq!(g.labels, ["Dana", "Eli", "Fay"]);
    }
}
