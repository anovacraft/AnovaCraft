//! The joint distribution of two columns.
//!
//! * Two **continuous** columns: Pearson and Spearman correlations with tests and
//!   95% intervals, the covariance, and a brushable scatter plot.
//! * Two **categorical** columns: the contingency table with margins, expected
//!   counts, and Pearson / likelihood-ratio tests of independence with
//!   Cramér's V (and a warning when expected counts are too small for the χ²
//!   approximation).
//! * One **continuous** and one **categorical** column (either order): the
//!   continuous variable's mean, spread and 95% interval in each group, a one-way
//!   ANOVA of the group means, comparisons that relax its assumptions — pooled
//!   and Welch t-tests of the difference (two groups) or Welch's ANOVA (three or
//!   more), Levene / Brown–Forsythe / Bartlett tests of equal variances, and the
//!   Wilcoxon rank-sum (two groups) and Kruskal–Wallis rank tests
//!   ([`stat_linear::group_tests`]) — and the values plotted by group with each
//!   group's mean marked.
//!
//! Rows missing either value, and excluded rows, are left out.

use crate::format::{format_number, format_optional, format_p_value};
use crate::report::{PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::correlation::{Correlation, PearsonSummary, pearson, spearman};
use stat_core::{Result, StatError};
use stat_data::{DataTable, MeasurementLevel, RowStateManager};
use stat_discrete::{ContingencyTable, IndependenceTest, test_independence};
use stat_inference::multiple::{ComparisonMethod, compare_all_pairs};
use stat_linear::{
    OneWayAnova, kruskal_wallis, natural_order, one_way_anova, rank_sum_test, two_sample_t_tests, variance_tests,
    welch_anova,
};

const DIGITS: usize = 6;

/// Confidence of the simultaneous pairwise intervals.
const PAIRWISE_LEVEL: f64 = 0.95;

/// Which two columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BivariateRequest {
    /// Horizontal / row variable.
    pub x: usize,
    /// Vertical / column variable.
    pub y: usize,
    /// Comparisons of every pair of groups when one column is continuous and
    /// the other categorical (`None`: off; Tukey–Kramer by default).
    pub pairwise: Option<ComparisonMethod>,
}

impl BivariateRequest {
    /// The two columns, with the default Tukey–Kramer pairwise comparisons.
    #[must_use]
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y, pairwise: Some(ComparisonMethod::TukeyKramer) }
    }
}

#[derive(Debug, Clone)]
enum Body {
    Continuous {
        points: Vec<RowPoint>,
        pearson: PearsonSummary,
        spearman: Correlation,
    },
    Categorical {
        table: ContingencyTable,
        test: IndependenceTest,
    },
    /// A continuous response compared across the levels of a categorical column.
    Grouped {
        /// Group labels, in order of first appearance.
        levels: Vec<String>,
        /// (row, value) for each group.
        members: Vec<Vec<(usize, f64)>>,
        anova: OneWayAnova,
    },
}

/// A computed bivariate analysis.
#[derive(Debug, Clone)]
pub struct BivariateAnalysis {
    x_name: String,
    y_name: String,
    /// Date-time displays of the two columns (`None` for numeric columns).
    x_time: Option<stat_data::TemporalDisplay>,
    y_time: Option<stat_data::TemporalDisplay>,
    /// For a grouped analysis: (continuous column name, grouping column name).
    grouped_names: Option<(String, String)>,
    body: Body,
    /// Pairwise comparison method of a grouped analysis.
    pairwise: Option<ComparisonMethod>,
    n_skipped: usize,
    exclusion_revision: u64,
}

fn is_continuous(table: &DataTable, i: usize) -> Option<bool> {
    table.column(i).map(|c| c.as_numeric().is_some() && c.level == MeasurementLevel::Continuous)
}

