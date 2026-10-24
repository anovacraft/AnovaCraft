//! Comparative tests bound to a dataset ([`stat_inference`]): one sample
//! against a hypothesized value, two or more independent groups, and paired
//! or repeated measurements.
//!
//! Data can be laid out either way:
//!
//! * **stacked** — every measurement in one column, with a second column
//!   naming its group (or, for paired data, its condition and subject);
//! * **side by side** — one column per group or condition (paired values on
//!   the same row).
//!
//! Excluded rows and missing values are left out. For paired data in stacked
//! form, subjects without a value for every condition are dropped.

use crate::format::{format_number, format_p_value};
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_inference::best::{BestDirection, comparisons_with_best};
use stat_inference::control::dunnett_test;
use stat_inference::effect_sizes::anova_effect_size;
use stat_inference::equivalence::{EquivalenceTest, tost_one_sample, tost_paired, tost_paired_ratio, tost_two_sample};
use stat_inference::kolmogorov::ks_two_sample;
use stat_inference::means::{
    RatioLimits, effect_size, one_sample_t, one_sample_z, paired_effect_size, paired_t, ratio_of_means, two_sample_t,
};
use stat_inference::multiple::{ComparisonMethod, PairwiseComparison, compare_all_pairs, holm_adjust};
use stat_inference::nonparametric::{friedman_test, paired_signed_rank, rank_sum_test, sign_test, signed_rank_test};
use stat_inference::repeated::{RepeatedMeasuresAnova, repeated_measures_anova};
pub use stat_inference::runs::MedianTies;
use stat_inference::runs::median_test;
use stat_inference::variances::{one_sample_std_dev, variance_ratio_test};
use stat_inference::{Alternative, ChiSquareResult, TestResult};
use stat_linear::group_tests::{kruskal_wallis, variance_tests, welch_anova};
use stat_linear::{natural_order, one_way_anova};

pub(crate) const DIGITS: usize = 5;

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn column_name(table: &DataTable, i: usize) -> Result<String> {
    table.column(i).map(|c| c.name.clone()).ok_or_else(|| invalid("no such column"))
}

fn numeric(table: &DataTable, i: usize) -> Result<&[f64]> {
    let c = table.column(i).ok_or_else(|| invalid("no such column"))?;
    c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))
}

/// Group labels with each group's `(row, value)` pairs.
pub(crate) type GroupedValues = (Vec<String>, Vec<Vec<(usize, f64)>>);

/// Values of `value` split by the labels of `group` (natural order), as
/// `(row, value)` pairs, skipping excluded rows, missing values and empty
/// labels. Without a group column everything is one group labelled "".
pub(crate) fn split_by_group(
    table: &DataTable,
    states: &RowStateManager,
    value: usize,
    group: Option<usize>,
) -> Result<GroupedValues> {
    let values = numeric(table, value)?;
    let group_col = match group {
        Some(g) if g == value => return Err(invalid("the grouping column must differ from the data column")),
        Some(g) => Some(table.column(g).ok_or_else(|| invalid("no such column"))?),
        None => None,
    };
    let mut labels: Vec<String> = Vec::new();
    let mut members: Vec<Vec<(usize, f64)>> = Vec::new();
    for r in 0..table.n_rows() {
        if states.is_excluded(r) || !values[r].is_finite() {
            continue;
        }
        let label = group_col.map(|c| c.level_text(r)).unwrap_or_default();
        if group_col.is_some() && label.is_empty() {
            continue;
        }
        let g = labels.iter().position(|l| *l == label).unwrap_or_else(|| {
            labels.push(label);
            members.push(Vec::new());
            labels.len() - 1
        });
        members[g].push((r, values[r]));
    }
    let mut order: Vec<usize> = (0..labels.len()).collect();
    order.sort_by(|&a, &b| natural_order(&labels[a], &labels[b]));
    Ok((
        order.iter().map(|&i| labels[i].clone()).collect(),
        order.iter().map(|&i| std::mem::take(&mut members[i])).collect(),
    ))
}

fn values(m: &[(usize, f64)]) -> Vec<f64> {
    m.iter().map(|v| v.1).collect()
}

pub(crate) fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn df_text(t: &TestResult) -> String {
    match (t.df, t.df2) {
        (Some(a), Some(b)) => format!("{}, {}", format_number(a, 4), format_number(b, 4)),
        (Some(a), None) => format_number(a, 4),
        _ => String::new(),
    }
}

/// Columns of a test table, with an optional leading group column.
pub(crate) fn test_columns(group: Option<&str>) -> Vec<String> {
    let mut c: Vec<String> = group.map(|g| vec![g.to_owned()]).unwrap_or_default();
    c.extend(["Test", "Estimate", "Hypothesized", "Statistic", "DF", "p-value", "Lower", "Upper"].map(str::to_owned));
    c
}

/// One row of a test table.
pub(crate) fn test_row(group: Option<&str>, t: &TestResult) -> Vec<String> {
    let mut row: Vec<String> = group.map(|g| vec![g.to_owned()]).unwrap_or_default();
    row.extend([
        t.method.to_owned(),
        format_number(t.estimate, DIGITS),
        format_number(t.null_value, DIGITS),
        format!("{} = {}", t.statistic_name, format_number(t.statistic, DIGITS)),
        df_text(t),
        format_p_value(t.p_value),
        format_number(t.confidence_interval.0, DIGITS),
        format_number(t.confidence_interval.1, DIGITS),
    ]);
    row
}

/// A failed test as a table row.
pub(crate) fn error_row(group: Option<&str>, method: &str, e: &StatError) -> Vec<String> {
    let mut row: Vec<String> = group.map(|g| vec![g.to_owned()]).unwrap_or_default();
    row.push(method.to_owned());
    row.push(format!("not available: {e}"));
    row.extend((0..6).map(|_| String::new()));
    row
}

pub(crate) fn push_test(table: &mut TextTable, group: Option<&str>, method: &str, r: Result<TestResult>) {
    match r {
        Ok(t) => table.push(test_row(group, &t)),
        Err(e) => table.push(error_row(group, method, &e)),
    }
}

fn hypothesis_note(alternative: Alternative, level: f64, parameter: &str, null: f64) -> String {
    format!(
        "H₁: {parameter} {} {}; {}% confidence limits",
        alternative.symbol(),
        format_number(null, DIGITS),
        format_number(100.0 * level, 4)
    )
}

pub(crate) fn equivalence_table(
    group: Option<&str>,
    rows: Vec<(Option<String>, Result<EquivalenceTest>)>,
) -> TextTable {
    let mut c: Vec<String> = group.map(|g| vec![g.to_owned()]).unwrap_or_default();
    c.extend(
        ["Estimate", "Lower bound", "Upper bound", "p (vs lower)", "p (vs upper)", "Interval (1 − 2α)", "Conclusion"]
            .map(str::to_owned),
    );
    let mut t = TextTable::new(c);
    for (label, r) in rows {
        let mut row: Vec<String> = label.map(|l| vec![l]).unwrap_or_default();
        match r {
            Ok(e) => row.extend([
                format_number(e.estimate, DIGITS),
                format_number(e.lower_bound, DIGITS),
                format_number(e.upper_bound, DIGITS),
                format_p_value(e.p_lower),
                format_p_value(e.p_upper),
                format!(
                    "{} to {}",
                    format_number(e.confidence_interval.0, DIGITS),
                    format_number(e.confidence_interval.1, DIGITS)
                ),
                if e.equivalent() { "equivalent".to_owned() } else { "not shown equivalent".to_owned() },
            ]),
            Err(err) => {
                row.push(format!("not available: {err}"));
                row.extend((0..6).map(|_| String::new()));
            }
        }
        t.push(row);
    }
    t
}

/// An equivalence test computed on logs, reported as ratios: the estimate,
/// bounds and interval exponentiated (the p-values are unchanged).
pub(crate) fn ratio_scale(e: &EquivalenceTest) -> EquivalenceTest {
    EquivalenceTest {
        estimate: e.estimate.exp(),
        lower_bound: e.lower_bound.exp(),
        upper_bound: e.upper_bound.exp(),
        confidence_interval: (e.confidence_interval.0.exp(), e.confidence_interval.1.exp()),
        ..e.clone()
    }
}

/// Values-by-group plot with each group's mean (bar) and confidence interval
/// (vertical segment), and an optional horizontal reference line.
#[allow(clippy::too_many_arguments)]
fn values_plot(
    title: &str,
    value_name: &str,
    group_name: &str,
    labels: &[String],
    members: &[Vec<(usize, f64)>],
    intervals: &[(f64, f64, f64)],
    reference: Option<f64>,
    kind: SectionKind,
) -> ReportSection {
    let mut points = Vec::new();
    let mut membership = Vec::new();
    for (g, m) in members.iter().enumerate() {
        let n = m.len();
        for (j, &(row, y)) in m.iter().enumerate() {
            let spread = if n > 1 { -0.2 + 0.4 * j as f64 / (n - 1) as f64 } else { 0.0 };
            points.push(RowPoint { row, x: (g + 1) as f64 + spread, y });
            membership.push(g);
        }
    }
    let lines = intervals
        .iter()
        .enumerate()
        .map(|(g, &(m, _, _))| vec![[(g + 1) as f64 - 0.3, m], [(g + 1) as f64 + 0.3, m]])
        .collect();
    let mut segments: Vec<Vec<[f64; 2]>> = intervals
        .iter()
        .enumerate()
        .filter(|(_, (_, lo, hi))| lo.is_finite() && hi.is_finite())
        .map(|(g, &(_, lo, hi))| vec![[(g + 1) as f64 + 0.32, lo], [(g + 1) as f64 + 0.32, hi]])
        .collect();
    if let Some(r) = reference {
        segments.push(vec![[0.5, r], [labels.len() as f64 + 0.5, r]]);
    }
    let display: Vec<String> =
        labels.iter().map(|l| if l.is_empty() { value_name.to_owned() } else { l.clone() }).collect();
    let spec = PlotSpec {
        x_time: None,
        y_time: None,
        x_label: group_name.to_owned(),
        y_label: value_name.to_owned(),
        points,
        line: None,
        band: None,
        groups: Some(PointGroups { title: group_name.to_owned(), labels: display.clone(), membership, lines }),
        x_ticks: Some(display.iter().enumerate().map(|(g, l)| ((g + 1) as f64, l.clone())).collect()),
        panes: Vec::new(),
        annotations: None,
        segments,
        ..PlotSpec::default()
    };
    section(kind, title, SectionContent::Plot(Box::new(spec)))
}

