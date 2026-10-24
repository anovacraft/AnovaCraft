//! Proportion and event-rate comparisons bound to a dataset
//! ([`stat_inference::proportions`], [`stat_inference::rates`]).
//!
//! A proportion analysis counts the rows whose outcome equals a chosen event
//! level, either in one sample (against a hypothesized proportion), by the
//! levels of a grouping column, or for paired outcomes in two columns
//! (McNemar). A rate analysis sums an event-count column (and optionally an
//! exposure column) overall or by group.

/// Largest count accepted: every integer up to 2⁵³ is exact in a `f64`.
const MAX_EXACT_COUNT: f64 = 9_007_199_254_740_992.0;

use crate::compare::{DIGITS, invalid, push_test, section, test_columns};
use crate::format::{format_number, format_p_value};
use crate::report::{Annotations, PlotSpec, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_data::{DataTable, RowStateManager};
use stat_discrete::{ContingencyTable, test_independence};
use stat_inference::Alternative;
use stat_inference::proportions::{exact_binomial_test, mcnemar_test, score_test, two_proportions};
use stat_inference::rates::{one_rate_test, rate_ratio_test};
use stat_linear::natural_order;

/// What the proportions are compared with.
#[derive(Debug, Clone, PartialEq)]
pub enum ProportionComparison {
    /// One sample against a hypothesized proportion.
    OneSample {
        /// `p₀`.
        null: f64,
    },
    /// Independent groups given by a column.
    Groups {
        /// Grouping column.
        group: usize,
    },
    /// Paired outcomes: a second outcome column on the same rows.
    Paired {
        /// Second outcome column.
        second: usize,
    },
}

/// Settings of a proportion analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct ProportionRequest {
    /// Outcome column (any type).
    pub outcome: usize,
    /// The outcome value counted as an event; `None` takes the last level in
    /// natural order (e.g. "1" of 0/1, "yes" of no/yes).
    pub event: Option<String>,
    /// Comparison.
    pub comparison: ProportionComparison,
    /// Alternative hypothesis.
    pub alternative: Alternative,
    /// Confidence level.
    pub level: f64,
}

impl ProportionRequest {
    /// Default settings.
    #[must_use]
    pub fn new(outcome: usize, comparison: ProportionComparison) -> Self {
        Self { outcome, event: None, comparison, alternative: Alternative::TwoSided, level: 0.95 }
    }

    /// Every column used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match self.comparison {
            ProportionComparison::OneSample { .. } => vec![self.outcome],
            ProportionComparison::Groups { group } => vec![self.outcome, group],
            ProportionComparison::Paired { second } => vec![self.outcome, second],
        }
    }
}

/// Distinct non-empty labels of a column in natural order.
#[must_use]
pub fn column_levels(table: &DataTable, column: usize) -> Vec<String> {
    let Some(c) = table.column(column) else { return Vec::new() };
    let mut levels: Vec<String> = Vec::new();
    for r in 0..table.n_rows() {
        let t = c.level_text(r);
        if !t.is_empty() && !levels.contains(&t) {
            levels.push(t);
        }
    }
    levels.sort_by(|a, b| natural_order(a, b));
    levels
}

/// A proportion analysis.
#[derive(Debug, Clone)]
pub struct ProportionAnalysis {
    request: ProportionRequest,
    outcome_name: String,
    event: String,
    /// `(label, events, trials)` per group (one entry for one sample).
    counts: Vec<(String, u64, u64)>,
    group_name: Option<String>,
    /// Paired 2×2 table `[n11, n12, n21, n22]` and the second column's name.
    paired: Option<([u64; 4], String)>,
}

