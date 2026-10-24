//! Nested comparison of groups bound to a dataset
//! ([`stat_mixed::nested_comparison`]): groups, subjects nested in groups,
//! replicate measurements on each subject. With two groups this is the
//! nested t test; with more, a nested one-way ANOVA.
//!
//! The data are stacked: one row per measurement, with response, group and
//! subject columns. Subjects are identified within their group, so subject
//! "1" in group A and subject "1" in group B are different subjects.

use crate::compare::section;
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, obj, s};
use crate::report::{PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_linear::natural_order;
use stat_mixed::{NestedComparison, NestedEstimate, nested_comparison};

const DIGITS: usize = 5;

/// Settings of a nested comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct NestedRequest {
    /// Response column (numeric).
    pub response: usize,
    /// Group (treatment) column.
    pub group: usize,
    /// Subject column (subjects nested in groups).
    pub subject: usize,
    /// Confidence level.
    pub level: f64,
}

impl NestedRequest {
    /// Default settings (95% intervals).
    #[must_use]
    pub fn new(response: usize, group: usize, subject: usize) -> Self {
        Self { response, group, subject, level: 0.95 }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.response, self.group, self.subject]
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        Self { response: f(self.response), group: f(self.group), subject: f(self.subject), ..self.clone() }
    }

    /// Project-file form.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        obj([
            ("analysis", s("nested-comparison")),
            ("response", col(self.response)),
            ("group", col(self.group)),
            ("subject", col(self.subject)),
            ("level", JsonValue::number(self.level)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing columns or a level outside (0, 1).
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let col = |key: &str| -> Result<usize> {
            let name = member(v, key)?.as_str().ok_or_else(|| invalid(format!("{key}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let level = v.get("level").and_then(JsonValue::as_f64).unwrap_or(0.95);
        if !(level > 0.0 && level < 1.0) {
            return Err(invalid("nested-comparison: level must lie between 0 and 1"));
        }
        Ok(Self { response: col("response")?, group: col("group")?, subject: col("subject")?, level })
    }
}

/// A computed nested comparison.
#[derive(Debug, Clone)]
pub struct NestedReport {
    request: NestedRequest,
    names: [String; 3],
    /// `(row, group, subject, value)` of every measurement used.
    rows: Vec<(usize, usize, usize, f64)>,
    /// Subject labels (within their group) and each subject's group.
    subjects: Vec<(String, usize)>,
    /// The analysis.
    pub analysis: NestedComparison,
}

impl NestedReport {
    /// Gather measurements and fit the model.
    ///
    /// # Errors
    /// Repeated or invalid columns, a non-numeric response, or the engine's
    /// errors (fewer than two groups, a group with one subject, no
    /// replicates, a failed fit).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &NestedRequest) -> Result<Self> {
        let cols = request.columns();
        if (1..cols.len()).any(|i| cols[..i].contains(&cols[i])) {
            return Err(invalid("the response, group and subject columns must all differ"));
        }
        let column = |i: usize| table.column(i).ok_or_else(|| invalid("no such column"));
        let response = column(request.response)?;
        let y = response.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", response.name)))?;
        let (gc, sc) = (column(request.group)?, column(request.subject)?);
        let used: Vec<usize> = (0..table.n_rows())
            .filter(|&r| {
                !states.is_excluded(r) && y[r].is_finite() && !gc.level_text(r).is_empty() && !sc.level_text(r).is_empty()
            })
            .collect();
        let mut groups: Vec<String> = Vec::new();
        for &r in &used {
            let g = gc.level_text(r);
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        groups.sort_by(|a, b| natural_order(a, b));
        let mut keys: Vec<(usize, String)> = Vec::new();
        for &r in &used {
            let g = groups.iter().position(|x| *x == gc.level_text(r)).unwrap_or(0);
            let key = (g, sc.level_text(r));
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| natural_order(&a.1, &b.1)));
        let mut rows = Vec::new();
        for &r in &used {
            let g = groups.iter().position(|x| *x == gc.level_text(r)).unwrap_or(0);
            let s = keys.iter().position(|k| k.0 == g && k.1 == sc.level_text(r)).unwrap_or(0);
            rows.push((r, g, s, y[r]));
        }
        let subject_names: Vec<String> = keys.iter().map(|(g, s)| format!("{}/{s}", groups[*g])).collect();
        let analysis = nested_comparison(
            &rows.iter().map(|r| r.3).collect::<Vec<_>>(),
            &rows.iter().map(|r| r.1).collect::<Vec<_>>(),
            &groups,
            &rows.iter().map(|r| r.2).collect::<Vec<_>>(),
            &subject_names,
            request.level,
        )?;
        Ok(Self {
            request: request.clone(),
            names: [response.name.clone(), gc.name.clone(), sc.name.clone()],
            rows,
            subjects: keys.into_iter().map(|(g, s)| (s, g)).collect(),
            analysis,
        })
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = if self.analysis.groups.len() == 2 { "Nested t Test" } else { "Nested Comparison" };
        format!("{kind}: {} by {} ({} nested)", self.names[0], self.names[1], self.names[2])
    }

    fn pct(&self) -> String {
        format!("{}%", format_number(100.0 * self.request.level, 4))
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let a = &self.analysis;
        let pct = self.pct();
        let mut summary = TextTable::new(
            [
                self.names[1].as_str(),
                "Subjects",
                "Measurements",
                "Mean",
                "Std error",
                "DF",
                &format!("Lower {pct}"),
                &format!("Upper {pct}"),
            ]
            .map(str::to_owned),
        );
        for (g, name) in a.groups.iter().enumerate() {
            let m = &a.means[g];
            summary.push([
                name.clone(),
                a.subjects_per_group[g].to_string(),
                a.observations_per_group[g].to_string(),
                format_number(m.estimate, DIGITS),
                format_number(m.std_error, DIGITS),
                format_number(m.df, 4),
                format_number(m.lower, DIGITS),
                format_number(m.upper, DIGITS),
            ]);
        }
        let mut out = vec![section(
            SectionKind::NestedSummary,
            format!(
                "Group Means (REML fit with {} as a random effect within {}; Satterthwaite degrees of freedom)",
                self.names[2], self.names[1]
            ),
            SectionContent::Table(summary),
        )];
        // Tests: the overall F test and every pair of groups.
        let mut tests = TextTable::new(
            [
                "Comparison",
                "Difference",
                "Std error",
                "DF",
                "t or F ratio",
                "p-value",
                &format!("Lower {pct}"),
                &format!("Upper {pct}"),
            ]
            .map(str::to_owned),
        );
        let t = &a.group_test;
        tests.push([
            format!("All {} equal", self.names[1]),
            String::new(),
            String::new(),
            format!("{}, {}", t.num_df, t.den_df.map_or_else(String::new, |d| format_number(d, 4))),
            format!("F = {}", format_number(t.statistic, DIGITS)),
            format_p_value(t.p_value),
            String::new(),
            String::new(),
        ]);
        for ((i, j), d) in &a.differences {
            tests.push(difference_row(&format!("{} − {}", a.groups[*i], a.groups[*j]), d));
        }
        let pairs = if a.groups.len() > 2 { "; pairs unadjusted for multiplicity" } else { "" };
        out.push(section(
            SectionKind::NestedTests,
            format!(
                "Tests ({} as the experimental unit: replicate measurements are not independent{pairs})",
                self.names[2]
            ),
            SectionContent::Table(tests),
        ));
        let total = a.subject_variance + a.residual_variance;
        let mut vc = TextTable::new(["Component", "Variance", "Std dev", "Percent of total"]);
        for (name, v) in [
            (format!("{} within {}", self.names[2], self.names[1]), a.subject_variance),
            ("Replicates (residual)".to_owned(), a.residual_variance),
        ] {
            vc.push([
                name,
                format_number(v, DIGITS),
                format_number(v.max(0.0).sqrt(), DIGITS),
                format!("{:.1}", 100.0 * v / total),
            ]);
        }
        let note = if a.fit.warnings.is_empty() { String::new() } else { format!("; {}", a.fit.warnings.join("; ")) };
        out.push(section(
            SectionKind::VarianceComponents,
            format!(
                "Variance Components (REML; intraclass correlation {}: the share of variance between {}{note})",
                format_number(a.icc, 3),
                self.names[2]
            ),
            SectionContent::Table(vc),
        ));
        out.push(self.anova_section());
        out.push(self.plot_section());
        out
    }

    fn anova_section(&self) -> ReportSection {
        let a = &self.analysis;
        if !a.balanced {
            return section(
                SectionKind::NestedAnova,
                "Nested Analysis of Variance",
                SectionContent::Note(
                    "Not shown: the classical nested ANOVA F test is exact only with equal replicates per subject \
                     and equal subjects per group. The REML tests above are valid for unbalanced data."
                        .to_owned(),
                ),
            );
        }
        let n = &a.anova;
        let mut t = TextTable::new(["Source", "DF", "Sum of squares", "Mean square", "F ratio", "p-value"]);
        t.push([
            self.names[1].clone(),
            format_number(n.df_groups, 4),
            format_number(n.ss_groups, DIGITS),
            format_number(n.ss_groups / n.df_groups, DIGITS),
            format_number(n.f_groups, DIGITS),
            format_p_value(n.p_groups),
        ]);
        t.push([
            format!("{} within {}", self.names[2], self.names[1]),
            format_number(n.df_subjects, 4),
            format_number(n.ss_subjects, DIGITS),
            format_number(n.ss_subjects / n.df_subjects, DIGITS),
            format_number(n.f_subjects, DIGITS),
            format_p_value(n.p_subjects),
        ]);
        t.push([
            "Replicates (residual)".to_owned(),
            format_number(n.df_residual, 4),
            format_number(n.ss_residual, DIGITS),
            format_number(n.ss_residual / n.df_residual, DIGITS),
            String::new(),
            String::new(),
        ]);
        section(
            SectionKind::NestedAnova,
            format!(
                "Nested Analysis of Variance (balanced data; {} tested against {} within {})",
                self.names[1], self.names[2], self.names[1]
            ),
            SectionContent::Table(t),
        )
    }

    /// Every measurement, at its group's position spread by subject, with
    /// each subject's mean marked.
    fn plot_section(&self) -> ReportSection {
        let a = &self.analysis;
        let per_group: Vec<Vec<usize>> = (0..a.groups.len())
            .map(|g| (0..self.subjects.len()).filter(|&s| self.subjects[s].1 == g).collect())
            .collect();
        let x_of = |s: usize| {
            let g = self.subjects[s].1;
            let k = per_group[g].iter().position(|&t| t == s).unwrap_or(0);
            let m = per_group[g].len();
            (g + 1) as f64 + if m > 1 { 0.6 * (k as f64 / (m - 1) as f64 - 0.5) } else { 0.0 }
        };
        let points: Vec<RowPoint> =
            self.rows.iter().map(|&(row, _, s, v)| RowPoint { row, x: x_of(s), y: v }).collect();
        let membership = self.rows.iter().map(|r| r.1).collect();
        let mut segments = Vec::new();
        for s in 0..self.subjects.len() {
            let v: Vec<f64> = self.rows.iter().filter(|r| r.2 == s).map(|r| r.3).collect();
            let m = v.iter().sum::<f64>() / v.len() as f64;
            let x = x_of(s);
            segments.push(vec![[x - 0.05, m], [x + 0.05, m]]);
        }
        let spec = PlotSpec {
            x_label: self.names[1].clone(),
            y_label: self.names[0].clone(),
            points,
            groups: Some(PointGroups {
                title: self.names[1].clone(),
                labels: a.groups.clone(),
                membership,
                lines: a
                    .means
                    .iter()
                    .enumerate()
                    .map(|(g, m)| vec![[(g + 1) as f64 - 0.4, m.estimate], [(g + 1) as f64 + 0.4, m.estimate]])
                    .collect(),
            }),
            x_ticks: Some(a.groups.iter().enumerate().map(|(g, l)| ((g + 1) as f64, l.clone())).collect()),
            segments,
            ..PlotSpec::default()
        };
        section(
            SectionKind::NestedPlot,
            format!(
                "Measurements by {} and {} (each column of points: one subject; short ticks: subject means; long \
                 lines: group means)",
                self.names[1], self.names[2]
            ),
            SectionContent::Plot(Box::new(spec)),
        )
    }
}

fn difference_row(name: &str, d: &NestedEstimate) -> [String; 8] {
    [
        name.to_owned(),
        format_number(d.estimate, DIGITS),
        format_number(d.std_error, DIGITS),
        format_number(d.df, 4),
        format!("t = {}", format_number(d.t, DIGITS)),
        format_p_value(d.p_value),
        format_number(d.lower, DIGITS),
        format_number(d.upper, DIGITS),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// The engine's balanced reference data (`stat_mixed::nested`), with
    /// subjects numbered 1–4 inside each group.
    fn data() -> DataTable {
        let y = [
            9.8, 8.9, 10.1, 9.9, 10.6, 10.0, 10.2, 10.2, 10.3, 8.5, 8.4, 10.4, 11.9, 13.4, 12.5, 11.5, 11.6, 10.7,
            11.7, 11.9, 12.0, 10.7, 12.4, 13.2, 12.7, 12.6, 13.3, 12.9, 13.6, 13.2, 14.3, 15.4, 15.9, 13.8, 15.2, 13.6,
        ];
        let group: Vec<String> = (0..36).map(|i| ["Control", "Low", "High"][i / 12].to_owned()).collect();
        let subject: Vec<String> = (0..36).map(|i| format!("{}", (i % 12) / 3 + 1)).collect();
        DataTable::new(
            "nested",
            vec![Column::numeric("Response", y.to_vec()), Column::text("Dose", group), Column::text("Animal", subject)],
        )
        .unwrap()
    }

    #[test]
    fn report_sections() {
        let t = data();
        let r = NestedReport::run(&t, &RowStateManager::new(36), &NestedRequest::new(0, 1, 2)).unwrap();
        assert_eq!(r.title(), "Nested Comparison: Response by Dose (Animal nested)");
        // Natural order of the groups: Control, High, Low.
        assert_eq!(r.analysis.groups, ["Control", "High", "Low"]);
        assert_eq!(r.analysis.subjects_per_group, [4, 4, 4]);
        let s = r.sections();
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::NestedSummary,
                SectionKind::NestedTests,
                SectionKind::VarianceComponents,
                SectionKind::NestedAnova,
                SectionKind::NestedPlot
            ]
        );
        let SectionContent::Table(tests) = &s[1].content else { panic!() };
        // R aov: F = 29.95288 on (2, 9).
        assert_eq!(tests.rows[0][4], "F = 29.953");
        assert_eq!(tests.rows[0][3], "2, 9");
        assert_eq!(tests.rows[1][0], "Control − High");
        assert_eq!(tests.rows[1][1], "-4.1");
        let SectionContent::Table(anova) = &s[3].content else { panic!() };
        assert_eq!(anova.rows[0][4], "29.953");
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        assert_eq!(p.points.len(), 36);
        assert_eq!(p.segments.len(), 12, "one mark per subject");
        // Two groups: the nested t test.
        let mut rows = RowStateManager::new(36);
        rows.set_excluded(24..36, true);
        let r = NestedReport::run(&t, &rows, &NestedRequest::new(0, 1, 2)).unwrap();
        assert!(r.title().starts_with("Nested t Test"));
        // Unbalanced: the classical table is replaced by a note.
        rows.set_excluded(0..1, true);
        let r = NestedReport::run(&t, &rows, &NestedRequest::new(0, 1, 2)).unwrap();
        assert!(matches!(r.sections()[3].content, SectionContent::Note(_)));
        assert!(NestedReport::run(&t, &rows, &NestedRequest::new(0, 1, 1)).is_err());
    }
}