fn mean_interval(x: &[f64], level: f64) -> (f64, f64, f64) {
    match one_sample_t(x, 0.0, level, Alternative::TwoSided) {
        Ok(t) => (t.estimate, t.confidence_interval.0, t.confidence_interval.1),
        Err(_) => (x.iter().sum::<f64>() / x.len().max(1) as f64, f64::NAN, f64::NAN),
    }
}

fn summary_table(group_heading: Option<&str>, labels: &[String], groups: &[Vec<f64>], level: f64) -> TextTable {
    let pct = format_number(100.0 * level, 4);
    let mut c: Vec<String> = group_heading.map(|g| vec![g.to_owned()]).unwrap_or_default();
    c.extend(["N".to_owned(), "Mean".to_owned(), "Std dev".to_owned(), "Std error".to_owned(), "Median".to_owned()]);
    c.push(format!("Lower {pct}% (mean)"));
    c.push(format!("Upper {pct}% (mean)"));
    let mut t = TextTable::new(c);
    for (label, x) in labels.iter().zip(groups) {
        let n = x.len() as f64;
        let mean = x.iter().sum::<f64>() / n;
        let sd =
            if x.len() > 1 { (x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt() } else { f64::NAN };
        let mut sorted = x.clone();
        sorted.sort_by(f64::total_cmp);
        let med = if sorted.is_empty() {
            f64::NAN
        } else if sorted.len() % 2 == 1 {
            sorted[sorted.len() / 2]
        } else {
            0.5 * (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2])
        };
        let (_, lo, hi) = mean_interval(x, level);
        let mut row: Vec<String> = group_heading.map(|_| vec![label.clone()]).unwrap_or_default();
        row.extend([
            x.len().to_string(),
            format_number(mean, DIGITS),
            format_number(sd, DIGITS),
            format_number(sd / n.sqrt(), DIGITS),
            format_number(med, DIGITS),
            format_number(lo, DIGITS),
            format_number(hi, DIGITS),
        ]);
        t.push(row);
    }
    t
}

// ───────────────────────────── One sample ─────────────────────────────

/// Columns and settings of a one-sample analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct OneSampleRequest {
    /// Numeric data column.
    pub column: usize,
    /// Optional grouping column: the tests are run separately in each group.
    pub group: Option<usize>,
    /// Hypothesized mean (and median for the rank tests).
    pub null_mean: f64,
    /// Known population standard deviation, for a z test.
    pub known_std_dev: Option<f64>,
    /// Hypothesized standard deviation, for the chi-square test.
    pub null_std_dev: Option<f64>,
    /// Equivalence bounds for the mean.
    pub equivalence: Option<(f64, f64)>,
    /// Alternative hypothesis.
    pub alternative: Alternative,
    /// Confidence level.
    pub level: f64,
}

impl OneSampleRequest {
    /// Default settings for `column`: two-sided tests of mean 0 at 95%.
    #[must_use]
    pub fn new(column: usize) -> Self {
        Self {
            column,
            group: None,
            null_mean: 0.0,
            known_std_dev: None,
            null_std_dev: None,
            equivalence: None,
            alternative: Alternative::TwoSided,
            level: 0.95,
        }
    }
}

/// A one-sample analysis.
#[derive(Debug, Clone)]
pub struct OneSampleAnalysis {
    request: OneSampleRequest,
    value_name: String,
    group_name: Option<String>,
    labels: Vec<String>,
    members: Vec<Vec<(usize, f64)>>,
}

impl OneSampleAnalysis {
    /// Gather the data.
    ///
    /// # Errors
    /// Invalid columns, a non-numeric data column, or no usable values.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &OneSampleRequest) -> Result<Self> {
        let (labels, members) = split_by_group(table, states, request.column, request.group)?;
        if members.is_empty() {
            return Err(invalid("no usable values"));
        }
        Ok(Self {
            request: request.clone(),
            value_name: column_name(table, request.column)?,
            group_name: request.group.map(|g| column_name(table, g)).transpose()?,
            labels,
            members,
        })
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.group_name {
            Some(g) => format!("One-Sample Tests: {} by {g}", self.value_name),
            None => format!("One-Sample Tests: {}", self.value_name),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let g = self.group_name.as_deref();
        let groups: Vec<Vec<f64>> = self.members.iter().map(|m| values(m)).collect();
        let mut out = vec![section(
            SectionKind::ComparisonSummary,
            "Summary",
            SectionContent::Table(summary_table(g, &self.labels, &groups, r.level)),
        )];
        let mut tests = TextTable::new(test_columns(g));
        for (label, x) in self.labels.iter().zip(&groups) {
            let lab = g.map(|_| label.as_str());
            push_test(&mut tests, lab, "One-sample t", one_sample_t(x, r.null_mean, r.level, r.alternative));
            if let Some(s) = r.known_std_dev {
                push_test(&mut tests, lab, "One-sample z", one_sample_z(x, r.null_mean, s, r.level, r.alternative));
            }
            push_test(
                &mut tests,
                lab,
                "Wilcoxon signed-rank",
                signed_rank_test(x, r.null_mean, r.level, r.alternative),
            );
            push_test(&mut tests, lab, "Sign test", sign_test(x, r.null_mean, r.level, r.alternative));
            if let Some(s0) = r.null_std_dev {
                push_test(&mut tests, lab, "Standard deviation", one_sample_std_dev(x, s0, r.level, r.alternative));
            }
        }
        let note = hypothesis_note(r.alternative, r.level, "mean (median for rank tests)", r.null_mean);
        out.push(section(
            SectionKind::ComparisonTests,
            format!("Tests ({note}; signed-rank estimate: Hodges–Lehmann, sign-test estimate: median)"),
            SectionContent::Table(tests),
        ));
        if let Some(bounds) = r.equivalence {
            let rows = self
                .labels
                .iter()
                .zip(&groups)
                .map(|(l, x)| (g.map(|_| l.clone()), tost_one_sample(x, bounds, (1.0 - r.level) / 2.0)))
                .collect();
            out.push(section(
                SectionKind::EquivalenceTests,
                format!("Equivalence (TOST at α = {})", format_number((1.0 - r.level) / 2.0, 4)),
                SectionContent::Table(equivalence_table(g, rows)),
            ));
        }
        let intervals: Vec<(f64, f64, f64)> = groups.iter().map(|x| mean_interval(x, r.level)).collect();
        out.push(values_plot(
            "Values (bar: mean; vertical line: confidence interval; horizontal line: hypothesized mean)",
            &self.value_name,
            g.unwrap_or(""),
            &self.labels,
            &self.members,
            &intervals,
            Some(r.null_mean),
            SectionKind::ValuesByGroup,
        ));
        out
    }
}

// ─────────────────────────── Independent groups ───────────────────────────

/// Where the groups' values are.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupLayout {
    /// One data column and a column naming each row's group.
    Stacked {
        /// Numeric data column.
        response: usize,
        /// Grouping column.
        group: usize,
    },
    /// One numeric column per group.
    Columns(Vec<usize>),
}

/// Settings of a comparison of independent groups.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupComparisonRequest {
    /// Data layout.
    pub layout: GroupLayout,
    /// Hypothesized difference (first − second) for two groups.
    pub null_difference: f64,
    /// Alternative hypothesis (two groups).
    pub alternative: Alternative,
    /// Confidence level.
    pub level: f64,
    /// Multiple-comparison procedure.
    pub method: ComparisonMethod,
    /// Equivalence bounds for the difference (two groups).
    pub equivalence: Option<(f64, f64)>,
    /// Label of the control group: each other group is compared with it
    /// (Dunnett 1955). `None`: no comparisons with a control.
    pub control: Option<String>,
    /// How Mood's median test counts values equal to the grand median.
    pub median_ties: MedianTies,
    /// Compare each group with the best of the others (Hsu 1996), larger or
    /// smaller being better. `None`: off.
    pub best: Option<BestDirection>,
    /// Test for a linear trend across the groups in their order (three or
    /// more groups).
    pub linear_trend: bool,
    /// Two groups: the ratio of the means (first / second) with Fieller's
    /// interval.
    pub ratio_of_means: bool,
}

impl GroupComparisonRequest {
    /// Default settings for `layout`.
    #[must_use]
    pub fn new(layout: GroupLayout) -> Self {
        Self {
            layout,
            null_difference: 0.0,
            alternative: Alternative::TwoSided,
            level: 0.95,
            method: ComparisonMethod::TukeyKramer,
            equivalence: None,
            control: None,
            median_ties: MedianTies::Below,
            best: None,
            linear_trend: false,
            ratio_of_means: false,
        }
    }