impl ProportionAnalysis {
    /// Count events.
    ///
    /// # Errors
    /// Invalid columns, an event level that never occurs, or no usable rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ProportionRequest) -> Result<Self> {
        let col = |i: usize| table.column(i).ok_or_else(|| invalid("no such column"));
        let outcome = col(request.outcome)?;
        let levels = column_levels(table, request.outcome);
        let event = match &request.event {
            Some(e) => e.clone(),
            None => levels.last().cloned().ok_or_else(|| invalid("the outcome column is empty"))?,
        };
        let usable = |r: usize| !states.is_excluded(r) && !outcome.level_text(r).is_empty();
        let mut counts: Vec<(String, u64, u64)> = Vec::new();
        let mut group_name = None;
        let mut paired = None;
        match request.comparison {
            ProportionComparison::OneSample { .. } => {
                let (mut x, mut n) = (0, 0);
                for r in (0..table.n_rows()).filter(|&r| usable(r)) {
                    n += 1;
                    x += u64::from(outcome.level_text(r) == event);
                }
                counts.push((String::new(), x, n));
            }
            ProportionComparison::Groups { group } => {
                if group == request.outcome {
                    return Err(invalid("the grouping column must differ from the outcome column"));
                }
                let g = col(group)?;
                group_name = Some(g.name.clone());
                for r in (0..table.n_rows()).filter(|&r| usable(r)) {
                    let label = g.level_text(r);
                    if label.is_empty() {
                        continue;
                    }
                    let i = counts.iter().position(|c| c.0 == label).unwrap_or_else(|| {
                        counts.push((label, 0, 0));
                        counts.len() - 1
                    });
                    counts[i].2 += 1;
                    counts[i].1 += u64::from(outcome.level_text(r) == event);
                }
                counts.sort_by(|a, b| natural_order(&a.0, &b.0));
            }
            ProportionComparison::Paired { second } => {
                if second == request.outcome {
                    return Err(invalid("choose two different outcome columns"));
                }
                let s = col(second)?;
                let mut cells = [0u64; 4];
                for r in (0..table.n_rows()).filter(|&r| usable(r)) {
                    let b = s.level_text(r);
                    if b.is_empty() {
                        continue;
                    }
                    let (x, y) = (outcome.level_text(r) == event, b == event);
                    cells[match (x, y) {
                        (true, true) => 0,
                        (true, false) => 1,
                        (false, true) => 2,
                        (false, false) => 3,
                    }] += 1;
                }
                counts.push((outcome.name.clone(), cells[0] + cells[1], cells.iter().sum()));
                counts.push((s.name.clone(), cells[0] + cells[2], cells.iter().sum()));
                paired = Some((cells, s.name.clone()));
            }
        }
        if counts.iter().all(|c| c.2 == 0) {
            return Err(invalid("no rows with an outcome"));
        }
        counts.retain(|c| c.2 > 0);
        Ok(Self { request: request.clone(), outcome_name: outcome.name.clone(), event, counts, group_name, paired })
    }

    /// `(label, events, trials)` per group.
    #[must_use]
    pub fn counts(&self) -> &[(String, u64, u64)] {
        &self.counts
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let what = format!("{} = {}", self.outcome_name, self.event);
        match (&self.group_name, &self.paired) {
            (Some(g), _) => format!("Proportions: {what} by {g}"),
            (None, Some((_, second))) => format!("Paired Proportions: {what} in {} and {second}", self.outcome_name),
            _ => format!("Proportion: {what}"),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let pct = format_number(100.0 * r.level, 4);
        let heading = self
            .group_name
            .clone()
            .unwrap_or_else(|| if self.paired.is_some() { "Column".to_owned() } else { String::new() });
        let mut summary = TextTable::new([
            heading.clone(),
            "Events".to_owned(),
            "N".to_owned(),
            "Proportion".to_owned(),
            format!("Lower {pct}% (exact)"),
            format!("Upper {pct}% (exact)"),
        ]);
        let mut intervals = Vec::new();
        for (label, x, n) in &self.counts {
            let ci = exact_binomial_test(*x, *n, 0.5, r.level, Alternative::TwoSided).map(|t| t.confidence_interval);
            let (lo, hi) = ci.unwrap_or((f64::NAN, f64::NAN));
            let p = *x as f64 / *n as f64;
            intervals.push((p, lo, hi));
            summary.push([
                if label.is_empty() { "All rows".to_owned() } else { label.clone() },
                x.to_string(),
                n.to_string(),
                format_number(p, DIGITS),
                format_number(lo, DIGITS),
                format_number(hi, DIGITS),
            ]);
        }
        let mut out = vec![section(
            SectionKind::ProportionSummary,
            format!("Proportion of {} = {}", self.outcome_name, self.event),
            SectionContent::Table(summary),
        )];
        match (&r.comparison, &self.paired) {
            (ProportionComparison::OneSample { null }, _) => {
                let (_, x, n) = self.counts[0];
                let mut t = TextTable::new(test_columns(None));
                push_test(&mut t, None, "Exact binomial", exact_binomial_test(x, n, *null, r.level, r.alternative));
                push_test(&mut t, None, "Score z", score_test(x, n, *null, r.level, r.alternative));
                out.push(section(
                    SectionKind::ProportionTests,
                    format!("Tests (H₁: proportion {} {})", r.alternative.symbol(), format_number(*null, DIGITS)),
                    SectionContent::Table(t),
                ));
            }
            (_, Some((cells, second))) => out.extend(self.paired_sections(cells, second)),
            _ if self.counts.len() == 2 => out.extend(self.two_groups()),
            _ => out.push(self.homogeneity()),
        }
        out.push(proportion_plot(
            &self.counts,
            &intervals,
            &heading,
            &format!("Proportion {} = {}", self.outcome_name, self.event),
            "Proportion",
        ));
        out
    }

    fn two_groups(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let ((l1, x1, n1), (l2, x2, n2)) = (&self.counts[0], &self.counts[1]);
        let mut out = Vec::new();
        match two_proportions(*x1, *n1, *x2, *n2, r.level, r.alternative) {
            Ok(t) => {
                let mut table = TextTable::new(test_columns(None));
                table.push(crate::compare::test_row(None, &t.z_test));
                table.push(crate::compare::test_row(None, &t.fisher));
                out.push(section(
                    SectionKind::ProportionTests,
                    format!("Tests (H₁: p({l1}) − p({l2}) {} 0; Fisher estimate: odds ratio)", r.alternative.symbol()),
                    SectionContent::Table(table),
                ));
                let pct = format_number(100.0 * r.level, 4);
                let mut m = TextTable::new([
                    "Measure".to_owned(),
                    "Estimate".to_owned(),
                    format!("Lower {pct}%"),
                    format!("Upper {pct}%"),
                ]);
                m.push([
                    format!("Difference p({l1}) − p({l2})"),
                    format_number(t.z_test.estimate, DIGITS),
                    format_number(t.z_test.confidence_interval.0, DIGITS),
                    format_number(t.z_test.confidence_interval.1, DIGITS),
                ]);
                for (name, e) in [("Relative risk", t.relative_risk), ("Odds ratio", t.odds_ratio)] {
                    m.push([
                        format!("{name} ({l1} : {l2})"),
                        format_number(e.estimate, DIGITS),
                        format_number(e.lower, DIGITS),
                        format_number(e.upper, DIGITS),
                    ]);
                }
                out.push(section(
                    SectionKind::EffectSizes,
                    "Measures of Association (two-sided limits)",
                    SectionContent::Table(m),
                ));
            }
            Err(e) => out.push(section(
                SectionKind::ProportionTests,
                "Tests",
                SectionContent::Note(format!("Not available: {e}")),
            )),
        }
        out
    }

    fn homogeneity(&self) -> ReportSection {
        let mut rows = Vec::new();
        let mut cols = Vec::new();
        for (label, x, n) in &self.counts {
            for i in 0..*n {
                rows.push(label.clone());
                cols.push(if i < *x { "event" } else { "no event" }.to_owned());
            }
        }
        let mut t = TextTable::new(["Test", "χ²", "DF", "p-value"]);
        match ContingencyTable::from_pairs(&rows, &cols).and_then(|c| test_independence(&c)) {
            Ok(i) => {
                t.push([
                    "Pearson χ² (equal proportions)".to_owned(),
                    format_number(i.pearson_chi_square, DIGITS),
                    i.df.to_string(),
                    format_p_value(i.pearson_p_value),
                ]);
                t.push([
                    "Likelihood ratio G²".to_owned(),
                    format_number(i.likelihood_ratio, DIGITS),
                    i.df.to_string(),
                    format_p_value(i.likelihood_ratio_p_value),
                ]);
            }
            Err(e) => {
                t.push(["Equal proportions".to_owned(), format!("not available: {e}"), String::new(), String::new()])
            }
        }
        section(SectionKind::ProportionTests, "Test of Equal Proportions (all groups)", SectionContent::Table(t))
    }

    fn paired_sections(&self, cells: &[u64; 4], second: &str) -> Vec<ReportSection> {
        let first = &self.outcome_name;
        let mut table =
            TextTable::new([String::new(), format!("{second} = {}", self.event), format!("{second} ≠ {}", self.event)]);
        table.push([format!("{first} = {}", self.event), cells[0].to_string(), cells[1].to_string()]);
        table.push([format!("{first} ≠ {}", self.event), cells[2].to_string(), cells[3].to_string()]);
        let mut out = vec![section(SectionKind::ContingencyCounts, "Paired Outcomes", SectionContent::Table(table))];
        let mut t = TextTable::new(test_columns(None));
        match mcnemar_test(cells[0], cells[1], cells[2], cells[3], self.request.level) {
            Ok(m) => {
                t.push(crate::compare::test_row(None, &m.chi_square));
                t.push(crate::compare::test_row(None, &m.exact));
            }
            Err(e) => t.push(crate::compare::error_row(None, "McNemar", &e)),
        }
        out.push(section(
            SectionKind::ProportionTests,
            format!("McNemar Test (equal marginal proportions; estimate: p({first}) − p({second}), Wald limits)"),
            SectionContent::Table(t),
        ));
        out
    }
}

