//! Bootstrap of any statistic a report shows (Efron & Tibshirani, *An
//! Introduction to the Bootstrap*, 1993): the report's request is re-run on
//! row-resampled copies of the data, and chosen numeric cells of one of its
//! tables are collected from each replicate.
//!
//! The resampling is [`stat_inference::bootstrap`]'s: `B` resamples, each of
//! the `n` rows the report uses drawn with replacement (within each level of
//! an optional stratum column, keeping the levels' sizes), seeded per
//! replicate and run in parallel, so a seed reproduces the report exactly.
//! The rows used are the non-excluded rows with a value in every column the
//! report refers to. The BCa acceleration comes from the jackknife (the
//! report re-run `n` times with one row left out), computed only for up to
//! [`JACKKNIFE_LIMIT`] rows; above that the report gives percentile
//! intervals only.
//!
//! Cells are read as the report shows them (the typographic minus and a
//! trailing `%` are understood), so the values carry the report's displayed
//! precision — typically five or six significant digits. A replicate whose
//! report fails, or whose cell is missing or not a number, is counted as
//! failed for that statistic and left out; the counts are reported.

use crate::format::format_number;
use crate::project::{SavedReport, invalid, member, obj, report_from_json, report_to_json, s};
use crate::report::{BarChartSpec, BarItem, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{Column, ColumnData, DataTable, JsonValue, RowStateManager};
use stat_inference::bootstrap::{BootstrapSummary, bootstrap_map, jackknife_map, summarize};

/// Significant digits in the bootstrap tables.
const DIGITS: usize = 6;

/// Largest number of rows for which the jackknife (and so the BCa interval)
/// is computed: the jackknife re-runs the report once per row.
pub const JACKKNIFE_LIMIT: usize = 500;

/// Most replicates a bootstrap report may request.
pub const MAX_REPLICATES: usize = 100_000;

/// A bootstrap of cells of one table of a report.
#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapRequest {
    /// The report whose statistics are bootstrapped (any report with tables,
    /// except a bootstrap or a graph).
    pub report: Box<SavedReport>,
    /// Title of the table section the statistics come from.
    pub section: String,
    /// Row labels (first cell of a row) to collect; empty for every row.
    pub rows: Vec<String>,
    /// Column headings to collect.
    pub columns: Vec<String>,
    /// Number of bootstrap replicates `B`.
    pub replicates: usize,
    /// Seed of the resampling.
    pub seed: u64,
    /// Two-sided confidence level of the intervals.
    pub level: f64,
    /// Resample within the levels of this column (`None`: from all rows).
    pub stratify: Option<usize>,
}

impl BootstrapRequest {
    /// A request for 1000 replicates, seed 1, 95% intervals, every row of
    /// `section` and the given columns.
    #[must_use]
    pub fn new(report: SavedReport, section: impl Into<String>, columns: Vec<String>) -> Self {
        Self {
            report: Box::new(report),
            section: section.into(),
            rows: Vec::new(),
            columns,
            replicates: 1000,
            seed: 1,
            level: 0.95,
            stratify: None,
        }
    }