    /// Labels of the groups in `table` (every row; natural order for a
    /// group column, column names side by side), for choosing a control.
    #[must_use]
    pub fn group_labels(&self, table: &DataTable) -> Vec<String> {
        match &self.layout {
            GroupLayout::Stacked { response, group } => {
                split_by_group(table, &RowStateManager::new(table.n_rows()), *response, Some(*group))
                    .map(|(labels, _)| labels)
                    .unwrap_or_default()
            }
            GroupLayout::Columns(cols) => cols.iter().filter_map(|&c| column_name(table, c).ok()).collect(),
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match &self.layout {
            GroupLayout::Stacked { response, group } => vec![*response, *group],
            GroupLayout::Columns(c) => c.clone(),
        }
    }
}

/// A comparison of independent groups.
#[derive(Debug, Clone)]
pub struct GroupComparison {
    request: GroupComparisonRequest,
    value_name: String,
    group_name: String,
    labels: Vec<String>,
    members: Vec<Vec<(usize, f64)>>,
    pairwise: Option<Result<Vec<PairwiseComparison>>>,
}

impl GroupComparison {
    /// Gather the groups and run the multiple comparisons.
    ///
    /// # Errors
    /// Invalid or non-numeric columns, or fewer than two non-empty groups.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &GroupComparisonRequest) -> Result<Self> {
        let (value_name, group_name, labels, members) = match &request.layout {
            GroupLayout::Stacked { response, group } => {
                let (labels, members) = split_by_group(table, states, *response, Some(*group))?;
                (column_name(table, *response)?, column_name(table, *group)?, labels, members)
            }
            GroupLayout::Columns(cols) => {
                let mut labels = Vec::new();
                let mut members = Vec::new();
                for &c in cols {
                    let v = numeric(table, c)?;
                    labels.push(column_name(table, c)?);
                    members.push(
                        (0..table.n_rows())
                            .filter(|&r| !states.is_excluded(r) && v[r].is_finite())
                            .map(|r| (r, v[r]))
                            .collect(),
                    );
                }
                ("Value".to_owned(), "Column".to_owned(), labels, members)
            }
        };
        let keep: Vec<usize> = (0..members.len()).filter(|&g| !members[g].is_empty()).collect();
        let labels: Vec<String> = keep.iter().map(|&g| labels[g].clone()).collect();
        let members: Vec<Vec<(usize, f64)>> = keep.iter().map(|&g| members[g].clone()).collect();
        if members.len() < 2 {
            return Err(invalid("a comparison needs at least two groups with values"));
        }
        let groups: Vec<Vec<f64>> = members.iter().map(|m| values(m)).collect();
        let pairwise = (groups.len() >= 3).then(|| compare_all_pairs(&groups, request.method, request.level));
        Ok(Self { request: request.clone(), value_name, group_name, labels, members, pairwise })
    }

    /// Group labels.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.request.layout {
            GroupLayout::Stacked { .. } => format!("Compare Groups: {} by {}", self.value_name, self.group_name),
            GroupLayout::Columns(_) => format!("Compare Columns: {}", self.labels.join(", ")),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let groups: Vec<Vec<f64>> = self.members.iter().map(|m| values(m)).collect();
        let heading = self.group_name.as_str();
        let mut out = vec![section(
            SectionKind::GroupMeans,
            "Summary by Group",
            SectionContent::Table(summary_table(Some(heading), &self.labels, &groups, r.level)),
        )];
        if groups.len() == 2 {
            let (a, b) = (&groups[0], &groups[1]);
            let mut tests = TextTable::new(test_columns(None));
            push_test(
                &mut tests,
                None,
                "Two-sample t (Welch)",
                two_sample_t(a, b, r.null_difference, false, r.level, r.alternative),
            );
            push_test(
                &mut tests,
                None,
                "Two-sample t (pooled)",
                two_sample_t(a, b, r.null_difference, true, r.level, r.alternative),
            );
            push_test(&mut tests, None, "Rank-sum", rank_sum_test(a, b, r.null_difference, r.level, r.alternative));
            push_test(&mut tests, None, "Variance ratio", variance_ratio_test(a, b, 1.0, r.level, r.alternative));
            push_test(&mut tests, None, "Kolmogorov–Smirnov", ks_two_sample(a, b, r.alternative, None));
            let parameter = format!("{} − {}", self.labels[0], self.labels[1]);
            out.push(section(
                SectionKind::ComparisonTests,
                format!(
                    "Two-Sample Tests ({}; rank-sum estimate: Hodges–Lehmann shift; variance ratio tested against 1; \
                     Kolmogorov–Smirnov D: largest gap between the two distribution functions, testing equal \
                     distributions)",
                    hypothesis_note(r.alternative, r.level, &parameter, r.null_difference)
                ),
                SectionContent::Table(tests),
            ));
            let mut es = TextTable::new(["Measure", "Value", "Interpretation (Cohen 1988)"]);
            match effect_size(a, b) {
                Ok(e) => {
                    let size = |d: f64| match d.abs() {
                        v if v < 0.2 => "negligible",
                        v if v < 0.5 => "small",
                        v if v < 0.8 => "medium",
                        _ => "large",
                    };
                    es.push(["Cohen's d".to_owned(), format_number(e.cohens_d, 4), size(e.cohens_d).to_owned()]);
                    es.push(["Hedges' g".to_owned(), format_number(e.hedges_g, 4), size(e.hedges_g).to_owned()]);
                    es.push([
                        format!("Glass's Δ (scaled by the SD of {})", self.labels[1]),
                        format_number(e.glass_delta, 4),
                        if e.glass_delta.is_nan() { String::new() } else { size(e.glass_delta).to_owned() },
                    ]);
                }
                Err(e) => es.push(["Effect size".to_owned(), format!("not available: {e}"), String::new()]),
            }
            out.push(section(SectionKind::EffectSizes, "Effect Size", SectionContent::Table(es)));
            if r.ratio_of_means {
                out.push(ratio_section(&self.labels, a, b, r.level, r.alternative));
            }
            if let Some(bounds) = r.equivalence {
                let alpha = (1.0 - r.level) / 2.0;
                let rows = vec![
                    (Some("Welch".to_owned()), tost_two_sample(a, b, bounds, false, alpha)),
                    (Some("Pooled".to_owned()), tost_two_sample(a, b, bounds, true, alpha)),
                ];
                out.push(section(
                    SectionKind::EquivalenceTests,
                    format!("Equivalence of {parameter} (TOST at α = {})", format_number(alpha, 4)),
                    SectionContent::Table(equivalence_table(Some("Variance"), rows)),
                ));
            }
        }
        out.push(self.equal_means(&groups));
        out.push(one_way_effect_sizes(&groups, r.level));
        out.push(median_test_section(heading, &self.labels, &groups, r.median_ties));
        out.push(self.equal_variances(&groups));
        if let Some(p) = &self.pairwise {
            out.push(self.pairwise_section(p));
        }
        if r.linear_trend && groups.len() >= 3 {
            out.push(crate::effect_size_sections::linear_trend_section(&self.labels, &groups, r.level));
        }
        if let Some(control) = &r.control {
            match self.labels.iter().position(|l| l == control) {
                Some(c) => out.extend(control_sections(
                    heading,
                    &self.value_name,
                    &self.labels,
                    &groups,
                    c,
                    r.level,
                    r.alternative,
                )),
                None => out.push(section(
                    SectionKind::ControlComparisons,
                    "Comparisons with Control",
                    SectionContent::Note(format!(
                        "The control group {control:?} has no values; choose another in the report's settings."
                    )),
                )),
            }
        }
        if let Some(direction) = r.best {
            out.extend(best_sections(heading, &self.value_name, &self.labels, &groups, r.level, direction));
        }
        let intervals: Vec<(f64, f64, f64)> = groups.iter().map(|x| mean_interval(x, r.level)).collect();
        out.push(values_plot(
            "Values by Group (bar: mean; vertical line: confidence interval of the mean)",
            &self.value_name,
            heading,
            &self.labels,
            &self.members,
            &intervals,
            None,
            SectionKind::ValuesByGroup,
        ));
        out
    }

    fn equal_means(&self, groups: &[Vec<f64>]) -> ReportSection {
        let mut t = TextTable::new(["Test", "Statistic", "DF", "p-value"]);
        match one_way_anova(groups) {
            Ok(a) => t.push([
                "One-way ANOVA (equal variances)".to_owned(),
                format!("F = {}", a.f_statistic.map_or("—".to_owned(), |f| format_number(f, DIGITS))),
                format!("{}, {}", a.df_between, a.df_within),
                a.p_value.map_or("—".to_owned(), format_p_value),
            ]),
            Err(e) => t.push(["One-way ANOVA".to_owned(), format!("not available: {e}"), String::new(), String::new()]),
        }
        match welch_anova(groups) {
            Ok(w) => t.push([
                "Welch ANOVA (unequal variances)".to_owned(),
                format!("F = {}", format_number(w.f_statistic, DIGITS)),
                format!("{}, {}", format_number(w.df1, 4), format_number(w.df2, 4)),
                format_p_value(w.p_value),
            ]),
            Err(e) => t.push(["Welch ANOVA".to_owned(), format!("not available: {e}"), String::new(), String::new()]),
        }
        match kruskal_wallis(groups) {
            Ok(k) => t.push([
                "Kruskal–Wallis (ranks)".to_owned(),
                format!("χ² = {}", format_number(k.statistic, DIGITS)),
                format_number(k.df, 4),
                format_p_value(k.p_value),
            ]),
            Err(e) => {
                t.push(["Kruskal–Wallis".to_owned(), format!("not available: {e}"), String::new(), String::new()])
            }
        }
        section(SectionKind::OneWayAnova, "Tests of Equal Centers (all groups)", SectionContent::Table(t))
    }

    fn equal_variances(&self, groups: &[Vec<f64>]) -> ReportSection {
        equal_variances_section(groups, "Tests of Equal Variances")
    }
}