fn proportion_plot(
    counts: &[(String, u64, u64)],
    intervals: &[(f64, f64, f64)],
    heading: &str,
    y_label: &str,
    estimate_name: &str,
) -> ReportSection {
    let labels: Vec<String> =
        counts.iter().map(|c| if c.0.is_empty() { "All rows".to_owned() } else { c.0.clone() }).collect();
    let items =
        intervals.iter().enumerate().map(|(g, &(p, _, _))| ((g + 1) as f64, p, format_number(p, 3), 0)).collect();
    let segments = intervals
        .iter()
        .enumerate()
        .filter(|(_, (_, lo, hi))| lo.is_finite() && hi.is_finite())
        .map(|(g, &(_, lo, hi))| vec![[(g + 1) as f64, lo], [(g + 1) as f64, hi]])
        .collect();
    let spec = PlotSpec {
        x_time: None,
        y_time: None,
        x_label: heading.to_owned(),
        y_label: y_label.to_owned(),
        points: Vec::new(),
        line: None,
        band: None,
        groups: None,
        x_ticks: Some(labels.iter().enumerate().map(|(g, l)| ((g + 1) as f64, l.clone())).collect()),
        panes: Vec::new(),
        annotations: Some(Annotations { groups: vec![estimate_name.to_owned()], items, labels: Vec::new() }),
        segments,
        ..PlotSpec::default()
    };
    section(
        SectionKind::ProportionPlot,
        "Proportions with Exact Confidence Intervals",
        SectionContent::Plot(Box::new(spec)),
    )
}