impl BivariateAnalysis {
    /// Analyse the pair, honouring row exclusion.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for invalid or identical columns, or one
    /// continuous and one categorical column (not yet supported); errors from the
    /// underlying computations (too few rows, constant variables, …).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &BivariateRequest) -> Result<Self> {
        let invalid = |m: String| StatError::InvalidArgument { message: m };
        if request.x == request.y {
            return Err(invalid("choose two different columns".into()));
        }
        let (cx, cy) = match (table.column(request.x), table.column(request.y)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(invalid("no such column".into())),
        };
        let kinds =
            (is_continuous(table, request.x).unwrap_or(false), is_continuous(table, request.y).unwrap_or(false));
        let usable = |r: usize| !states.is_excluded(r);
        let (body, n_skipped) = match kinds {
            (true, true) => {
                let (xs, ys) = (cx.as_numeric().unwrap_or(&[]), cy.as_numeric().unwrap_or(&[]));
                let points: Vec<RowPoint> = (0..table.n_rows())
                    .filter(|&r| usable(r) && xs[r].is_finite() && ys[r].is_finite())
                    .map(|r| RowPoint { row: r, x: xs[r], y: ys[r] })
                    .collect();
                let (x, y): (Vec<f64>, Vec<f64>) = points.iter().map(|p| (p.x, p.y)).unzip();
                let pearson = pearson(&x, &y, 0.95)?;
                let spearman = spearman(&x, &y, 0.95)?;
                let skipped = table.n_rows() - points.len();
                (Body::Continuous { points, pearson, spearman }, skipped)
            }
            (false, false) => {
                let (mut rows, mut cols) = (Vec::new(), Vec::new());
                for r in 0..table.n_rows() {
                    let (a, b) = (cx.level_text(r), cy.level_text(r));
                    if usable(r) && !a.is_empty() && !b.is_empty() {
                        rows.push(a);
                        cols.push(b);
                    }
                }
                let skipped = table.n_rows() - rows.len();
                let contingency = ContingencyTable::from_pairs(&rows, &cols)?;
                let test = test_independence(&contingency)?;
                (Body::Categorical { table: contingency, test }, skipped)
            }
            (x_continuous, _) => {
                // One continuous, one categorical: compare the continuous values
                // across the categorical column's levels.
                let (values_col, group_col) = if x_continuous { (cx, cy) } else { (cy, cx) };
                let values = values_col.as_numeric().unwrap_or(&[]);
                let mut levels: Vec<String> = Vec::new();
                let mut members: Vec<Vec<(usize, f64)>> = Vec::new();
                for r in 0..table.n_rows() {
                    let label = group_col.level_text(r);
                    if !usable(r) || label.is_empty() || !values[r].is_finite() {
                        continue;
                    }
                    let g = levels.iter().position(|l| *l == label).unwrap_or_else(|| {
                        levels.push(label);
                        members.push(Vec::new());
                        levels.len() - 1
                    });
                    members[g].push((r, values[r]));
                }
                // Groups in natural order, as Compare Groups lists them.
                let mut order: Vec<usize> = (0..levels.len()).collect();
                order.sort_by(|&a, &b| natural_order(&levels[a], &levels[b]));
                let levels: Vec<String> = order.iter().map(|&g| levels[g].clone()).collect();
                let members: Vec<Vec<(usize, f64)>> = order.iter().map(|&g| std::mem::take(&mut members[g])).collect();
                let groups: Vec<Vec<f64>> = members.iter().map(|m| m.iter().map(|v| v.1).collect()).collect();
                let anova = one_way_anova(&groups)?;
                let used: usize = members.iter().map(Vec::len).sum();
                let skipped = table.n_rows() - used;
                (Body::Grouped { levels, members, anova }, skipped)
            }
        };
        let grouped_names = match (&body, kinds.0) {
            (Body::Grouped { .. }, true) => Some((cx.name.clone(), cy.name.clone())),
            (Body::Grouped { .. }, false) => Some((cy.name.clone(), cx.name.clone())),
            _ => None,
        };
        Ok(Self {
            x_name: cx.name.clone(),
            y_name: cy.name.clone(),
            x_time: cx.temporal(),
            y_time: cy.temporal(),
            grouped_names,
            body,
            pairwise: request.pairwise,
            n_skipped,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// `true` if row exclusion changed since the analysis ran.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// `true` when one column is continuous and the other categorical, so the
    /// report compares groups (and offers pairwise comparisons).
    #[must_use]
    pub fn is_grouped(&self) -> bool {
        matches!(self.body, Body::Grouped { .. })
    }

    /// Change the pairwise comparison method (computed from the stored
    /// groups, so the report need not be re-run).
    pub fn set_pairwise(&mut self, method: Option<ComparisonMethod>) {
        self.pairwise = method;
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.grouped_names {
            Some((values, groups)) => format!("Bivariate: {values} by {groups}"),
            None => format!("Bivariate: {} by {}", self.y_name, self.x_name),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        match &self.body {
            Body::Continuous { points, pearson, spearman } => {
                vec![self.correlation_table(pearson, spearman), self.scatter(points)]
            }
            Body::Categorical { table, test } => {
                let mut out =
                    vec![counts_table(table, &self.x_name), self.tests_table(test, table), association(test, table)];
                out.push(residuals_table(table, &self.x_name));
                out.extend(proportion_differences(table, &self.y_name));
                out
            }
            Body::Grouped { levels, members, anova } => {
                let values: Vec<Vec<f64>> = members.iter().map(|m| m.iter().map(|v| v.1).collect()).collect();
                let mut out = vec![self.group_means_table(levels, anova), self.anova_table(anova)];
                if values.len() == 2 {
                    out.push(mean_difference_table(levels, &values));
                } else {
                    out.push(welch_table(&values));
                }
                out.push(equal_variance_table(&values));
                if let Some(method) = self.pairwise.filter(|_| values.len() >= 3) {
                    out.push(pairwise_section(levels, &values, method));
                }
                out.push(rank_test_table(&values));
                out.push(self.values_by_group(levels, members, anova));
                out
            }
        }
    }

    fn names(&self) -> (&str, &str) {
        self.grouped_names.as_ref().map_or((&self.y_name, &self.x_name), |(v, g)| (v.as_str(), g.as_str()))
    }

    fn group_means_table(&self, levels: &[String], a: &OneWayAnova) -> ReportSection {
        let (_, group_name) = self.names();
        let mut t =
            TextTable::new([group_name, "N", "Mean", "Std Dev", "Std Error (pooled)", "Lower 95%", "Upper 95%"]);
        for (label, g) in levels.iter().zip(&a.groups) {
            t.push([
                label.clone(),
                g.n.to_string(),
                format_number(g.mean, DIGITS),
                format_optional(g.std_dev, DIGITS),
                format_number(g.pooled_std_error, DIGITS),
                format_number(g.confidence_interval.0, DIGITS),
                format_number(g.confidence_interval.1, DIGITS),
            ]);
        }
        let blank = String::new;
        t.push([
            "All".to_owned(),
            a.n_total.to_string(),
            format_number(a.grand_mean, DIGITS),
            blank(),
            blank(),
            blank(),
            blank(),
        ]);
        if self.n_skipped > 0 {
            t.push([
                "Rows not used".to_owned(),
                self.n_skipped.to_string(),
                blank(),
                blank(),
                blank(),
                blank(),
                blank(),
            ]);
        }
        section(SectionKind::GroupMeans, "Means by Group", SectionContent::Table(t))
    }

    fn anova_table(&self, a: &OneWayAnova) -> ReportSection {
        let (_, group_name) = self.names();
        let mut t = TextTable::new(["Source", "DF", "Sum of Squares", "Mean Square", "F", "p-value"]);
        t.push([
            format!("Between groups ({group_name})"),
            a.df_between.to_string(),
            format_number(a.ss_between, DIGITS),
            format_number(a.ms_between, DIGITS),
            format_optional(a.f_statistic, DIGITS),
            a.p_value.map_or_else(|| "—".to_owned(), format_p_value),
        ]);
        t.push([
            "Within groups (error)".to_owned(),
            a.df_within.to_string(),
            format_number(a.ss_within, DIGITS),
            format_number(a.ms_within, DIGITS),
            String::new(),
            String::new(),
        ]);
        t.push([
            "Total".to_owned(),
            (a.df_between + a.df_within).to_string(),
            format_number(a.ss_between + a.ss_within, DIGITS),
            String::new(),
            String::new(),
            String::new(),
        ]);
        t.push([
            "R²".to_owned(),
            format_number(a.r_squared, DIGITS),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]);
        t.push([
            "Residual std dev".to_owned(),
            format_number(a.residual_std_dev, DIGITS),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]);
        section(SectionKind::OneWayAnova, "Analysis of Variance", SectionContent::Table(t))
    }

    /// Values by group: group `g` sits at x = g + 1 with its values spread evenly
    /// across ±0.2 (in row order) so they do not overlap, colored by group, with a
    /// short bar at the group mean.
    fn values_by_group(&self, levels: &[String], members: &[Vec<(usize, f64)>], a: &OneWayAnova) -> ReportSection {
        let (value_name, group_name) = self.names();
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
        let lines = a
            .groups
            .iter()
            .enumerate()
            .map(|(g, s)| vec![[(g + 1) as f64 - 0.3, s.mean], [(g + 1) as f64 + 0.3, s.mean]])
            .collect();
        // The values are on the vertical axis.
        let value_time =
            if self.grouped_names.as_ref().is_some_and(|(v, _)| *v == self.x_name) { self.x_time } else { self.y_time };
        let spec = PlotSpec {
            x_time: None,
            y_time: value_time,
            x_label: group_name.to_owned(),
            y_label: value_name.to_owned(),
            points,
            line: None,
            band: None,
            groups: Some(PointGroups { title: group_name.to_owned(), labels: levels.to_vec(), membership, lines }),
            x_ticks: Some(levels.iter().enumerate().map(|(g, l)| ((g + 1) as f64, l.clone())).collect()),
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        };
        section(SectionKind::ValuesByGroup, "Values by Group", SectionContent::Plot(Box::new(spec)))
    }

    fn correlation_table(&self, p: &PearsonSummary, s: &Correlation) -> ReportSection {
        let mut t = TextTable::new(["Measure", "Estimate", "Lower 95%", "Upper 95%", "t", "p-value"]);
        for (name, c) in [("Pearson r", &p.correlation), ("Spearman ρ", s)] {
            t.push([
                name.to_owned(),
                format_number(c.estimate, DIGITS),
                format_optional(c.confidence_interval.map(|i| i.0), DIGITS),
                format_optional(c.confidence_interval.map(|i| i.1), DIGITS),
                format_optional(c.t_statistic, 4),
                c.p_value.map_or_else(|| "—".to_owned(), format_p_value),
            ]);
        }
        let blank = || String::new();
        t.push(["Covariance".to_owned(), format_number(p.covariance, DIGITS), blank(), blank(), blank(), blank()]);
        t.push(["N (pairs)".to_owned(), p.correlation.n.to_string(), blank(), blank(), blank(), blank()]);
        if self.n_skipped > 0 {
            t.push(["Rows not used".to_owned(), self.n_skipped.to_string(), blank(), blank(), blank(), blank()]);
        }
        section(SectionKind::Correlations, "Correlations", SectionContent::Table(t))
    }

    fn scatter(&self, points: &[RowPoint]) -> ReportSection {
        let spec = PlotSpec {
            x_time: self.x_time,
            y_time: self.y_time,
            x_label: self.x_name.clone(),
            y_label: self.y_name.clone(),
            points: points.to_vec(),
            line: None,
            band: None,
            groups: None,
            x_ticks: None,
            panes: Vec::new(),
            annotations: None,
            segments: Vec::new(),
            ..PlotSpec::default()
        };
        section(SectionKind::BivariateScatter, "Scatter Plot", SectionContent::Plot(Box::new(spec)))
    }

    fn tests_table(&self, t: &IndependenceTest, table: &ContingencyTable) -> ReportSection {
        let mut out = TextTable::new(["Test", "Statistic", "DF", "p-value"]);
        out.push([
            "Pearson χ²".to_owned(),
            format_number(t.pearson_chi_square, DIGITS),
            t.df.to_string(),
            format_p_value(t.pearson_p_value),
        ]);
        out.push([
            "Likelihood ratio G²".to_owned(),
            format_number(t.likelihood_ratio, DIGITS),
            t.df.to_string(),
            format_p_value(t.likelihood_ratio_p_value),
        ]);
        out.push(["Cramér's V".to_owned(), format_number(t.cramers_v, DIGITS), String::new(), String::new()]);
        out.push(["N".to_owned(), table.total().to_string(), String::new(), String::new()]);
        out.push([
            "Smallest expected count".to_owned(),
            format_number(t.min_expected, 4),
            String::new(),
            if t.small_expected_counts() {
                "χ² approximation doubtful: expected counts too small".to_owned()
            } else {
                String::new()
            },
        ]);
        section(SectionKind::IndependenceTests, "Tests of Independence", SectionContent::Table(out))
    }
}

/// φ (2 × 2 tables) and Cramér's V with 95% noncentral-χ² intervals.
fn association(t: &IndependenceTest, table: &ContingencyTable) -> ReportSection {
    let (r, c) = (table.row_labels.len(), table.column_labels.len());
    crate::effect_size_sections::association_section(
        stat_inference::effect_sizes::association_effect_size(
            t.pearson_chi_square,
            t.df as f64,
            table.total() as f64,
            r,
            c,
            0.95,
        ),
        r == 2 && c == 2,
        0.95,
    )
}

/// Counts with row and column totals. (Expected counts feed the tests section;
/// they are not shown cell by cell, to keep the table readable.)
fn counts_table(table: &ContingencyTable, row_name: &str) -> ReportSection {
    let mut columns = vec![row_name.to_owned()];
    columns.extend(table.column_labels.iter().cloned());
    columns.push("Total".to_owned());
    let mut t = TextTable::new(columns);
    let row_totals = table.row_totals();
    for (i, label) in table.row_labels.iter().enumerate() {
        let mut row = vec![label.clone()];
        row.extend(table.counts[i].iter().map(u64::to_string));
        row.push(row_totals[i].to_string());
        t.push(row);
    }
    let mut totals = vec!["Total".to_owned()];
    totals.extend(table.column_totals().iter().map(u64::to_string));
    totals.push(table.total().to_string());
    t.push(totals);
    section(SectionKind::ContingencyCounts, "Contingency Table", SectionContent::Table(t))
}

/// Each cell's adjusted residual (Haberman 1973) with a sign glyph where it
/// exceeds the two-sided 5% normal point: ▲ more often than independence
/// predicts, ▼ less often.
fn residuals_table(table: &ContingencyTable, row_name: &str) -> ReportSection {
    let mut columns = vec![row_name.to_owned()];
    columns.extend(table.column_labels.iter().cloned());
    let mut t = TextTable::new(columns);
    let residuals = stat_discrete::adjusted_residuals(table).unwrap_or_default();
    for (i, label) in table.row_labels.iter().enumerate() {
        let mut row = vec![label.clone()];
        for j in 0..table.column_labels.len() {
            let r = residuals.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN);
            row.push(residual_cell(r));
        }
        t.push(row);
    }
    section(
        SectionKind::AdjustedResiduals,
        "Adjusted Residuals (▲ above, ▼ below what independence predicts, |r| ≥ 1.96)",
        SectionContent::Table(t),
    )
}

/// An adjusted residual with its glyph.
pub(crate) fn residual_cell(r: f64) -> String {
    if r.is_nan() {
        return "—".to_owned();
    }
    let glyph = if r >= RESIDUAL_CUTOFF {
        " ▲"
    } else if r <= -RESIDUAL_CUTOFF {
        " ▼"
    } else {
        ""
    };
    format!("{}{glyph}", format_number(r, 3))
}

/// The two-sided 5% point of the standard normal distribution.
const RESIDUAL_CUTOFF: f64 = 1.959_963_984_540_054;

/// Most row levels whose pairwise differences of proportions are listed.
pub const MAX_PROPORTION_GROUPS: usize = 10;

/// Differences between every pair of rows in the proportion of each column
/// level (only the second level when the column has two), with Newcombe's
/// hybrid-score intervals and a forest plot.
fn proportion_differences(table: &ContingencyTable, column_name: &str) -> Vec<ReportSection> {
    let (r, c) = (table.row_labels.len(), table.column_labels.len());
    if r > MAX_PROPORTION_GROUPS {
        return vec![section(
            SectionKind::ProportionDifferences,
            "Differences of Proportions",
            SectionContent::Note(format!(
                "Not listed for more than {MAX_PROPORTION_GROUPS} row levels ({r} here): the pairs would be too many to read."
            )),
        )];
    }
    let outcomes: Vec<usize> = if c == 2 { vec![1] } else { (0..c).collect() };
    let totals = table.row_totals();
    let mut t = TextTable::new([
        "Proportion".to_owned(),
        "Comparison".to_owned(),
        "Difference".to_owned(),
        "Lower 95%".to_owned(),
        "Upper 95%".to_owned(),
    ]);
    let mut rows = Vec::new();
    for &j in &outcomes {
        let what = format!("P({column_name} = {})", table.column_labels[j]);
        for a in 0..r {
            for b in a + 1..r {
                let label = format!("{} − {}", table.row_labels[b], table.row_labels[a]);
                match stat_inference::proportions::newcombe_difference(
                    table.counts[b][j],
                    totals[b],
                    table.counts[a][j],
                    totals[a],
                    0.95,
                ) {
                    Ok((d, lo, hi)) => {
                        t.push([
                            what.clone(),
                            label.clone(),
                            format_number(d, DIGITS),
                            format_number(lo, DIGITS),
                            format_number(hi, DIGITS),
                        ]);
                        let excludes_zero = usize::from(lo > 0.0 || hi < 0.0);
                        let name =
                            if outcomes.len() == 1 { label } else { format!("{label}: {}", table.column_labels[j]) };
                        rows.push(crate::fdr::IntervalRow {
                            label: name,
                            estimate: d,
                            interval: (lo, hi),
                            group: excludes_zero,
                        });
                    }
                    Err(e) => {
                        t.push([what.clone(), label, format!("not available: {e}"), String::new(), String::new()])
                    }
                }
            }
        }
    }
    let mut out = vec![section(
        SectionKind::ProportionDifferences,
        "Differences of Proportions (Newcombe hybrid-score 95% intervals)",
        SectionContent::Table(t),
    )];
    if !rows.is_empty() {
        out.push(crate::fdr::interval_plot(
            SectionKind::ProportionForest,
            "Forest Plot of the Differences of Proportions".to_owned(),
            "Difference of proportions".to_owned(),
            &rows,
            vec!["Interval covers 0".to_owned(), "Interval excludes 0".to_owned()],
            Some((0.0, "No difference".to_owned())),
        ));
    }
    out
}

/// Pooled and Welch t-tests of the difference of two group means.
fn mean_difference_table(levels: &[String], values: &[Vec<f64>]) -> ReportSection {
    let title = format!("Difference of Means ({} − {})", levels[0], levels[1]);
    let tests = match two_sample_t_tests(&values[0], &values[1], 0.95) {
        Ok(t) => t,
        Err(e) => {
            return section(SectionKind::MeanDifference, &title, SectionContent::Note(format!("Not available: {e}")));
        }
    };
    let mut t =
        TextTable::new(["Assumption", "Difference", "Std Error", "DF", "t", "p-value", "Lower 95%", "Upper 95%"]);
    for (name, d) in [("Equal variances (pooled)", tests.pooled), ("Unequal variances (Welch)", tests.welch)] {
        t.push([
            name.to_owned(),
            format_number(d.difference, DIGITS),
            format_number(d.std_error, DIGITS),
            format_number(d.df, 5),
            format_number(d.t_statistic, 4),
            format_p_value(d.p_value),
            format_number(d.confidence_interval.0, DIGITS),
            format_number(d.confidence_interval.1, DIGITS),
        ]);
    }
    section(SectionKind::MeanDifference, &title, SectionContent::Table(t))
}

/// Welch's ANOVA for three or more groups.
fn welch_table(values: &[Vec<f64>]) -> ReportSection {
    let title = "Welch's Test of Equal Means (unequal variances)";
    match welch_anova(values) {
        Ok(w) => {
            let mut t = TextTable::new(["F", "DF (numerator)", "DF (denominator)", "p-value"]);
            t.push([
                format_number(w.f_statistic, DIGITS),
                format_number(w.df1, 5),
                format_number(w.df2, 5),
                format_p_value(w.p_value),
            ]);
            section(SectionKind::WelchAnova, title, SectionContent::Table(t))
        }
        Err(e) => section(SectionKind::WelchAnova, title, SectionContent::Note(format!("Not available: {e}"))),
    }
}

/// Levene, Brown–Forsythe and Bartlett tests.
fn equal_variance_table(values: &[Vec<f64>]) -> ReportSection {
    let title = "Tests of Equal Variances (small p-values indicate unequal spreads)";
    match variance_tests(values) {
        Ok(v) => {
            let mut t = TextTable::new(["Test", "Statistic", "DF", "p-value"]);
            let f_df = |df1: f64, df2: f64| format!("{}, {}", format_number(df1, 5), format_number(df2, 5));
            t.push([
                "Levene (F)".to_owned(),
                format_number(v.levene.f_statistic, DIGITS),
                f_df(v.levene.df1, v.levene.df2),
                format_p_value(v.levene.p_value),
            ]);
            t.push([
                "Brown–Forsythe (F)".to_owned(),
                format_number(v.brown_forsythe.f_statistic, DIGITS),
                f_df(v.brown_forsythe.df1, v.brown_forsythe.df2),
                format_p_value(v.brown_forsythe.p_value),
            ]);
            t.push([
                "Bartlett (χ²)".to_owned(),
                format_number(v.bartlett.statistic, DIGITS),
                format_number(v.bartlett.df, 5),
                format_p_value(v.bartlett.p_value),
            ]);
            section(SectionKind::EqualVariances, title, SectionContent::Table(t))
        }
        Err(e) => section(SectionKind::EqualVariances, title, SectionContent::Note(format!("Not available: {e}"))),
    }
}

/// Wilcoxon rank-sum (two groups) and Kruskal–Wallis tests.
fn rank_test_table(values: &[Vec<f64>]) -> ReportSection {
    let title = "Rank Tests (normal / χ² approximations)";
    let mut t = TextTable::new(["Test", "Statistic", "DF", "p-value"]);
    if values.len() == 2 {
        match rank_sum_test(&values[0], &values[1]) {
            Ok(r) => t.push([
                "Wilcoxon rank-sum (Mann–Whitney)".to_owned(),
                format!("U = {}, z = {}", format_number(r.u_statistic, DIGITS), format_number(r.z, 4)),
                String::new(),
                format_p_value(r.p_value),
            ]),
            Err(e) => t.push(["Wilcoxon rank-sum (Mann–Whitney)".to_owned(), "—".into(), String::new(), e.to_string()]),
        }
    }
    match kruskal_wallis(values) {
        Ok(k) => t.push([
            "Kruskal–Wallis (χ²)".to_owned(),
            format_number(k.statistic, DIGITS),
            format_number(k.df, 5),
            format_p_value(k.p_value),
        ]),
        Err(e) => t.push(["Kruskal–Wallis (χ²)".to_owned(), "—".into(), String::new(), e.to_string()]),
    }
    section(SectionKind::RankTests, title, SectionContent::Table(t))
}

/// Every pair of groups compared by `method` with 95% intervals: the
/// pooled-variance methods on the pooled within-group mean square (`N − k`
/// df), Games–Howell, Tamhane T2 and Dunnett T3 on Welch standard errors and
/// df per pair. Shown for three or more groups; two groups
/// have the t tests above.
fn pairwise_section(levels: &[String], values: &[Vec<f64>], method: ComparisonMethod) -> ReportSection {
    let content = match compare_all_pairs(values, method, PAIRWISE_LEVEL) {
        Ok(list) => SectionContent::Table(crate::compare::pairwise_table(levels, &list, method, PAIRWISE_LEVEL)),
        Err(e) => SectionContent::Note(format!("Not available: {e}")),
    };
    section(SectionKind::PairwiseComparisons, &crate::compare::pairwise_title(method, PAIRWISE_LEVEL), content)
}

fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let g1 = ["a", "a", "b", "b", "a", "b", "a", "b"];
        let g2 = ["x", "y", "y", "y", "x", "y", "x", "x"];
        DataTable::new(
            "t",
            vec![
                Column::numeric("u", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]),
                Column::numeric("v", vec![2.0, 1.0, 4.0, 3.0, 7.0, 8.0, 6.0, 9.0]),
                Column::text("g1", g1.iter().map(|s| (*s).to_owned()).collect()),
                Column::text("g2", g2.iter().map(|s| (*s).to_owned()).collect()),
            ],
        )
        .unwrap()
    }

    #[test]
    fn continuous_pair() {
        let t = table();
        let a = BivariateAnalysis::run(&t, &RowStateManager::new(8), &BivariateRequest::new(0, 1)).unwrap();
        assert_eq!(a.title(), "Bivariate: v by u");
        let s = a.sections();
        let SectionContent::Table(c) = &s[0].content else { panic!() };
        assert_eq!(c.cell(0, "Estimate"), Some("0.896421"));
        let SectionContent::Plot(p) = &s[1].content else { panic!() };
        assert_eq!(p.points.len(), 8);
        assert_eq!(p.points[4].row, 4);
    }

    #[test]
    fn categorical_pair() {
        let t = table();
        let a = BivariateAnalysis::run(&t, &RowStateManager::new(8), &BivariateRequest::new(2, 3)).unwrap();
        let s = a.sections();
        let SectionContent::Table(counts) = &s[0].content else { panic!() };
        // a: x x x y → (3, 1); b: y y y x → (1, 3).
        assert_eq!(counts.columns, vec!["g1", "x", "y", "Total"]);
        assert_eq!(counts.rows[0], vec!["a", "3", "1", "4"]);
        assert_eq!(counts.rows[2], vec!["Total", "4", "4", "8"]);
        let SectionContent::Table(tests) = &s[1].content else { panic!() };
        // X² = 8·(3·3 − 1·1)² / (4·4·4·4) = 2.
        assert_eq!(tests.cell(0, "Statistic"), Some("2"));
        assert!(tests.rows[4][3].contains("doubtful"), "expected counts of 2");
        // Adjusted residual (3 − 2)/√(2 · ½ · ½) = √2 in each corner: no glyph.
        let SectionContent::Table(res) = &s.iter().find(|x| x.kind == SectionKind::AdjustedResiduals).unwrap().content
        else {
            panic!()
        };
        assert_eq!(res.rows[0], vec!["a", "1.41", "-1.41"]);
        assert_eq!(residual_cell(2.5), "2.5 ▲");
        assert_eq!(residual_cell(-1.96), "-1.96 ▼");
        // P(g2 = y): 1/4 in a, 3/4 in b; Newcombe's interval for b − a.
        let SectionContent::Table(diff) =
            &s.iter().find(|x| x.kind == SectionKind::ProportionDifferences).unwrap().content
        else {
            panic!()
        };
        let (d, lo, hi) = stat_inference::proportions::newcombe_difference(3, 4, 1, 4, 0.95).unwrap();
        assert_eq!(diff.rows[0][1], "b − a");
        assert_eq!(diff.rows[0][2], format_number(d, DIGITS));
        assert_eq!(diff.rows[0][3], format_number(lo, DIGITS));
        assert_eq!(diff.rows[0][4], format_number(hi, DIGITS));
        assert!(s.iter().any(|x| x.kind == SectionKind::ProportionForest));
    }

    /// Three or more groups get pairwise comparisons, Tukey–Kramer by
    /// default. Reference: SciPy 1.x `tukey_hsd` on its documented example
    /// (p = 0.01445, 0.98031, 0.02033; A − B interval −8.249159 to −0.950841).
    #[test]
    fn three_groups_get_pairwise_comparisons() {
        let a = [24.5, 23.5, 26.4, 27.1, 29.9];
        let b = [28.4, 34.2, 29.5, 32.2, 30.1];
        let c = [26.1, 28.3, 24.3, 26.2, 27.8];
        let values: Vec<f64> = a.iter().chain(&b).chain(&c).copied().collect();
        let groups: Vec<String> =
            ["A", "B", "C"].iter().flat_map(|g| std::iter::repeat_n((*g).to_owned(), 5)).collect();
        let t = DataTable::new("t", vec![Column::numeric("y", values), Column::text("g", groups)]).unwrap();
        let s = RowStateManager::new(15);
        let mut analysis = BivariateAnalysis::run(&t, &s, &BivariateRequest::new(1, 0)).unwrap();
        assert!(analysis.is_grouped());
        let sec = analysis.sections();
        let pairs = sec.iter().find(|x| x.kind == SectionKind::PairwiseComparisons).unwrap();
        assert!(pairs.title.starts_with("All Pairs: Tukey–Kramer"));
        let SectionContent::Table(tab) = &pairs.content else { panic!() };
        let p = |r: usize| -> f64 { tab.cell(r, "Adjusted p").unwrap().parse().unwrap() };
        assert!((p(0) - 0.014_448).abs() < 1e-4);
        assert!((p(1) - 0.980_311).abs() < 1e-4);
        assert!((p(2) - 0.020_331).abs() < 1e-4);
        let lo: f64 = tab.cell(0, "Lower").unwrap().parse().unwrap();
        let hi: f64 = tab.cell(0, "Upper").unwrap().parse().unwrap();
        assert!((lo + 8.249_159).abs() < 1e-4 && (hi + 0.950_841).abs() < 1e-4);
        analysis.set_pairwise(None);
        assert!(analysis.sections().iter().all(|x| x.kind != SectionKind::PairwiseComparisons));
        analysis.set_pairwise(Some(ComparisonMethod::GamesHowell));
        assert!(analysis.sections().iter().any(|x| x.title.starts_with("All Pairs: Games–Howell")));
    }

    #[test]
    fn continuous_by_categorical_compares_group_means() {
        let t = table();
        let s = RowStateManager::new(8);
        // u by g1: a = {1, 2, 5, 7} (mean 3.75), b = {3, 4, 6, 8} (mean 5.25).
        for request in [BivariateRequest::new(2, 0), BivariateRequest::new(0, 2)] {
            let a = BivariateAnalysis::run(&t, &s, &request).unwrap();
            assert_eq!(a.title(), "Bivariate: u by g1", "either column order");
            let sec = a.sections();
            let kinds: Vec<_> = sec.iter().map(|x| x.kind).collect();
            assert_eq!(
                kinds,
                [
                    SectionKind::GroupMeans,
                    SectionKind::OneWayAnova,
                    SectionKind::MeanDifference,
                    SectionKind::EqualVariances,
                    SectionKind::RankTests,
                    SectionKind::ValuesByGroup
                ]
            );
            // With two groups the pooled t² equals the ANOVA F (0.72).
            let SectionContent::Table(diff) = &sec[2].content else { panic!() };
            assert_eq!(diff.cell(0, "Difference"), Some("-1.5"));
            let t_stat: f64 = diff.cell(0, "t").unwrap().parse().unwrap();
            assert!((t_stat * t_stat - 0.72).abs() < 1e-3);
            assert_eq!(diff.cell(0, "DF"), Some("6"));
            let SectionContent::Table(ranks) = &sec[4].content else { panic!() };
            assert!(ranks.cell(0, "Statistic").unwrap().starts_with("U = 5,"), "ranks 1, 2, 5, 7 sum to 15");
            let SectionContent::Table(means) = &sec[0].content else { panic!() };
            assert_eq!(means.columns[0], "g1");
            assert_eq!(means.rows[0][..3], ["a".to_owned(), "4".to_owned(), "3.75".to_owned()]);
            assert_eq!(means.rows[1][2], "5.25");
            // SS_between = 4·(0.75² + 0.75²) = 4.5; SS_within = 22.75 + 14.75 = 37.5.
            let SectionContent::Table(anova) = &sec[1].content else { panic!() };
            assert_eq!(anova.cell(0, "Sum of Squares"), Some("4.5"));
            assert_eq!(anova.cell(1, "Sum of Squares"), Some("37.5"));
            assert_eq!(anova.cell(0, "F"), Some("0.72"));
            let SectionContent::Plot(p) = &sec[5].content else { panic!() };
            let g = p.groups.as_ref().unwrap();
            assert_eq!(g.labels, vec!["a", "b"]);
            assert_eq!(p.points.len(), 8);
            for (pt, &m) in p.points.iter().zip(&g.membership) {
                assert!((pt.x - (m + 1) as f64).abs() <= 0.2 + 1e-12);
                assert_eq!(t.column(2).unwrap().level_text(pt.row), g.labels[m]);
            }
            assert_eq!(g.lines[1][0][1], 5.25, "mean bar of group b");
            assert_eq!(p.x_ticks.as_ref().unwrap()[1], (2.0, "b".to_owned()));
        }
    }

    #[test]
    fn groups_follow_natural_order_not_first_appearance() {
        // B appears first in the data; the report lists A first and the
        // difference is mean(A) − mean(B), as Compare Groups reports it.
        let values = vec![10.0, 12.0, 11.0, 1.0, 2.0, 3.0];
        let groups: Vec<String> = ["B", "B", "B", "A", "A", "A"].map(String::from).to_vec();
        let t = DataTable::new("t", vec![Column::numeric("y", values), Column::text("g", groups)]).unwrap();
        let a = BivariateAnalysis::run(&t, &RowStateManager::new(6), &BivariateRequest::new(1, 0)).unwrap();
        let sec = a.sections();
        let SectionContent::Table(means) = &sec[0].content else { panic!() };
        assert_eq!(means.rows[0][0], "A");
        let diff = sec.iter().find(|x| x.kind == SectionKind::MeanDifference).unwrap();
        let SectionContent::Table(diff) = &diff.content else { panic!() };
        assert_eq!(diff.cell(0, "Difference"), Some("-9"));
    }

    #[test]
    fn same_column_is_rejected() {
        let t = table();
        let s = RowStateManager::new(8);
        assert!(BivariateAnalysis::run(&t, &s, &BivariateRequest::new(1, 1)).is_err());
    }
}