/// The ratio of two group means, first / second, tested against 1 with
/// Fieller's interval ([`stat_inference::means::ratio_of_means`]), pooled
/// and with unequal variances.
fn ratio_section(labels: &[String], a: &[f64], b: &[f64], level: f64, alternative: Alternative) -> ReportSection {
    let pct = format_number(level * 100.0, 4);
    let mut t = TextTable::new([
        "Variance".to_owned(),
        "Ratio".to_owned(),
        "t".to_owned(),
        "DF".to_owned(),
        "p-value".to_owned(),
        format!("Lower {pct}%"),
        format!("Upper {pct}%"),
    ]);
    for (name, pooled) in [("Pooled", true), ("Unequal (Welch)", false)] {
        match ratio_of_means(a, b, 1.0, pooled, level, alternative) {
            Ok(r) => {
                let (lo, hi) = match r.limits {
                    RatioLimits::Bounded(lo, hi) => (format_number(lo, DIGITS), format_number(hi, DIGITS)),
                    RatioLimits::Unbounded => ("unbounded".to_owned(), "unbounded".to_owned()),
                };
                t.push([
                    name.to_owned(),
                    format_number(r.estimate, DIGITS),
                    format_number(r.statistic, DIGITS),
                    format_number(r.df, 4),
                    format_p_value(r.p_value),
                    lo,
                    hi,
                ]);
            }
            Err(e) => {
                let mut row = vec![name.to_owned(), format!("not available: {e}")];
                row.extend((0..5).map(|_| String::new()));
                t.push(row);
            }
        }
    }
    let side = match alternative {
        Alternative::TwoSided => "ratio ≠ 1",
        Alternative::Greater => "ratio > 1",
        Alternative::Less => "ratio < 1",
    };
    section(
        SectionKind::RatioOfMeans,
        format!(
            "Ratio of Means, {} / {} (Fieller interval; test of {side}; the interval is unbounded when the mean of {} \
             is not clearly different from zero)",
            labels[0], labels[1], labels[1]
        ),
        SectionContent::Table(t),
    )
}

/// Effect sizes of the one-way ANOVA of `groups` (η², ω², Cohen's f with
/// noncentral-F intervals).
pub(crate) fn one_way_effect_sizes(groups: &[Vec<f64>], level: f64) -> ReportSection {
    let r = one_way_anova(groups).and_then(|a| {
        anova_effect_size(
            a.ss_between,
            a.df_between as f64,
            a.ss_within,
            a.df_within as f64,
            a.ss_between + a.ss_within,
            a.n_total as f64,
            level,
        )
    });
    crate::effect_size_sections::anova_effect_size_section(vec![("Groups".to_owned(), r)], level)
}

/// Levene, Brown–Forsythe and Bartlett tests of equal variances across `groups`.
pub(crate) fn equal_variances_section(groups: &[Vec<f64>], title: &str) -> ReportSection {
    let mut t = TextTable::new(["Test", "Statistic", "DF", "p-value"]);
    match variance_tests(groups) {
        Ok(v) => {
            for (name, f) in [("Levene (mean)", v.levene), ("Brown–Forsythe (median)", v.brown_forsythe)] {
                t.push([
                    name.to_owned(),
                    format!("F = {}", format_number(f.f_statistic, DIGITS)),
                    format!("{}, {}", format_number(f.df1, 4), format_number(f.df2, 4)),
                    format_p_value(f.p_value),
                ]);
            }
            t.push([
                "Bartlett (assumes normality)".to_owned(),
                format!("χ² = {}", format_number(v.bartlett.statistic, DIGITS)),
                format_number(v.bartlett.df, 4),
                format_p_value(v.bartlett.p_value),
            ]);
        }
        Err(e) => t.push(["Equal variances".to_owned(), format!("not available: {e}"), String::new(), String::new()]),
    }
    section(SectionKind::EqualVariances, title, SectionContent::Table(t))
}

/// Mood's median test across `groups`: counts above and not above the grand
/// median, Pearson's χ² on k − 1 df, and the smallest expected count.
pub(crate) fn median_test_section(
    heading: &str,
    labels: &[String],
    groups: &[Vec<f64>],
    ties: MedianTies,
) -> ReportSection {
    let tie_text = match ties {
        MedianTies::Below => "values at the median counted as not above",
        MedianTies::Above => "values at the median counted as above",
        MedianTies::Ignore => "values at the median left out",
    };
    let m = match median_test(groups, ties) {
        Ok(m) => m,
        Err(e) => {
            return section(
                SectionKind::MedianTest,
                "Median Test (Mood)",
                SectionContent::Note(format!("Not available: {e}")),
            );
        }
    };
    let group_heading = if heading.is_empty() { "Group" } else { heading };
    let mut t = TextTable::new([group_heading, "Above median", "Not above median", "N"]);
    for (g, label) in labels.iter().enumerate() {
        let n = m.above[g] + m.below[g];
        t.push([label.clone(), m.above[g].to_string(), m.below[g].to_string(), n.to_string()]);
    }
    let (above, below): (usize, usize) = (m.above.iter().sum(), m.below.iter().sum());
    t.push(["Total".to_owned(), above.to_string(), below.to_string(), (above + below).to_string()]);
    let warning = if m.min_expected < 1.0 {
        format!(
            "; smallest expected count {} is below 1, so the χ² p-value is doubtful",
            format_number(m.min_expected, 3)
        )
    } else {
        format!("; smallest expected count {}", format_number(m.min_expected, 3))
    };
    section(
        SectionKind::MedianTest,
        format!(
            "Median Test (Mood; grand median {}, {tie_text}): χ² = {}, DF = {}, p = {}{warning}",
            format_number(m.grand_median, DIGITS),
            format_number(m.test.statistic, DIGITS),
            format_number(m.test.df, 3),
            format_p_value(m.test.p_value),
        ),
        SectionContent::Table(t),
    )
}

/// Most groups compared with a control: each adjusted p-value is a double
/// integral whose cost grows with the number of distinct group sizes, so a
/// grouping column with hundreds of levels would stall the report.
const MAX_CONTROL_COMPARISONS: usize = 100;

/// Dunnett's comparisons of every group with `groups[control]`: a table of
/// differences with simultaneous intervals and adjusted p-values, and an
/// interval plot of the differences with a zero reference line.
pub(crate) fn control_sections(
    heading: &str,
    value_name: &str,
    labels: &[String],
    groups: &[Vec<f64>],
    control: usize,
    level: f64,
    alternative: Alternative,
) -> Vec<ReportSection> {
    let treated: Vec<usize> = (0..groups.len()).filter(|&g| g != control).collect();
    if treated.len() > MAX_CONTROL_COMPARISONS {
        return vec![section(
            SectionKind::ControlComparisons,
            format!("Comparisons with Control Group “{}”", labels[control]),
            SectionContent::Note(format!(
                "Not computed: {} groups to compare with the control; the limit is {MAX_CONTROL_COMPARISONS}. \
                 Group by a column with fewer levels.",
                treated.len()
            )),
        )];
    }
    let treatments: Vec<&[f64]> = treated.iter().map(|&g| groups[g].as_slice()).collect();
    let d = match dunnett_test(&groups[control], &treatments, level, alternative) {
        Ok(d) => d,
        Err(e) => {
            return vec![section(
                SectionKind::ControlComparisons,
                format!("Comparisons with Control Group “{}”", labels[control]),
                SectionContent::Note(format!("Not available: {e}")),
            )];
        }
    };
    let group_heading = if heading.is_empty() { "Group" } else { heading };
    let mut t = TextTable::new([
        group_heading.to_owned(),
        format!("Difference from {}", labels[control]),
        "Std error".to_owned(),
        "t".to_owned(),
        "Adjusted p".to_owned(),
        "Lower".to_owned(),
        "Upper".to_owned(),
        "Significant".to_owned(),
    ]);
    let bound = |x: f64| if x.is_finite() { format_number(x, DIGITS) } else { "—".to_owned() };
    for c in &d.comparisons {
        t.push([
            labels[treated[c.treatment]].clone(),
            format_number(c.difference, DIGITS),
            format_number(c.std_error, DIGITS),
            format_number(c.statistic, DIGITS),
            format_p_value(c.p_value),
            bound(c.confidence_interval.0),
            bound(c.confidence_interval.1),
            if c.p_value < 1.0 - level {
                format!("yes (p < {})", format_number(1.0 - level, 3))
            } else {
                "no".to_owned()
            },
        ]);
    }
    let side = match alternative {
        Alternative::TwoSided => "two-sided",
        Alternative::Greater => "one-sided: above control",
        Alternative::Less => "one-sided: below control",
    };
    let title = format!(
        "Comparisons with Control Group “{}” (Dunnett; {side}; simultaneous {}% intervals; critical value {}, error DF {})",
        labels[control],
        format_number(100.0 * level, 4),
        format_number(d.critical_value, DIGITS),
        format_number(d.df, 4),
    );
    // Interval plot: one column per treatment, the estimate as a marker,
    // the interval as a vertical line (clipped to the data for one-sided
    // intervals), and zero as a reference.
    let (mut lo_all, mut hi_all) = (0.0_f64, 0.0_f64);
    for c in &d.comparisons {
        let (lo, hi) = c.confidence_interval;
        for v in [c.difference, lo, hi].into_iter().filter(|v| v.is_finite()) {
            lo_all = lo_all.min(v);
            hi_all = hi_all.max(v);
        }
    }
    let pad = 0.1 * (hi_all - lo_all).max(f64::EPSILON);
    let segments = d
        .comparisons
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (lo, hi) = c.confidence_interval;
            let x = (i + 1) as f64;
            vec![
                [x, if lo.is_finite() { lo } else { lo_all - pad }],
                [x, if hi.is_finite() { hi } else { hi_all + pad }],
            ]
        })
        .collect();
    let items =
        d.comparisons.iter().enumerate().map(|(i, c)| ((i + 1) as f64, c.difference, String::new(), 0)).collect();
    let spec = PlotSpec {
        x_time: None,
        y_time: None,
        x_label: group_heading.to_owned(),
        y_label: format!("Difference in {value_name} from {}", labels[control]),
        points: Vec::new(),
        line: Some(vec![[0.5, 0.0], [d.comparisons.len() as f64 + 0.5, 0.0]]),
        line_name: Some("No difference".to_owned()),
        band: None,
        groups: None,
        x_ticks: Some(treated.iter().enumerate().map(|(i, &g)| ((i + 1) as f64, labels[g].clone())).collect()),
        panes: Vec::new(),
        annotations: Some(Annotations { groups: vec!["Difference".to_owned()], items, labels: Vec::new() }),
        segments,
        ..PlotSpec::default()
    };
    vec![
        section(SectionKind::ControlComparisons, title, SectionContent::Table(t)),
        section(
            SectionKind::ControlComparisonPlot,
            "Differences from Control (marker: difference; line: simultaneous interval; horizontal line: no difference)",
            SectionContent::Plot(Box::new(spec)),
        ),
    ]
}