// ─────────────────────────────── Rates ───────────────────────────────

/// Settings of an event-rate analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct RateRequest {
    /// Event counts per row (non-negative integers).
    pub count: usize,
    /// Exposure per row (time, area, units); 1 per row when absent.
    pub exposure: Option<usize>,
    /// Optional grouping column.
    pub group: Option<usize>,
    /// Hypothesized rate (one group).
    pub null_rate: f64,
    /// Alternative hypothesis.
    pub alternative: Alternative,
    /// Confidence level.
    pub level: f64,
}

impl RateRequest {
    /// Default settings.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self { count, exposure: None, group: None, null_rate: 1.0, alternative: Alternative::TwoSided, level: 0.95 }
    }

    /// Every column used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.count).chain(self.exposure).chain(self.group).collect()
    }
}

/// An event-rate analysis.
#[derive(Debug, Clone)]
pub struct RateAnalysis {
    request: RateRequest,
    count_name: String,
    exposure_name: Option<String>,
    group_name: Option<String>,
    /// `(label, events, exposure)` per group.
    totals: Vec<(String, u64, f64)>,
}

impl RateAnalysis {
    /// Sum events and exposure.
    ///
    /// # Errors
    /// Invalid or non-numeric columns, counts that are not non-negative
    /// integers, non-positive exposure, or no usable rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &RateRequest) -> Result<Self> {
        let num = |i: usize| -> Result<(&[f64], String)> {
            let c = table.column(i).ok_or_else(|| invalid("no such column"))?;
            Ok((c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?, c.name.clone()))
        };
        let (counts, count_name) = num(request.count)?;
        let exposure = request.exposure.map(num).transpose()?;
        let group = request.group.map(|g| table.column(g).ok_or_else(|| invalid("no such column"))).transpose()?;
        let mut totals: Vec<(String, u64, f64)> = Vec::new();
        for r in 0..table.n_rows() {
            let c = counts[r];
            let e = exposure.as_ref().map_or(1.0, |(v, _)| v[r]);
            if states.is_excluded(r) || !c.is_finite() || !e.is_finite() {
                continue;
            }
            if c < 0.0 || c.fract() != 0.0 {
                return Err(invalid(format!(
                    "{count_name} must hold whole, non-negative counts (row {} has {c})",
                    r + 1
                )));
            }
            // Counts beyond 2⁵³ are not exact in floating point (and their sums could overflow).
            if c > MAX_EXACT_COUNT {
                return Err(invalid(format!("{count_name} has a count too large to handle exactly (row {})", r + 1)));
            }
            if e <= 0.0 {
                return Err(invalid(format!("exposure must be positive (row {} has {e})", r + 1)));
            }
            let label = group.map(|g| g.level_text(r)).unwrap_or_default();
            if group.is_some() && label.is_empty() {
                continue;
            }
            let i = totals.iter().position(|t| t.0 == label).unwrap_or_else(|| {
                totals.push((label, 0, 0.0));
                totals.len() - 1
            });
            totals[i].1 = totals[i]
                .1
                .checked_add(c as u64)
                .ok_or_else(|| invalid(format!("the total of {count_name} is too large to handle")))?;
            totals[i].2 += e;
        }
        if totals.is_empty() {
            return Err(invalid("no usable rows"));
        }
        totals.sort_by(|a, b| natural_order(&a.0, &b.0));
        Ok(Self {
            request: request.clone(),
            count_name,
            exposure_name: exposure.map(|e| e.1),
            group_name: group.map(|g| g.name.clone()),
            totals,
        })
    }

    /// `(label, events, exposure)` per group.
    #[must_use]
    pub fn totals(&self) -> &[(String, u64, f64)] {
        &self.totals
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let per = self.exposure_name.as_ref().map_or_else(|| "per row".to_owned(), |e| format!("per {e}"));
        match &self.group_name {
            Some(g) => format!("Event Rates: {} {per} by {g}", self.count_name),
            None => format!("Event Rate: {} {per}", self.count_name),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let pct = format_number(100.0 * r.level, 4);
        let heading = self.group_name.clone().unwrap_or_default();
        let mut summary = TextTable::new([
            heading.clone(),
            "Events".to_owned(),
            "Exposure".to_owned(),
            "Rate".to_owned(),
            format!("Lower {pct}% (exact)"),
            format!("Upper {pct}% (exact)"),
        ]);
        let mut intervals = Vec::new();
        for (label, x, e) in &self.totals {
            let (lo, hi) = one_rate_test(*x, *e, 1.0, r.level, Alternative::TwoSided)
                .map(|t| t.confidence_interval)
                .unwrap_or((f64::NAN, f64::NAN));
            let rate = *x as f64 / e;
            intervals.push((rate, lo, hi));
            summary.push([
                if label.is_empty() { "All rows".to_owned() } else { label.clone() },
                x.to_string(),
                format_number(*e, DIGITS + 1),
                format_number(rate, DIGITS),
                format_number(lo, DIGITS),
                format_number(hi, DIGITS),
            ]);
        }
        let mut out = vec![section(SectionKind::RateSummary, "Rates", SectionContent::Table(summary))];
        let mut t = TextTable::new(test_columns(None));
        let title = match self.totals.len() {
            1 => {
                let (_, x, e) = self.totals[0];
                push_test(&mut t, None, "Exact Poisson rate", one_rate_test(x, e, r.null_rate, r.level, r.alternative));
                format!("Test (H₁: rate {} {})", r.alternative.symbol(), format_number(r.null_rate, DIGITS))
            }
            2 => {
                let ((l1, x1, e1), (l2, x2, e2)) = (&self.totals[0], &self.totals[1]);
                push_test(&mut t, None, "Rate ratio", rate_ratio_test(*x1, *e1, *x2, *e2, 1.0, r.level, r.alternative));
                format!("Test of the Rate Ratio {l1} : {l2} (H₁: ratio {} 1)", r.alternative.symbol())
            }
            _ => {
                // Pearson χ² for equal rates: expected events proportional to exposure.
                let total_x: f64 = self.totals.iter().map(|t| t.1 as f64).sum();
                let total_e: f64 = self.totals.iter().map(|t| t.2).sum();
                let chi: f64 = self
                    .totals
                    .iter()
                    .map(|t| {
                        let expected = total_x * t.2 / total_e;
                        (t.1 as f64 - expected).powi(2) / expected
                    })
                    .sum();
                let df = (self.totals.len() - 1) as f64;
                let p = ChiSquared::new(df).and_then(|c| c.sf(chi)).unwrap_or(f64::NAN);
                let mut tt = TextTable::new(["Test", "χ²", "DF", "p-value"]);
                tt.push([
                    "Equal rates (Pearson χ²)".to_owned(),
                    format_number(chi, DIGITS),
                    format_number(df, 3),
                    format_p_value(p),
                ]);
                t = tt;
                "Test of Equal Rates (all groups)".to_owned()
            }
        };
        out.push(section(SectionKind::RateTests, title, SectionContent::Table(t)));
        let counts: Vec<(String, u64, u64)> = self.totals.iter().map(|(l, x, _)| (l.clone(), *x, 0)).collect();
        let mut plot =
            proportion_plot(&counts, &intervals, &heading, &format!("{} per unit exposure", self.count_name), "Rate");
        plot.kind = SectionKind::RatePlot;
        plot.title = "Rates with Exact Confidence Intervals".to_owned();
        out.push(plot);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        // 45 of 80 vs 30 of 85, outcome as text.
        let mut outcome = Vec::new();
        let mut group = Vec::new();
        let mut after = Vec::new();
        for i in 0..165 {
            let (g, yes) = if i < 80 { ("Treated", i < 45) } else { ("Control", i - 80 < 30) };
            group.push(g.to_owned());
            outcome.push(if yes { "yes" } else { "no" }.to_owned());
            after.push(if i % 3 == 0 { "yes" } else { "no" }.to_owned());
        }
        DataTable::new(
            "p",
            vec![Column::text("Improved", outcome), Column::text("Arm", group), Column::text("Later", after)],
        )
        .unwrap()
    }

    #[test]
    fn two_groups_from_one_outcome_column() {
        let t = table();
        let req = ProportionRequest::new(0, ProportionComparison::Groups { group: 1 });
        let a = ProportionAnalysis::run(&t, &RowStateManager::new(165), &req).unwrap();
        assert_eq!(a.title(), "Proportions: Improved = yes by Arm");
        assert_eq!(a.counts(), [("Control".to_owned(), 30, 85), ("Treated".to_owned(), 45, 80)]);
        let s = a.sections();
        let SectionContent::Table(tests) = &s[1].content else { panic!() };
        // Control − Treated: z = −2.7018, Fisher p = 0.00804 (SciPy).
        assert_eq!(tests.rows[0][3], "z = -2.7018");
        assert_eq!(tests.rows[1][5], "0.0080");
        assert_eq!(s.last().unwrap().kind, SectionKind::ProportionPlot);
    }

    #[test]
    fn one_sample_paired_and_several_groups() {
        let t = table();
        let rows = RowStateManager::new(165);
        let one = ProportionAnalysis::run(
            &t,
            &rows,
            &ProportionRequest {
                event: Some("yes".into()),
                ..ProportionRequest::new(0, ProportionComparison::OneSample { null: 0.5 })
            },
        )
        .unwrap();
        assert_eq!(one.counts()[0].1, 75);
        let SectionContent::Table(tests) = &one.sections()[1].content else { panic!() };
        assert_eq!(tests.rows.len(), 2);
        let paired =
            ProportionAnalysis::run(&t, &rows, &ProportionRequest::new(0, ProportionComparison::Paired { second: 2 }))
                .unwrap();
        assert!(paired.title().starts_with("Paired Proportions"));
        assert!(paired.sections().iter().any(|s| s.kind == SectionKind::ContingencyCounts));
        let three =
            ProportionAnalysis::run(&t, &rows, &ProportionRequest::new(2, ProportionComparison::Groups { group: 0 }));
        assert!(three.is_ok());
    }

    #[test]
    fn rates_by_group() {
        // 11 events in 800 vs 21 in 3011 (R poisson.test example): p = 0.07967.
        let counts = vec![5.0, 6.0, 10.0, 11.0];
        let exposure = vec![400.0, 400.0, 1500.0, 1511.0];
        let group: Vec<String> = ["A", "A", "B", "B"].map(str::to_owned).to_vec();
        let t = DataTable::new(
            "r",
            vec![Column::numeric("Events", counts), Column::numeric("Years", exposure), Column::text("Plant", group)],
        )
        .unwrap();
        let req = RateRequest { exposure: Some(1), group: Some(2), ..RateRequest::new(0) };
        let a = RateAnalysis::run(&t, &RowStateManager::new(4), &req).unwrap();
        assert_eq!(a.title(), "Event Rates: Events per Years by Plant");
        let s = a.sections();
        let SectionContent::Table(tests) = &s[1].content else { panic!() };
        assert_eq!(tests.rows[0][1], "1.9715");
        assert_eq!(tests.rows[0][5], "0.0797");
        let bad = DataTable::new("r", vec![Column::numeric("Events", vec![1.5])]).unwrap();
        assert!(RateAnalysis::run(&bad, &RowStateManager::new(1), &RateRequest::new(0)).is_err());
    }
}