    /// Columns used: the report's and the stratum column.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.report.columns().into_iter().chain(self.stratify).collect()
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: &dyn Fn(usize) -> usize) -> Self {
        self.report = Box::new(self.report.map_columns(f));
        self.stratify = self.stratify.map(f);
        self
    }

    /// The project-file form (`"analysis": "bootstrap"`), with the
    /// bootstrapped report's own form under `"report"`.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let names = |v: &[String]| JsonValue::Array(v.iter().map(|c| s(c)).collect());
        obj([
            ("analysis", s("bootstrap")),
            ("report", report_to_json(&self.report, table)),
            ("section", s(&self.section)),
            ("rows", names(&self.rows)),
            ("columns", names(&self.columns)),
            ("replicates", JsonValue::Number(self.replicates as f64)),
            ("seed", JsonValue::Number(self.seed as f64)),
            ("level", JsonValue::number(self.level)),
            ("stratify", self.stratify.and_then(|c| table.column(c)).map_or(JsonValue::Null, |c| s(&c.name))),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// A missing or malformed member, or an unknown column.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let strings = |key: &str| -> Result<Vec<String>> {
            match v.get(key) {
                None | Some(JsonValue::Null) => Ok(Vec::new()),
                Some(JsonValue::Array(a)) => a
                    .iter()
                    .map(|x| {
                        x.as_str().map(str::to_owned).ok_or_else(|| invalid(format!("bootstrap: {key} must be text")))
                    })
                    .collect(),
                Some(_) => Err(invalid(format!("bootstrap: {key} must be a list"))),
            }
        };
        let stratify = match v.get("stratify").and_then(JsonValue::as_str) {
            None => None,
            Some(name) => {
                Some(table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))?)
            }
        };
        Ok(Self {
            report: Box::new(report_from_json(member(v, "report")?, table)?),
            section: member(v, "section")?
                .as_str()
                .ok_or_else(|| invalid("bootstrap: section must be a section title"))?
                .to_owned(),
            rows: strings("rows")?,
            columns: strings("columns")?,
            replicates: v.get("replicates").and_then(JsonValue::as_usize).unwrap_or(1000),
            seed: v.get("seed").and_then(crate::seed::seed_from_json).unwrap_or(1),
            level: v.get("level").and_then(JsonValue::as_f64).unwrap_or(0.95),
            stratify,
        })
    }
}

/// A report cell read as a number: the typographic minus becomes `-`, a
/// trailing `%` is dropped; anything else that is not a finite number
/// (a dash, `∞`, text) gives `None`.
#[must_use]
pub fn cell_value(cell: &str) -> Option<f64> {
    let t = cell.trim().replace('−', "-");
    let t = t.strip_suffix('%').unwrap_or(&t).trim();
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The table titled `title` among `sections`.
fn find_table<'a>(sections: &'a [ReportSection], title: &str) -> Option<&'a TextTable> {
    sections.iter().find_map(|s| match &s.content {
        SectionContent::Table(t) if s.title == title => Some(t),
        _ => None,
    })
}

/// Statistics that can be bootstrapped: `(row label, column heading)` of
/// every numeric cell of `table` outside its first column, in table order.
#[must_use]
pub fn numeric_cells(table: &TextTable) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for row in &table.rows {
        for (j, cell) in row.iter().enumerate().skip(1) {
            if cell_value(cell).is_some() {
                out.push((row[0].clone(), table.columns[j].clone()));
            }
        }
    }
    out
}

/// `true` when a report of this kind can be bootstrapped (it has tables
/// computed from rows: not a graph, not a bootstrap itself, and not a
/// report that uses no columns).
#[must_use]
pub fn can_bootstrap(saved: &SavedReport) -> bool {
    !matches!(saved.unscoped(), SavedReport::Graph { .. } | SavedReport::Bootstrap { .. })
        && !saved.columns().is_empty()
}

/// Value of the `(row, column)` cell of `table` (first row with that label).
fn lookup(table: &TextTable, row: &str, column: &str) -> Option<f64> {
    let j = table.columns.iter().position(|c| c == column)?;
    table.rows.iter().find(|r| r[0] == row).and_then(|r| cell_value(&r[j]))
}

/// The rows of `table` at positions `rows` (repeats allowed), as a new table
/// with the same columns, levels and display.
fn take_rows(table: &DataTable, rows: &[usize]) -> Result<DataTable> {
    let columns = table
        .columns()
        .iter()
        .map(|c| {
            let data = match &c.data {
                ColumnData::Numeric(v) => ColumnData::Numeric(rows.iter().map(|&r| v[r]).collect()),
                ColumnData::Text(v) => ColumnData::Text(rows.iter().map(|&r| v[r].clone()).collect()),
                ColumnData::DateTime { values, display } => {
                    ColumnData::DateTime { values: rows.iter().map(|&r| values[r]).collect(), display: *display }
                }
            };
            Column {
                name: c.name.clone(),
                data,
                level: c.level,
                formula: None,
                value_labels: c.value_labels.clone(),
                hover: c.hover,
            }
        })
        .collect();
    DataTable::new(table.name(), columns)
}