/// Hsu's comparisons of each group with the best of the others: a table of
/// each group's mean, its difference from the best other mean and the
/// constrained simultaneous interval, and an interval plot with a zero line.
pub(crate) fn best_sections(
    heading: &str,
    value_name: &str,
    labels: &[String],
    groups: &[Vec<f64>],
    level: f64,
    direction: BestDirection,
) -> Vec<ReportSection> {
    let d = match comparisons_with_best(groups, level, direction) {
        Ok(d) => d,
        Err(e) => {
            return vec![section(
                SectionKind::BestComparisons,
                "Comparisons with the Best",
                SectionContent::Note(format!("Not available: {e}")),
            )];
        }
    };
    let group_heading = if heading.is_empty() { "Group" } else { heading };
    let other = match direction {
        BestDirection::Largest => "Mean − largest other mean",
        BestDirection::Smallest => "Mean − smallest other mean",
    };
    let mut t = TextTable::new([
        group_heading.to_owned(),
        "N".to_owned(),
        "Mean".to_owned(),
        other.to_owned(),
        "Critical value".to_owned(),
        "Lower".to_owned(),
        "Upper".to_owned(),
        "Conclusion".to_owned(),
    ]);
    for c in &d.comparisons {
        t.push([
            labels[c.group].clone(),
            c.n.to_string(),
            format_number(c.mean, DIGITS),
            format_number(c.difference, DIGITS),
            format_number(c.critical_value, DIGITS),
            format_number(c.lower, DIGITS),
            format_number(c.upper, DIGITS),
            c.verdict.label().to_owned(),
        ]);
    }
    let title = format!(
        "Comparisons with the Best (Hsu; {}; simultaneous {}% intervals for the difference from the best of the \
         others; error DF {})",
        direction.label(),
        format_number(100.0 * level, 4),
        format_number(d.df, 4),
    );
    let (mut lo_all, mut hi_all) = (0.0_f64, 0.0_f64);
    for c in &d.comparisons {
        for v in [c.difference, c.lower, c.upper] {
            lo_all = lo_all.min(v);
            hi_all = hi_all.max(v);
        }
    }
    let segments = d
        .comparisons
        .iter()
        .enumerate()
        .map(|(i, c)| vec![[(i + 1) as f64, c.lower], [(i + 1) as f64, c.upper]])
        .collect();
    let items =
        d.comparisons.iter().enumerate().map(|(i, c)| ((i + 1) as f64, c.difference, String::new(), 0)).collect();
    let spec = PlotSpec {
        x_time: None,
        y_time: None,
        x_label: group_heading.to_owned(),
        y_label: format!("{value_name}: {}", other.to_lowercase()),
        points: Vec::new(),
        line: Some(vec![[0.5, 0.0], [d.comparisons.len() as f64 + 0.5, 0.0]]),
        line_name: Some("No difference".to_owned()),
        band: None,
        groups: None,
        x_ticks: Some(
            d.comparisons.iter().enumerate().map(|(i, c)| ((i + 1) as f64, labels[c.group].clone())).collect(),
        ),
        panes: Vec::new(),
        annotations: Some(Annotations { groups: vec!["Difference".to_owned()], items, labels: Vec::new() }),
        segments,
        ..PlotSpec::default()
    };
    vec![
        section(SectionKind::BestComparisons, title, SectionContent::Table(t)),
        section(
            SectionKind::BestComparisonPlot,
            match direction {
                BestDirection::Largest => {
                    "Differences from the Best of the Others (marker: difference; line: simultaneous interval; an \
                     interval ending at 0 from below: not the best; from above: the best)"
                }
                BestDirection::Smallest => {
                    "Differences from the Best of the Others (marker: difference; line: simultaneous interval; an \
                     interval ending at 0 from above: not the best; from below: the best)"
                }
            },
            SectionContent::Plot(Box::new(spec)),
        ),
    ]
}

impl GroupComparison {
    fn pairwise_section(&self, p: &Result<Vec<PairwiseComparison>>) -> ReportSection {
        let method = self.request.method;
        let content = match p {
            Err(e) => SectionContent::Note(format!("Not available: {e}")),
            Ok(list) => SectionContent::Table(pairwise_table(&self.labels, list, method, self.request.level)),
        };
        section(SectionKind::PairwiseComparisons, pairwise_title(method, self.request.level), content)
    }
}

/// Title of an all-pairs section: the method and what its intervals are.
pub(crate) fn pairwise_title(method: ComparisonMethod, level: f64) -> String {
    let unequal = if method.unequal_variances() { "unequal variances; " } else { "" };
    format!("All Pairs: {} ({unequal}{})", method.label(), method.interval_note(level))
}

/// The all-pairs table; a pair is marked significant when its adjusted p is
/// below `1 − level`, the level of the table's intervals.
pub(crate) fn pairwise_table(
    labels: &[String],
    list: &[PairwiseComparison],
    method: ComparisonMethod,
    level: f64,
) -> TextTable {
    let stat = method.statistic_symbol();
    let mut t = TextTable::new([
        "Pair".to_owned(),
        "Difference".to_owned(),
        "Std error".to_owned(),
        stat.to_owned(),
        "DF".to_owned(),
        "Adjusted p".to_owned(),
        "Lower".to_owned(),
        "Upper".to_owned(),
        "Significant".to_owned(),
    ]);
    for c in list {
        t.push([
            format!("{} − {}", labels[c.first], labels[c.second]),
            format_number(c.difference, DIGITS),
            format_number(c.std_error, DIGITS),
            format_number(c.statistic, DIGITS),
            if c.df.is_finite() { format_number(c.df, 4) } else { String::new() },
            format_p_value(c.p_value),
            format_number(c.confidence_interval.0, DIGITS),
            format_number(c.confidence_interval.1, DIGITS),
            if c.p_value < 1.0 - level {
                format!("yes (p < {})", format_number(1.0 - level, 3))
            } else {
                "no".to_owned()
            },
        ]);
    }
    t
}

// ──────────────────────────── Paired / repeated ────────────────────────────

/// Where paired or repeated measurements are.
#[derive(Debug, Clone, PartialEq)]
pub enum PairedLayout {
    /// One numeric column per condition; each row is one subject.
    Columns(Vec<usize>),
    /// One data column, a condition column and a subject (ID) column.
    Stacked {
        /// Numeric data column.
        response: usize,
        /// Condition (treatment, time point) column.
        condition: usize,
        /// Subject identifier column.
        subject: usize,
    },
}

/// Settings of a paired comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct PairedRequest {
    /// Data layout.
    pub layout: PairedLayout,
    /// Hypothesized mean difference (first − second condition; two conditions).
    pub null_difference: f64,
    /// Alternative hypothesis (two conditions).
    pub alternative: Alternative,
    /// Confidence level.
    pub level: f64,
    /// Equivalence bounds for the mean difference (two conditions).
    pub equivalence: Option<(f64, f64)>,
    /// Test equivalence of the ratio first/second (geometric means; the
    /// bounds are ratios, e.g. 0.8 and 1.25) instead of the difference.
    pub ratio_equivalence: bool,
}

impl PairedRequest {
    /// Default settings for `layout`.
    #[must_use]
    pub fn new(layout: PairedLayout) -> Self {
        Self {
            layout,
            null_difference: 0.0,
            alternative: Alternative::TwoSided,
            level: 0.95,
            equivalence: None,
            ratio_equivalence: false,
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        match &self.layout {
            PairedLayout::Columns(c) => c.clone(),
            PairedLayout::Stacked { response, condition, subject } => vec![*response, *condition, *subject],
        }
    }
}

/// A paired or repeated-measures comparison.
#[derive(Debug, Clone)]
pub struct PairedComparison {
    request: PairedRequest,
    value_name: String,
    conditions: Vec<String>,
    /// Per subject: label, and `(row, value)` per condition.
    subjects: Vec<(String, Vec<(usize, f64)>)>,
    n_dropped: usize,
}

impl PairedComparison {
    /// Gather complete subjects.
    ///
    /// # Errors
    /// Invalid or non-numeric columns, fewer than two conditions, a subject
    /// measured twice under one condition, or fewer than two complete subjects.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &PairedRequest) -> Result<Self> {
        let mut dropped = 0;
        let (value_name, conditions, subjects) = match &request.layout {
            PairedLayout::Columns(cols) => {
                if cols.len() < 2 {
                    return Err(invalid("choose at least two columns"));
                }
                let data: Vec<&[f64]> = cols.iter().map(|&c| numeric(table, c)).collect::<Result<_>>()?;
                let names: Vec<String> = cols.iter().map(|&c| column_name(table, c)).collect::<Result<_>>()?;
                let mut subjects = Vec::new();
                for r in 0..table.n_rows() {
                    if states.is_excluded(r) {
                        continue;
                    }
                    if data.iter().all(|d| d[r].is_finite()) {
                        subjects.push((format!("Row {}", r + 1), data.iter().map(|d| (r, d[r])).collect()));
                    } else if data.iter().any(|d| d[r].is_finite()) {
                        dropped += 1;
                    }
                }
                ("Value".to_owned(), names, subjects)
            }
            PairedLayout::Stacked { response, condition, subject } => {
                if response == condition || response == subject || condition == subject {
                    return Err(invalid("the data, condition and subject columns must all differ"));
                }
                let (conds, members) = split_by_group(table, states, *response, Some(*condition))?;
                if conds.len() < 2 {
                    return Err(invalid("the condition column needs at least two levels"));
                }
                let subj_col = table.column(*subject).ok_or_else(|| invalid("no such column"))?;
                let mut ids: Vec<String> = Vec::new();
                let mut cells: Vec<Vec<Option<(usize, f64)>>> = Vec::new();
                for (c, m) in members.iter().enumerate() {
                    for &(row, v) in m {
                        let id = subj_col.level_text(row);
                        if id.is_empty() {
                            continue;
                        }
                        let s = ids.iter().position(|x| *x == id).unwrap_or_else(|| {
                            ids.push(id.clone());
                            cells.push(vec![None; conds.len()]);
                            ids.len() - 1
                        });
                        if cells[s][c].is_some() {
                            return Err(invalid(format!(
                                "subject {id:?} has more than one value for condition {:?}",
                                conds[c]
                            )));
                        }
                        cells[s][c] = Some((row, v));
                    }
                }
                let mut subjects = Vec::new();
                for (id, row) in ids.into_iter().zip(cells) {
                    if row.iter().all(Option::is_some) {
                        subjects.push((id, row.into_iter().flatten().collect()));
                    } else {
                        dropped += 1;
                    }
                }
                subjects.sort_by(|a, b| natural_order(&a.0, &b.0));
                (column_name(table, *response)?, conds, subjects)
            }
        };
        if subjects.len() < 2 {
            return Err(invalid("fewer than two subjects have a value for every condition"));
        }
        Ok(Self { request: request.clone(), value_name, conditions, subjects, n_dropped: dropped })
    }

    /// Number of complete subjects.
    #[must_use]
    pub fn n_subjects(&self) -> usize {
        self.subjects.len()
    }

    fn column(&self, c: usize) -> Vec<f64> {
        self.subjects.iter().map(|s| s.1[c].1).collect()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        if self.conditions.len() == 2 {
            format!("Paired Comparison: {} vs {}", self.conditions[0], self.conditions[1])
        } else {
            format!("Repeated Measures: {}", self.conditions.join(", "))
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let r = &self.request;
        let k = self.conditions.len();
        let cols: Vec<Vec<f64>> = (0..k).map(|c| self.column(c)).collect();
        let mut summary = summary_table(Some("Condition"), &self.conditions, &cols, r.level);
        if k == 2 {
            let d: Vec<f64> = cols[0].iter().zip(&cols[1]).map(|(a, b)| a - b).collect();
            let t2 = summary_table(
                Some("Condition"),
                &[format!("Difference ({} − {})", self.conditions[0], self.conditions[1])],
                &[d],
                r.level,
            );
            summary.rows.extend(t2.rows);
        }
        let dropped = if self.n_dropped > 0 {
            format!(" — {} subjects with a missing value left out", self.n_dropped)
        } else {
            String::new()
        };
        let mut out = vec![section(
            SectionKind::ComparisonSummary,
            format!("Summary ({} complete subjects{dropped})", self.subjects.len()),
            SectionContent::Table(summary),
        )];
        if k == 2 {
            out.extend(self.two_conditions(&cols[0], &cols[1]));
        } else {
            out.extend(self.several_conditions(&cols));
        }
        out
    }

    fn two_conditions(&self, a: &[f64], b: &[f64]) -> Vec<ReportSection> {
        let r = &self.request;
        let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
        let mut tests = TextTable::new(test_columns(None));
        push_test(&mut tests, None, "Paired t", paired_t(a, b, r.null_difference, r.level, r.alternative));
        push_test(
            &mut tests,
            None,
            "Wilcoxon signed-rank",
            paired_signed_rank(a, b, r.null_difference, r.level, r.alternative),
        );
        push_test(&mut tests, None, "Sign test", sign_test(&d, r.null_difference, r.level, r.alternative));
        let parameter = format!("mean of {} − {}", self.conditions[0], self.conditions[1]);
        let mut out = vec![section(
            SectionKind::ComparisonTests,
            format!(
                "Paired Tests ({}; signed-rank estimate: Hodges–Lehmann; sign-test estimate: median difference)",
                hypothesis_note(r.alternative, r.level, &parameter, r.null_difference)
            ),
            SectionContent::Table(tests),
        )];
        let mut es = TextTable::new(["Measure", "Value"]);
        es.push([
            "Standardized mean difference d_z".to_owned(),
            paired_effect_size(&d).map_or("—".to_owned(), |v| format_number(v, 4)),
        ]);
        if let Ok(p) = stat_core::correlation::pearson(a, b, r.level) {
            es.push(["Correlation between conditions".to_owned(), format_number(p.correlation.estimate, 4)]);
        }
        out.push(section(SectionKind::EffectSizes, "Effect Size", SectionContent::Table(es)));
        if let Some(bounds) = r.equivalence {
            let alpha = (1.0 - r.level) / 2.0;
            let (title, test) = if r.ratio_equivalence {
                (
                    format!(
                        "Equivalence of the ratio {} / {} (geometric means; TOST on logs at α = {}; estimates and \
                         interval as ratios)",
                        self.conditions[0],
                        self.conditions[1],
                        format_number(alpha, 4)
                    ),
                    tost_paired_ratio(a, b, bounds, alpha).map(|e| ratio_scale(&e)),
                )
            } else {
                (
                    format!("Equivalence of the mean difference (TOST at α = {})", format_number(alpha, 4)),
                    tost_paired(a, b, bounds, alpha),
                )
            };
            out.push(section(
                SectionKind::EquivalenceTests,
                title,
                SectionContent::Table(equivalence_table(None, vec![(None, test)])),
            ));
        }
        // Difference against mean (Tukey mean-difference / Bland–Altman plot).
        let n = d.len() as f64;
        let mean_d = d.iter().sum::<f64>() / n;
        let sd_d = (d.iter().map(|v| (v - mean_d).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
        let points: Vec<RowPoint> = self
            .subjects
            .iter()
            .map(|(_, v)| RowPoint { row: v[0].0, x: 0.5 * (v[0].1 + v[1].1), y: v[0].1 - v[1].1 })
            .collect();
        let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(p.x), h.max(p.x)));
        let hline = |y: f64| vec![[lo, y], [hi, y]];
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: format!("Mean of {} and {}", self.conditions[0], self.conditions[1]),
            y_label: format!("{} − {}", self.conditions[0], self.conditions[1]),
            points,
            line: Some(hline(mean_d)),
            line_name: Some("Mean difference".to_owned()),
            band: Some((hline(mean_d - 1.96 * sd_d), hline(mean_d + 1.96 * sd_d))),
            band_name: Some("Limits of agreement (±1.96 SD)".to_owned()),
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: vec![hline(r.null_difference)],
            ..PlotSpec::default()
        };
        out.push(section(
            SectionKind::PairedDifferences,
            "Difference vs Mean (solid: mean difference; dashed: ±1.96 SD limits of agreement; thin: hypothesized difference)",
            SectionContent::Plot(Box::new(spec)),
        ));
        out
    }

    fn several_conditions(&self, cols: &[Vec<f64>]) -> Vec<ReportSection> {
        let r = &self.request;
        let k = cols.len();
        let rows: Vec<Vec<f64>> = (0..self.subjects.len()).map(|i| cols.iter().map(|c| c[i]).collect()).collect();
        let mut out = Vec::new();
        let mut t = TextTable::new(["Test", "Statistic", "DF", "p-value"]);
        match repeated_measures_anova(&rows) {
            Ok(a) => rm_rows(&mut t, &a),
            Err(e) => t.push([
                "Repeated-measures ANOVA".to_owned(),
                format!("not available: {e}"),
                String::new(),
                String::new(),
            ]),
        }
        match friedman_test(&rows) {
            Ok(ChiSquareResult { statistic, df, p_value }) => t.push([
                "Friedman (ranks within subjects)".to_owned(),
                format!("χ² = {}", format_number(statistic, DIGITS)),
                format_number(df, 4),
                format_p_value(p_value),
            ]),
            Err(e) => t.push(["Friedman".to_owned(), format!("not available: {e}"), String::new(), String::new()]),
        }
        out.push(section(
            SectionKind::RepeatedMeasures,
            "Tests of Equal Conditions (within subjects; a small Mauchly p-value means sphericity fails: use a \
             corrected test)",
            SectionContent::Table(t),
        ));
        // Pairwise paired t tests, Holm-adjusted.
        let mut list = Vec::new();
        for i in 0..k {
            for j in i + 1..k {
                if let Ok(t) = paired_t(&cols[i], &cols[j], 0.0, r.level, Alternative::TwoSided) {
                    list.push(PairwiseComparison {
                        first: i,
                        second: j,
                        difference: t.estimate,
                        std_error: t.std_error,
                        statistic: t.statistic,
                        df: t.df.unwrap_or(f64::NAN),
                        p_value: t.p_value,
                        confidence_interval: t.confidence_interval,
                    });
                }
            }
        }
        let adjusted = holm_adjust(&list.iter().map(|c| c.p_value).collect::<Vec<_>>());
        for (c, p) in list.iter_mut().zip(adjusted) {
            c.p_value = p;
        }
        out.push(section(
            SectionKind::PairwiseComparisons,
            "All Pairs of Conditions: paired t, Holm-adjusted p (unadjusted intervals)",
            SectionContent::Table(pairwise_table(&self.conditions, &list, ComparisonMethod::HolmT, r.level)),
        ));
        // Profiles: one thin line per subject across conditions; means as the fit line.
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let mut segments = Vec::new();
        for (_, v) in &self.subjects {
            segments.push(v.iter().enumerate().map(|(c, &(_, y))| [(c + 1) as f64, y]).collect());
            for (c, &(row, y)) in v.iter().enumerate() {
                points.push(RowPoint { row, x: (c + 1) as f64, y });
                membership.push(c);
            }
        }
        let means: Vec<[f64; 2]> =
            cols.iter().enumerate().map(|(c, x)| [(c + 1) as f64, x.iter().sum::<f64>() / x.len() as f64]).collect();
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "Condition".to_owned(),
            y_label: self.value_name.clone(),
            points,
            line: Some(means),
            line_name: Some("Condition means".to_owned()),
            band: None,
            groups: Some(PointGroups {
                title: "Condition".to_owned(),
                labels: self.conditions.clone(),
                membership,
                lines: Vec::new(),
            }),
            x_ticks: Some(self.conditions.iter().enumerate().map(|(c, l)| ((c + 1) as f64, l.clone())).collect()),
            panes: Vec::new(),
            annotations: None,
            segments,
            ..PlotSpec::default()
        };
        out.push(section(
            SectionKind::ValuesByGroup,
            "Subject Profiles (thin lines: subjects; thick line: condition means)",
            SectionContent::Plot(Box::new(spec)),
        ));
        out
    }
}