/// One bootstrapped statistic.
#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapStatistic {
    /// Row label of the cell.
    pub row: String,
    /// Column heading of the cell.
    pub column: String,
    /// Summary of its replicates.
    pub summary: BootstrapSummary,
    /// The finite replicate values, in replicate order.
    pub values: Vec<f64>,
}

impl BootstrapStatistic {
    /// "row · column".
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} · {}", self.row, self.column)
    }
}

/// A computed bootstrap report.
#[derive(Debug, Clone)]
pub struct BootstrapAnalysis {
    request: BootstrapRequest,
    report_title: String,
    rows_used: usize,
    strata: Option<(String, usize)>,
    statistics: Vec<BootstrapStatistic>,
    /// Replicates whose report could not be computed at all.
    report_failures: usize,
    /// `None`: no jackknife (too many rows); otherwise the number of
    /// leave-one-out reports that failed.
    jackknife_failures: Option<usize>,
}

impl BootstrapAnalysis {
    /// Run the bootstrap.
    ///
    /// # Errors
    /// The bootstrapped report fails on the data, its section or cells are
    /// not found or not numeric, fewer than two rows are used, the settings
    /// are out of range (replicates from 2 to [`MAX_REPLICATES`], a level in
    /// (0, 1)), or fewer than two replicates succeed for a statistic.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &BootstrapRequest) -> Result<Self> {
        if !can_bootstrap(&request.report) {
            return Err(invalid("this report cannot be bootstrapped: it has no statistics computed from rows"));
        }
        if !(2..=MAX_REPLICATES).contains(&request.replicates) {
            return Err(invalid(format!("the number of replicates must be from 2 to {MAX_REPLICATES}")));
        }
        if !(request.level > 0.0 && request.level < 1.0) {
            return Err(invalid("the confidence level must lie between 0 and 1"));
        }
        if request.columns.is_empty() {
            return Err(invalid("choose at least one column of the table to bootstrap"));
        }
        // A report's own row filter limits the rows resampled.
        let filtered = request.report.scope().and_then(|scope| scope.apply(table, states));
        let states = filtered.as_ref().unwrap_or(states);
        let original = crate::run_report(&request.report, table, states)?;
        let section_table = find_table(&original.sections, &request.section)
            .ok_or_else(|| invalid(format!("the report has no table titled {:?}", request.section)))?;
        let rows: Vec<String> = if request.rows.is_empty() {
            section_table.rows.iter().map(|r| r[0].clone()).collect()
        } else {
            request.rows.clone()
        };
        let mut cells: Vec<(String, String, f64)> = Vec::new();
        for row in &rows {
            for column in &request.columns {
                if let Some(v) = lookup(section_table, row, column) {
                    if !cells.iter().any(|c| c.0 == *row && c.1 == *column) {
                        cells.push((row.clone(), column.clone(), v));
                    }
                }
            }
        }
        if cells.is_empty() {
            return Err(invalid(format!(
                "no numeric cells of {:?} match the chosen rows and columns",
                request.section
            )));
        }
        // Rows used: included, with a value in every column the report uses.
        let used_columns: Vec<&Column> = request.columns().iter().filter_map(|&c| table.column(c)).collect();
        let used: Vec<usize> = (0..table.n_rows())
            .filter(|&r| !states.is_excluded(r) && used_columns.iter().all(|c| !c.level_text(r).is_empty()))
            .collect();
        if used.len() < 2 {
            return Err(invalid("the bootstrap needs at least two rows with values in the report's columns"));
        }
        let (codes, strata) = match request.stratify {
            None => (None, None),
            Some(c) => {
                let col = table.column(c).ok_or_else(|| invalid(format!("no column {c}")))?;
                let mut levels: Vec<String> = Vec::new();
                let codes: Vec<usize> = used
                    .iter()
                    .map(|&r| {
                        let label = col.level_text(r);
                        levels.iter().position(|l| *l == label).unwrap_or_else(|| {
                            levels.push(label);
                            levels.len() - 1
                        })
                    })
                    .collect();
                (Some(codes), Some((col.name.clone(), levels.len())))
            }
        };
        let evaluate = |positions: &[usize]| -> Option<Vec<Option<f64>>> {
            let picked: Vec<usize> = positions.iter().map(|&p| used[p]).collect();
            let t = take_rows(table, &picked).ok()?;
            let computed = crate::run_report(&request.report, &t, &RowStateManager::new(t.n_rows())).ok()?;
            let tt = find_table(&computed.sections, &request.section)?;
            Some(cells.iter().map(|(r, c, _)| lookup(tt, r, c)).collect())
        };
        let n = used.len();
        let reps = bootstrap_map(n, codes.as_deref(), request.replicates, request.seed, evaluate)?;
        let report_failures = reps.iter().filter(|r| r.is_none()).count();
        let jack = (n <= JACKKNIFE_LIMIT).then(|| jackknife_map(n, evaluate)).transpose()?;
        let jackknife_failures = jack.as_ref().map(|j| j.iter().filter(|r| r.is_none()).count());
        let mut statistics = Vec::new();
        for (k, (row, column, estimate)) in cells.iter().enumerate() {
            let values: Vec<f64> = reps.iter().map(|r| r.as_ref().and_then(|v| v[k]).unwrap_or(f64::NAN)).collect();
            // BCa needs every jackknife value.
            let jack_k: Option<Vec<f64>> =
                jack.as_ref().and_then(|j| j.iter().map(|r| r.as_ref().and_then(|v| v[k])).collect());
            let summary = summarize(*estimate, &values, jack_k.as_deref(), codes.as_deref(), request.level)
                .map_err(|e| invalid(format!("{row} · {column}: {e}")))?;
            statistics.push(BootstrapStatistic {
                row: row.clone(),
                column: column.clone(),
                summary,
                values: values.into_iter().filter(|v| v.is_finite()).collect(),
            });
        }
        Ok(Self {
            request: request.clone(),
            report_title: original.title,
            rows_used: n,
            strata,
            statistics,
            report_failures,
            jackknife_failures,
        })
    }

    /// The bootstrapped statistics, in table order.
    #[must_use]
    pub fn statistics(&self) -> &[BootstrapStatistic] {
        &self.statistics
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &BootstrapRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Bootstrap: {} · {}", self.report_title, self.request.section)
    }

    /// Report sections: the estimates table, one histogram per statistic and
    /// a note on the method.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let pct = format!("{}%", format_number(100.0 * self.request.level, 4));
        let mut t = TextTable::new([
            "Statistic".to_owned(),
            "Estimate".to_owned(),
            "Bootstrap mean".to_owned(),
            "Bias".to_owned(),
            "Std error".to_owned(),
            format!("Lower {pct} percentile"),
            format!("Upper {pct} percentile"),
            format!("Lower {pct} BCa"),
            format!("Upper {pct} BCa"),
            "Replicates".to_owned(),
            "Failed".to_owned(),
        ]);
        let f = |v: f64| format_number(v, DIGITS);
        for st in &self.statistics {
            let m = &st.summary;
            t.push([
                st.label(),
                f(m.estimate),
                f(m.mean),
                f(m.bias),
                f(m.std_error),
                f(m.percentile.0),
                f(m.percentile.1),
                m.bca.map_or("—".to_owned(), |b| f(b.lower)),
                m.bca.map_or("—".to_owned(), |b| f(b.upper)),
                m.replicates.to_string(),
                m.failed.to_string(),
            ]);
        }
        let mut out = vec![ReportSection {
            kind: SectionKind::BootstrapEstimates,
            title: "Bootstrap Estimates".to_owned(),
            content: SectionContent::Table(t),
        }];
        for st in &self.statistics {
            out.push(ReportSection {
                kind: SectionKind::BootstrapDistribution,
                title: format!("Bootstrap Distribution: {}", st.label()),
                content: SectionContent::Bars(self.histogram(st, &pct)),
            });
        }
        out.push(ReportSection {
            kind: SectionKind::BootstrapNote,
            title: "About This Bootstrap".to_owned(),
            content: SectionContent::Note(self.note()),
        });
        out
    }

    fn histogram(&self, st: &BootstrapStatistic, pct: &str) -> BarChartSpec {
        let tagged: Vec<(usize, f64)> = st.values.iter().copied().enumerate().collect();
        let bars = crate::distribution::histogram_bars(&tagged)
            .into_iter()
            .map(|b| BarItem { rows: Vec::new(), unlinked: b.rows.len(), ..b })
            .collect();
        let m = &st.summary;
        let mut references = vec![(m.estimate, "Estimate".to_owned())];
        match m.bca {
            Some(b) => {
                references.push((b.lower, format!("BCa {pct} lower")));
                references.push((b.upper, format!("BCa {pct} upper")));
            }
            None => {
                references.push((m.percentile.0, format!("Percentile {pct} lower")));
                references.push((m.percentile.1, format!("Percentile {pct} upper")));
            }
        }
        BarChartSpec {
            x_label: st.label(),
            y_label: "Replicates".to_owned(),
            bars,
            x_ticks: None,
            curve: None,
            references,
            x_time: None,
        }
    }

    fn note(&self) -> String {
        let r = &self.request;
        let mut note = format!(
            "{} bootstrap replicates (seed {}) of the {} rows the report uses (rows not excluded, with a value in \
             each of its columns), each drawn with replacement",
            r.replicates, r.seed, self.rows_used
        );
        match &self.strata {
            Some((name, k)) => note.push_str(&format!(" within the {k} levels of {name}, keeping their sizes. ")),
            None => note.push_str(". "),
        }
        note.push_str(
            "Each replicate re-runs the report on its resample and reads the cells as the report shows them, so \
             values carry the report's displayed precision. Bias is the bootstrap mean minus the estimate; the \
             standard error is the standard deviation of the replicates. Percentile limits are quantiles of the \
             replicates; BCa limits adjust them for bias and skewness (Efron 1987), with the acceleration from \
             the jackknife. ",
        );
        if self.report_failures > 0 {
            note.push_str(&format!(
                "The report could not be computed on {} of the resamples; they are left out and counted as failed. ",
                self.report_failures
            ));
        }
        match self.jackknife_failures {
            None => note.push_str(&format!(
                "BCa intervals are not given: the jackknife re-runs the report once per row and is computed for at \
                 most {JACKKNIFE_LIMIT} rows."
            )),
            Some(0) => note.push_str(&format!("The jackknife re-ran the report {} times.", self.rows_used)),
            Some(k) => note.push_str(&format!(
                "The report failed on {k} of the {} jackknife samples, so statistics depending on them have no BCa \
                 interval.",
                self.rows_used
            )),
        }
        note
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DistributionOptions, DistributionRequest};

    fn distribution(column: usize) -> SavedReport {
        SavedReport::Distribution {
            request: DistributionRequest { column, quantile_method: Default::default(), censoring: None },
            options: DistributionOptions::default(),
        }
    }

    #[test]
    fn cells_read_as_shown() {
        assert_eq!(cell_value("−1.5"), Some(-1.5));
        assert_eq!(cell_value(" 12.5% "), Some(12.5));
        assert_eq!(cell_value("1.50e-7"), Some(1.5e-7));
        assert_eq!(cell_value("—"), None);
        assert_eq!(cell_value("∞"), None);
        assert_eq!(cell_value("a"), None);
    }

    #[test]
    fn standard_error_of_a_mean_through_the_report() {
        // The bootstrap SE of a mean is close to s/√n · √((n − 1)/n).
        let t = crate::sample::longley();
        let rows = RowStateManager::new(t.n_rows());
        let col = 1;
        let x: Vec<f64> = t.column(col).unwrap().as_numeric().unwrap().to_vec();
        let n = x.len() as f64;
        let m = x.iter().sum::<f64>() / n;
        let s = (x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
        let ideal = s / n.sqrt() * ((n - 1.0) / n).sqrt();
        let mut req = BootstrapRequest::new(distribution(col), "Summary Statistics", vec!["Value".into()]);
        req.rows = vec!["Mean".into(), "Standard deviation".into()];
        req.replicates = 4000;
        let a = BootstrapAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.statistics().len(), 2);
        let mean = &a.statistics()[0].summary;
        assert!((mean.std_error / ideal - 1.0).abs() < 0.05, "{} vs {ideal}", mean.std_error);
        assert!((mean.estimate - m).abs() < 1e-4 * m.abs());
        assert!(mean.bca.is_some() && mean.failed == 0);
        let sections = a.sections();
        assert_eq!(sections.len(), 4);
        let SectionContent::Bars(b) = &sections[1].content else { panic!() };
        assert_eq!(b.bars.iter().map(BarItem::count).sum::<usize>(), 4000);
        assert_eq!(b.references.len(), 3);
        // Same seed, same report; the request survives a project round trip.
        let again = BootstrapAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(again.statistics(), a.statistics());
        let back = BootstrapRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn strata_exclusions_and_errors() {
        let t = crate::sample::longley();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([0, 1], true);
        let mut req = BootstrapRequest::new(distribution(1), "Summary Statistics", vec!["Value".into()]);
        req.rows = vec!["Mean".into()];
        req.replicates = 200;
        req.stratify = Some(0);
        let a = BootstrapAnalysis::run(&t, &rows, &req).unwrap();
        assert_eq!(a.rows_used, t.n_rows() - 2);
        assert!(
            a.sections().last().is_some_and(|s| matches!(&s.content, SectionContent::Note(n) if n.contains("levels")))
        );
        assert_eq!(req.columns(), vec![1, 0]);
        assert_eq!(req.clone().map_columns(&|c| c + 1).columns(), vec![2, 1]);
        let bad = |f: &dyn Fn(&mut BootstrapRequest)| {
            let mut r = req.clone();
            f(&mut r);
            BootstrapAnalysis::run(&t, &rows, &r).is_err()
        };
        assert!(bad(&|r| r.section = "No such table".into()));
        assert!(bad(&|r| r.columns = vec!["Nope".into()]));
        assert!(bad(&|r| r.columns.clear()));
        assert!(bad(&|r| r.replicates = 1));
        assert!(bad(&|r| r.level = 1.0));
        assert!(bad(&|r| *r.report = SavedReport::Bootstrap { request: req.clone() }));
    }

    #[test]
    fn failing_replicates_are_counted() {
        // A two-row table: many resamples repeat one row, and the standard
        // deviation of a constant is still defined but the skewness is not.
        let t = DataTable::new("t", vec![Column::numeric("x", vec![1.0, 2.0, 4.0])]).unwrap();
        let rows = RowStateManager::new(3);
        let mut req = BootstrapRequest::new(distribution(0), "Summary Statistics", vec!["Value".into()]);
        req.rows = vec!["Mean".into(), "Skewness (G₁)".into()];
        req.replicates = 300;
        let a = BootstrapAnalysis::run(&t, &rows, &req).unwrap();
        let skew = &a.statistics()[1].summary;
        assert!(skew.failed > 0 && skew.failed + skew.replicates == 300);
        assert_eq!(a.statistics()[0].summary.failed, 0);
        let SectionContent::Table(tab) = &a.sections()[0].content else { panic!() };
        assert_eq!(tab.cell(1, "Failed"), Some(skew.failed.to_string().as_str()));
    }
}