fn rm_rows(t: &mut TextTable, a: &RepeatedMeasuresAnova) {
    let f = format!("F = {}", format_number(a.f_statistic, DIGITS));
    t.push([
        "Repeated-measures ANOVA (sphericity assumed)".to_owned(),
        f.clone(),
        format!("{}, {}", format_number(a.df_conditions, 4), format_number(a.df_error, 4)),
        format_p_value(a.p_value),
    ]);
    let e = a.greenhouse_geisser;
    t.push([
        format!("Greenhouse–Geisser corrected (ε = {})", format_number(e, 4)),
        f.clone(),
        format!("{}, {}", format_number(e * a.df_conditions, 4), format_number(e * a.df_error, 4)),
        format_p_value(a.p_value_gg),
    ]);
    if let Some(s) = &a.sphericity {
        let h = s.huynh_feldt_capped();
        let raw = if s.huynh_feldt > 1.0 {
            format!("ε = 1; estimate {}", format_number(s.huynh_feldt, 4))
        } else {
            format!("ε = {}", format_number(h, 4))
        };
        t.push([
            format!("Huynh–Feldt corrected ({raw})"),
            f,
            format!("{}, {}", format_number(h * a.df_conditions, 4), format_number(h * a.df_error, 4)),
            format_p_value(a.p_value_hf),
        ]);
        t.push([
            "Mauchly's test of sphericity".to_owned(),
            if s.mauchly_w.is_finite() {
                format!("W = {}, χ² = {}", format_number(s.mauchly_w, 4), format_number(s.chi_square, DIGITS))
            } else {
                "not available: fewer subjects than conditions".to_owned()
            },
            format_number(s.df, 4),
            if s.p_value.is_finite() { format_p_value(s.p_value) } else { String::new() },
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// A control group named "Control" read "Comparisons with Control
    /// Control"; and a grouping column with hundreds of levels stalled the
    /// report for minutes, so beyond the limit the section is a note.
    #[test]
    fn control_title_and_group_limit() {
        let labels: Vec<String> = ["Control", "A", "B"].map(str::to_owned).to_vec();
        let groups = vec![vec![1.0, 2.0, 3.0], vec![2.0, 3.0, 5.0], vec![4.0, 6.0, 5.0]];
        let out = control_sections("Group", "y", &labels, &groups, 0, 0.95, Alternative::TwoSided);
        assert!(out[0].title.starts_with("Comparisons with Control Group “Control” (Dunnett"), "{}", out[0].title);
        let many = MAX_CONTROL_COMPARISONS + 2;
        let labels: Vec<String> = (0..many).map(|g| format!("G{g}")).collect();
        let groups: Vec<Vec<f64>> = (0..many).map(|g| (0..=(g % 9)).map(|i| (g * i) as f64).collect()).collect();
        let start = std::time::Instant::now();
        let out = control_sections("Group", "y", &labels, &groups, 0, 0.95, Alternative::TwoSided);
        assert!(start.elapsed().as_secs() < 5);
        let SectionContent::Note(text) = &out[0].content else { panic!("expected a note") };
        assert!(text.contains("the limit is 100"), "{text}");
    }

    const SLEEP_1: [f64; 10] = [0.7, -1.6, -0.2, -1.2, -0.1, 3.4, 3.7, 0.8, 0.0, 2.0];
    const SLEEP_2: [f64; 10] = [1.9, 0.8, 1.1, 0.1, -0.1, 4.4, 5.5, 1.6, 4.6, 3.4];

    /// The sleep data stacked (extra, group, ID) as in R's `sleep` dataset,
    /// with side-by-side copies in two more columns.
    fn sleep() -> DataTable {
        let extra: Vec<f64> = SLEEP_1.iter().chain(&SLEEP_2).copied().collect();
        let group: Vec<String> = (0..20).map(|i| if i < 10 { "1" } else { "2" }.to_owned()).collect();
        let id: Vec<String> = (0..20).map(|i| format!("P{}", i % 10 + 1)).collect();
        let mut drug_a = SLEEP_1.to_vec();
        drug_a.extend([f64::NAN; 10]);
        let mut drug_b = SLEEP_2.to_vec();
        drug_b.extend([f64::NAN; 10]);
        DataTable::new(
            "sleep",
            vec![
                Column::numeric("extra", extra),
                Column::text("group", group),
                Column::text("ID", id),
                Column::numeric("Drug A", drug_a),
                Column::numeric("Drug B", drug_b),
            ],
        )
        .unwrap()
    }

    fn table_of(s: &[ReportSection], kind: SectionKind) -> &TextTable {
        match &s.iter().find(|x| x.kind == kind).unwrap().content {
            SectionContent::Table(t) => t,
            _ => panic!("not a table"),
        }
    }

    /// The ratio-of-means option adds Fieller's interval for both variance
    /// assumptions (the limits themselves are checked in stat-inference).
    #[test]
    fn ratio_of_means_section() {
        let t = sleep();
        let mut req = GroupComparisonRequest::new(GroupLayout::Stacked { response: 0, group: 1 });
        let a = GroupComparison::run(&t, &RowStateManager::new(20), &req).unwrap();
        assert!(!a.sections().iter().any(|s| s.kind == SectionKind::RatioOfMeans));
        req.ratio_of_means = true;
        let a = GroupComparison::run(&t, &RowStateManager::new(20), &req).unwrap();
        let s = a.sections();
        let r = table_of(&s, SectionKind::RatioOfMeans);
        assert_eq!(r.rows.len(), 2);
        assert_eq!(r.rows[0][1], format_number(0.75 / 2.33, DIGITS));
        let f = ratio_of_means(&SLEEP_1, &SLEEP_2, 1.0, false, 0.95, Alternative::TwoSided).unwrap();
        let RatioLimits::Bounded(lo, hi) = f.limits else { panic!() };
        assert_eq!(r.rows[1][5], format_number(lo, DIGITS));
        assert_eq!(r.rows[1][6], format_number(hi, DIGITS));
    }

    #[test]
    fn stacked_and_side_by_side_groups_agree() {
        let t = sleep();
        let rows = RowStateManager::new(20);
        let stacked = GroupComparison::run(
            &t,
            &rows,
            &GroupComparisonRequest::new(GroupLayout::Stacked { response: 0, group: 1 }),
        )
        .unwrap();
        let wide =
            GroupComparison::run(&t, &rows, &GroupComparisonRequest::new(GroupLayout::Columns(vec![3, 4]))).unwrap();
        assert_eq!(stacked.title(), "Compare Groups: extra by group");
        assert_eq!(wide.title(), "Compare Columns: Drug A, Drug B");
        let (s1, s2) = (stacked.sections(), wide.sections());
        let (a, b) = (table_of(&s1, SectionKind::ComparisonTests), table_of(&s2, SectionKind::ComparisonTests));
        assert_eq!(a.rows, b.rows, "the same tests from either layout");
        // Welch: t = −1.8608, p = 0.07939 (published).
        assert_eq!(a.rows[0][0], "Two-sample t (Welch)");
        assert_eq!(a.rows[0][3], "t = -1.8608");
        assert_eq!(a.rows[0][5], "0.0794");
        assert!(s1.iter().any(|x| x.kind == SectionKind::EffectSizes));
        assert!(!s1.iter().any(|x| x.kind == SectionKind::PairwiseComparisons), "two groups: no multiple comparisons");
        let SectionContent::Plot(p) = &s1.last().unwrap().content else { panic!() };
        assert_eq!(p.points.len(), 20);
    }

    #[test]
    fn three_groups_with_multiple_comparisons_and_equivalence() {
        let a = [24.5, 23.5, 26.4, 27.1, 29.9];
        let b = [28.4, 34.2, 29.5, 32.2, 30.1];
        let c = [26.1, 28.3, 24.3, 26.2, 27.8];
        let y: Vec<f64> = a.iter().chain(&b).chain(&c).copied().collect();
        let g: Vec<String> = (0..15).map(|i| ["A", "B", "C"][i / 5].to_owned()).collect();
        let t = DataTable::new("t", vec![Column::numeric("y", y), Column::text("g", g)]).unwrap();
        let req = GroupComparisonRequest::new(GroupLayout::Stacked { response: 0, group: 1 });
        let r = GroupComparison::run(&t, &RowStateManager::new(15), &req).unwrap();
        let s = r.sections();
        let pairs = table_of(&s, SectionKind::PairwiseComparisons);
        assert_eq!(pairs.rows.len(), 3);
        assert_eq!(pairs.rows[0][0], "A − B");
        assert_eq!(pairs.rows[0][5], "0.0144", "Tukey p (SciPy tukey_hsd: 0.01445)");
        assert_eq!(pairs.rows[0][8], "yes (p < 0.05)");
        // The significance column follows the chosen level, as the intervals
        // do: at 99% the same p = 0.0144 is not significant.
        let strict = GroupComparisonRequest { level: 0.99, ..req.clone() };
        let r99 = GroupComparison::run(&t, &RowStateManager::new(15), &strict).unwrap();
        let pairs99 = table_of(&r99.sections(), SectionKind::PairwiseComparisons).clone();
        assert_eq!(pairs99.rows[0][8], "no");
        assert_eq!(table_of(&s, SectionKind::PairwiseComparisons).rows[0][5], pairs99.rows[0][5]);
        assert!(!s.iter().any(|x| x.kind == SectionKind::ComparisonTests));
        let req = GroupComparisonRequest { method: ComparisonMethod::Dunn, ..req };
        let r = GroupComparison::run(&t, &RowStateManager::new(15), &req).unwrap();
        assert_eq!(
            table_of(&r.sections(), SectionKind::PairwiseComparisons).rows[0][5],
            "0.0400",
            "Holm step-down keeps it at 3 × 0.01333"
        );
        // Equivalence for two groups (exclude group C).
        let mut rows = RowStateManager::new(15);
        rows.set_excluded(10..15, true);
        let req = GroupComparisonRequest {
            equivalence: Some((-5.0, 5.0)),
            ..GroupComparisonRequest::new(GroupLayout::Stacked { response: 0, group: 1 })
        };
        let r = GroupComparison::run(&t, &rows, &req).unwrap();
        let sections = r.sections();
        assert_eq!(table_of(&sections, SectionKind::EquivalenceTests).rows.len(), 2);
    }

    #[test]
    fn paired_from_both_layouts() {
        let t = sleep();
        let rows = RowStateManager::new(20);
        let stacked = PairedComparison::run(
            &t,
            &rows,
            &PairedRequest::new(PairedLayout::Stacked { response: 0, condition: 1, subject: 2 }),
        )
        .unwrap();
        let wide = PairedComparison::run(&t, &rows, &PairedRequest::new(PairedLayout::Columns(vec![3, 4]))).unwrap();
        assert_eq!((stacked.n_subjects(), wide.n_subjects()), (10, 10));
        let (s1, s2) = (stacked.sections(), wide.sections());
        let (a, b) = (table_of(&s1, SectionKind::ComparisonTests), table_of(&s2, SectionKind::ComparisonTests));
        assert_eq!(a.rows, b.rows);
        // Published: paired t = −4.0621, p = 0.002833; signed-rank V = 0, p = 0.009091.
        assert_eq!(a.rows[0][3], "t = -4.0621");
        assert_eq!(a.rows[0][5], "0.0028");
        assert_eq!(a.rows[1][3], "V = 0");
        assert_eq!(a.rows[1][5], "0.0091");
        let diff = s1.iter().find(|x| x.kind == SectionKind::PairedDifferences).unwrap();
        let SectionContent::Plot(p) = &diff.content else { panic!() };
        assert_eq!(p.line_label(), "Mean difference");
        assert_eq!(p.band_label(), "Limits of agreement (±1.96 SD)");
        // Published mean difference of the sleep data: −1.58 hours.
        assert!(p.line.as_ref().unwrap().iter().all(|q| (q[1] + 1.58).abs() < 1e-9));
        // Duplicated subject × condition is rejected.
        let mut dup = sleep();
        dup.set_value(1, 2, stat_data::CellValue::Text("P1".into())).unwrap();
        assert!(
            PairedComparison::run(
                &dup,
                &rows,
                &PairedRequest::new(PairedLayout::Stacked { response: 0, condition: 1, subject: 2 })
            )
            .is_err()
        );
    }

    #[test]
    fn repeated_measures_with_three_conditions() {
        let y = [[8.0, 7.2, 6.1], [9.1, 8.5, 7.0], [7.5, 7.9, 6.8], [8.8, 8.0, 7.9], [9.5, 8.1, 7.2], [7.9, 7.0, 7.1]];
        let cols: Vec<Column> =
            (0..3).map(|c| Column::numeric(format!("T{}", c + 1), y.iter().map(|r| r[c]).collect())).collect();
        let t = DataTable::new("rm", cols).unwrap();
        let r = PairedComparison::run(
            &t,
            &RowStateManager::new(6),
            &PairedRequest::new(PairedLayout::Columns(vec![0, 1, 2])),
        )
        .unwrap();
        assert_eq!(r.title(), "Repeated Measures: T1, T2, T3");
        let s = r.sections();
        let tests = table_of(&s, SectionKind::RepeatedMeasures);
        assert_eq!(tests.rows[0][1], "F = 14.854");
        assert_eq!(tests.rows[1][0], "Greenhouse–Geisser corrected (ε = 0.9429)");
        // Huynh–Feldt ε̃ = 1.4956 (R car) is capped at 1; Mauchly W = 0.93945,
        // p = 0.88258 (R car / mauchly.test).
        assert_eq!(tests.rows[2][0], "Huynh–Feldt corrected (ε = 1; estimate 1.496)");
        assert_eq!(tests.rows[2][2], "2, 10");
        assert_eq!(tests.rows[3][0], "Mauchly's test of sphericity");
        assert!(tests.rows[3][1].starts_with("W = 0.9395"), "{}", tests.rows[3][1]);
        assert_eq!(tests.rows[3][3], "0.8826");
        assert_eq!(tests.rows[4][1], "χ² = 8.3333", "Friedman (SciPy: 8.3333)");
        assert_eq!(table_of(&s, SectionKind::PairwiseComparisons).rows.len(), 3);
    }

    /// Hsu's comparisons with the best in the group report (engine values
    /// are checked in `stat_inference::best`).
    #[test]
    fn comparisons_with_the_best_section() {
        let t = sleep();
        let req = GroupComparisonRequest {
            best: Some(BestDirection::Largest),
            ..GroupComparisonRequest::new(GroupLayout::Stacked { response: 0, group: 1 })
        };
        let a = GroupComparison::run(&t, &RowStateManager::new(20), &req).unwrap();
        let s = a.sections();
        let best = table_of(&s, SectionKind::BestComparisons);
        assert_eq!(best.rows.len(), 2);
        // With two groups the interval for each is ±(difference ± t·SE), the
        // pooled t interval made one-sided at 0.
        assert!(s.iter().any(|x| x.kind == SectionKind::BestComparisonPlot));
        let verdicts: Vec<&str> = best.rows.iter().map(|r| r[7].as_str()).collect();
        assert!(verdicts.contains(&"may be the best") || verdicts.contains(&"the best"), "{verdicts:?}");
    }

    #[test]
    fn paired_ratio_equivalence_reports_ratios() {
        let extra: Vec<f64> = SLEEP_1.iter().chain(&SLEEP_2).map(|v| v + 10.0).collect();
        let shifted = DataTable::new(
            "sleep",
            vec![
                Column::numeric("extra", extra),
                Column::text("group", (0..20).map(|i| if i < 10 { "1" } else { "2" }.to_owned()).collect()),
                Column::text("ID", (0..20).map(|i| format!("P{}", i % 10 + 1)).collect()),
            ],
        )
        .unwrap();
        let req = PairedRequest {
            equivalence: Some((0.8, 1.25)),
            ratio_equivalence: true,
            ..PairedRequest::new(PairedLayout::Stacked { response: 0, condition: 1, subject: 2 })
        };
        let a = PairedComparison::run(&shifted, &RowStateManager::new(20), &req).unwrap();
        let s = a.sections();
        let eq = table_of(&s, SectionKind::EquivalenceTests);
        assert_eq!(eq.rows[0][1], "0.8", "bounds are shown as ratios");
        assert_eq!(eq.rows[0][2], "1.25");
    }

    #[test]
    fn one_sample_by_group() {
        let t = sleep();
        let req = OneSampleRequest {
            group: Some(1),
            known_std_dev: Some(2.0),
            null_std_dev: Some(1.5),
            equivalence: Some((-1.0, 3.0)),
            ..OneSampleRequest::new(0)
        };
        let a = OneSampleAnalysis::run(&t, &RowStateManager::new(20), &req).unwrap();
        assert_eq!(a.title(), "One-Sample Tests: extra by group");
        let s = a.sections();
        let tests = table_of(&s, SectionKind::ComparisonTests);
        assert_eq!(tests.rows.len(), 2 * 5, "t, z, signed-rank, sign, sd for each group");
        assert_eq!(tests.rows[0][..2], ["1".to_owned(), "One-sample t".to_owned()]);
        assert_eq!(table_of(&s, SectionKind::EquivalenceTests).rows.len(), 2);
        let ungrouped = OneSampleAnalysis::run(&t, &RowStateManager::new(20), &OneSampleRequest::new(3)).unwrap();
        assert_eq!(table_of(&ungrouped.sections(), SectionKind::ComparisonTests).rows.len(), 3);
    }
}
