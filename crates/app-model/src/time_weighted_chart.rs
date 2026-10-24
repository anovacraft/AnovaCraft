//! EWMA and CUSUM control charts of one numeric column, bound to the dataset's
//! row state ([`stat_quality::time_weighted`]).
//!
//! Observations are taken in row order from rows that have a value and are not
//! excluded. The target mean and σ may be given (e.g. from a historical
//! in-control period) or estimated from the data (mean and moving range).
//! Charts are drawn as row-linked scatter plots, so brushing a point selects the
//! row behind it.

use crate::format::format_number;
use crate::report::{
    Annotations, PlotSpec, PointGroups, ReportSection, RowPoint, SectionContent, SectionKind, TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_quality::{CusumChart, EwmaChart, ProcessTarget, cusum_chart, ewma_chart};

/// Significant digits in report tables.
const DIGITS: usize = 5;

/// Which chart, with its design parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeWeightedKind {
    /// EWMA with weight `λ` and limit width `L`.
    Ewma {
        /// Weight of the newest observation, in (0, 1].
        lambda: f64,
        /// Limit width in standard deviations of the statistic.
        width: f64,
    },
    /// Tabular CUSUM with `K = kσ` and `H = hσ`.
    Cusum {
        /// Reference value in units of σ (often half the shift to detect).
        k: f64,
        /// Decision interval in units of σ.
        h: f64,
    },
}

impl TimeWeightedKind {
    /// EWMA with λ = 0.2, L = 3 (a common general-purpose design).
    pub const EWMA_DEFAULT: Self = Self::Ewma { lambda: 0.2, width: 3.0 };
    /// CUSUM with k = 0.5, h = 5 (detects a one-σ shift quickly).
    pub const CUSUM_DEFAULT: Self = Self::Cusum { k: 0.5, h: 5.0 };
}

/// What to chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeWeightedRequest {
    /// Measurement column.
    pub column: usize,
    /// Chart and parameters.
    pub kind: TimeWeightedKind,
    /// Target mean and σ; `None` estimates them from the data.
    pub target: Option<(f64, f64)>,
}

#[derive(Debug, Clone)]
enum Chart {
    Ewma(EwmaChart),
    Cusum(CusumChart),
}

/// A computed EWMA or CUSUM chart.
#[derive(Debug, Clone)]
pub struct TimeWeightedAnalysis {
    request: TimeWeightedRequest,
    name: String,
    /// Dataset row of each observation, in order.
    rows: Vec<usize>,
    /// The observations, in order.
    values: Vec<f64>,
    chart: Chart,
    n_missing: usize,
    n_excluded: usize,
    exclusion_revision: u64,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl TimeWeightedAnalysis {
    /// Compute the chart.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a missing or non-numeric column, no
    /// usable values, invalid parameters, or a σ that cannot be estimated.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &TimeWeightedRequest) -> Result<Self> {
        let col = table.column(request.column).ok_or_else(|| invalid(format!("no column {}", request.column)))?;
        let values = col.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", col.name)))?;
        let present: Vec<usize> = (0..table.n_rows()).filter(|&r| values[r].is_finite()).collect();
        let rows: Vec<usize> = present.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        if rows.is_empty() {
            return Err(invalid(format!("{} has no values in the rows used", col.name)));
        }
        let x: Vec<f64> = rows.iter().map(|&r| values[r]).collect();
        let n_used = rows.len();
        let target = request.target.map(|(mean, sigma)| ProcessTarget { mean, sigma });
        let chart = match request.kind {
            TimeWeightedKind::Ewma { lambda, width } => Chart::Ewma(ewma_chart(&x, target, lambda, width)?),
            TimeWeightedKind::Cusum { k, h } => Chart::Cusum(cusum_chart(&x, target, k, h)?),
        };
        Ok(Self {
            request: *request,
            name: col.name.clone(),
            rows,
            values: x,
            chart,
            n_missing: table.n_rows() - present.len(),
            n_excluded: present.len() - n_used,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &TimeWeightedRequest {
        &self.request
    }

    /// `true` if row exclusion has changed since the chart was computed.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match self.chart {
            Chart::Ewma(_) => format!("EWMA Chart of {}", self.name),
            Chart::Cusum(_) => format!("CUSUM Chart of {}", self.name),
        }
    }

    /// Dataset rows (in sample order) where the chart signals.
    #[must_use]
    pub fn signal_rows(&self) -> Vec<usize> {
        match &self.chart {
            Chart::Ewma(c) => c.points.iter().zip(&self.rows).filter(|(p, _)| p.signal).map(|(_, &r)| r).collect(),
            Chart::Cusum(c) => c
                .points
                .iter()
                .zip(&self.rows)
                .filter(|(p, _)| p.upper_signal || p.lower_signal)
                .map(|(_, &r)| r)
                .collect(),
        }
    }

    /// Report sections: settings and summary, the chart, and the signals.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![self.summary(), self.plot(), self.signal_table()]
    }

    fn target(&self) -> ProcessTarget {
        match &self.chart {
            Chart::Ewma(c) => c.target,
            Chart::Cusum(c) => c.target,
        }
    }

    fn summary(&self) -> ReportSection {
        let t = self.target();
        let source = if self.request.target.is_some() { "given" } else { "estimated from the data" };
        let mut table = TextTable::new(["Setting", "Value"]);
        table.push([format!("Target mean ({source})"), format_number(t.mean, DIGITS)]);
        let sigma_source = if self.request.target.is_some() { "given" } else { "moving range ÷ d₂" };
        table.push([format!("σ ({sigma_source})"), format_number(t.sigma, DIGITS)]);
        match &self.chart {
            Chart::Ewma(c) => {
                table.push(["Weight λ".to_owned(), format_number(c.lambda, 4)]);
                table.push(["Limit width L".to_owned(), format_number(c.width, 4)]);
            }
            Chart::Cusum(c) => {
                table.push(["Reference value K = kσ".to_owned(), format_number(c.reference, DIGITS)]);
                table.push(["Decision interval H = hσ".to_owned(), format_number(c.decision, DIGITS)]);
            }
        }
        table.push(["Observations".to_owned(), self.rows.len().to_string()]);
        if self.n_excluded > 0 {
            table.push(["Rows excluded".to_owned(), self.n_excluded.to_string()]);
        }
        if self.n_missing > 0 {
            table.push(["Rows without a value".to_owned(), self.n_missing.to_string()]);
        }
        let signals = self.signal_rows();
        table.push(["Samples signalling".to_owned(), signals.len().to_string()]);
        if let Chart::Cusum(c) = &self.chart {
            if let Some((i, mean)) = c.shift_estimate() {
                let p = &c.points[i];
                let run = if p.upper_signal { p.upper_run } else { p.lower_run };
                table.push([
                    "First signal".to_owned(),
                    match i + 1 - run {
                        // The run reaches back to the first sample: there is
                        // no "sample 0" to begin after.
                        0 => {
                            format!("sample {} (row {}); shift present from the first sample", i + 1, self.rows[i] + 1)
                        }
                        start => {
                            format!("sample {} (row {}); shift began after sample {start}", i + 1, self.rows[i] + 1)
                        }
                    },
                ]);
                table.push(["Estimated new mean".to_owned(), format_number(mean, DIGITS)]);
            }
        } else if let Some(&r) = signals.first() {
            let i = self.rows.iter().position(|&x| x == r).unwrap_or(0);
            table.push(["First signal".to_owned(), format!("sample {} (row {})", i + 1, r + 1)]);
        }
        section(SectionKind::TimeWeightedSummary, "Chart Settings and Summary", SectionContent::Table(table))
    }

    fn plot(&self) -> ReportSection {
        let n = self.rows.len();
        let x = |i: usize| (i + 1) as f64;
        let last = x(n - 1);
        let t = self.target();
        let mut segments = Vec::new();
        let mut labels = Vec::new();
        let mut items = Vec::new();
        let (spec_points, groups, y_label, title) = match &self.chart {
            Chart::Ewma(c) => {
                let pts: Vec<RowPoint> = c
                    .points
                    .iter()
                    .zip(&self.rows)
                    .enumerate()
                    .map(|(i, (p, &row))| RowPoint { row, x: x(i), y: p.statistic })
                    .collect();
                segments.push(c.points.iter().enumerate().map(|(i, p)| [x(i), p.upper]).collect());
                segments.push(c.points.iter().enumerate().map(|(i, p)| [x(i), p.lower]).collect());
                segments.push(vec![[1.0, t.mean], [last, t.mean]]);
                segments.push(pts.iter().map(|p| [p.x, p.y]).collect());
                let end = c.points[n - 1];
                labels.push((last, end.upper, format!("UCL {}", format_number(end.upper, 4))));
                labels.push((last, end.lower, format!("LCL {}", format_number(end.lower, 4))));
                for (i, p) in c.points.iter().enumerate().filter(|(_, p)| p.signal) {
                    items.push((x(i), p.statistic, String::new(), 0));
                }
                (pts, None, "EWMA".to_owned(), format!("EWMA (λ = {}, L = {})", c.lambda, c.width))
            }
            Chart::Cusum(c) => {
                // Upper sums above zero, lower sums drawn below it.
                let mut pts = Vec::with_capacity(2 * n);
                let mut membership = Vec::with_capacity(2 * n);
                for (i, (p, &row)) in c.points.iter().zip(&self.rows).enumerate() {
                    pts.push(RowPoint { row, x: x(i), y: p.upper });
                    membership.push(0);
                    pts.push(RowPoint { row, x: x(i), y: -p.lower });
                    membership.push(1);
                }
                let up: Vec<[f64; 2]> = c.points.iter().enumerate().map(|(i, p)| [x(i), p.upper]).collect();
                let down: Vec<[f64; 2]> = c.points.iter().enumerate().map(|(i, p)| [x(i), -p.lower]).collect();
                segments.push(vec![[1.0, c.decision], [last, c.decision]]);
                segments.push(vec![[1.0, -c.decision], [last, -c.decision]]);
                segments.push(vec![[1.0, 0.0], [last, 0.0]]);
                labels.push((last, c.decision, format!("H = {}", format_number(c.decision, 4))));
                labels.push((last, -c.decision, format!("−H = {}", format_number(-c.decision, 4))));
                for (i, p) in c.points.iter().enumerate() {
                    if p.upper_signal {
                        items.push((x(i), p.upper, String::new(), 0));
                    }
                    if p.lower_signal {
                        items.push((x(i), -p.lower, String::new(), 0));
                    }
                }
                let groups = PointGroups {
                    title: "Sum".to_owned(),
                    labels: vec!["C⁺ (upward)".to_owned(), "−C⁻ (downward)".to_owned()],
                    membership,
                    lines: vec![up, down],
                };
                (pts, Some(groups), "Cumulative sum".to_owned(), format!("CUSUM (k = {}, h = {})", c_k(c), c_h(c)))
            }
        };
        let spec = PlotSpec {
            x_time: None,
            y_time: None,
            x_label: "Sample".to_owned(),
            y_label,
            points: spec_points,
            line: None,
            band: None,
            groups,
            x_ticks: None,
            panes: Vec::new(),
            annotations: Some(Annotations { groups: vec!["Signal".to_owned()], items, labels }),
            segments,
            ..PlotSpec::default()
        };
        section(SectionKind::TimeWeightedChart, title, SectionContent::Plot(Box::new(spec)))
    }

    fn signal_table(&self) -> ReportSection {
        let mut table = match &self.chart {
            Chart::Ewma(_) => TextTable::new(["Sample", "Row", "Value", "EWMA", "Limit crossed"]),
            Chart::Cusum(_) => TextTable::new(["Sample", "Row", "Value", "C⁺", "C⁻", "Direction"]),
        };
        let col_value = |i: usize| self.values[i];
        match &self.chart {
            Chart::Ewma(c) => {
                for (i, p) in c.points.iter().enumerate().filter(|(_, p)| p.signal) {
                    let which = if p.statistic > p.upper { "upper" } else { "lower" };
                    table.push([
                        (i + 1).to_string(),
                        (self.rows[i] + 1).to_string(),
                        format_number(col_value(i), DIGITS),
                        format_number(p.statistic, DIGITS),
                        which.to_owned(),
                    ]);
                }
            }
            Chart::Cusum(c) => {
                for (i, p) in c.points.iter().enumerate().filter(|(_, p)| p.upper_signal || p.lower_signal) {
                    let dir = if p.upper_signal { "upward" } else { "downward" };
                    table.push([
                        (i + 1).to_string(),
                        (self.rows[i] + 1).to_string(),
                        format_number(col_value(i), DIGITS),
                        format_number(p.upper, DIGITS),
                        format_number(p.lower, DIGITS),
                        dir.to_owned(),
                    ]);
                }
            }
        }
        if table.rows.is_empty() {
            return section(
                SectionKind::TimeWeightedSignals,
                "Signals",
                SectionContent::Note("No sample signals: the process looks in control at this sensitivity.".to_owned()),
            );
        }
        section(SectionKind::TimeWeightedSignals, "Signals", SectionContent::Table(table))
    }
}

fn c_k(c: &CusumChart) -> String {
    format_number(c.reference / c.target.sigma, 4)
}

fn c_h(c: &CusumChart) -> String {
    format_number(c.decision / c.target.sigma, 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    /// Montgomery's CUSUM example data (see `stat_quality::time_weighted`), with
    /// a missing value and an excluded row added that the chart must skip.
    fn table() -> DataTable {
        let mut v = vec![
            9.45, 7.99, 9.29, 11.66, 12.16, 10.18, 8.04, 11.46, 9.20, 10.34, 9.03, 11.47, 10.51, 9.40, 10.08, 9.37,
            10.62, 10.31, 8.52, 10.84, 10.90, 9.33, 12.29, 11.50, 10.60, 11.08, 10.38, 11.62, 11.31, 10.52,
        ];
        v.insert(3, f64::NAN);
        v.insert(10, 99.0);
        DataTable::new("cusum", vec![Column::numeric("x", v)]).unwrap()
    }

    /// A shift present from the first sample read "began after sample 0".
    #[test]
    fn cusum_shift_from_the_start() {
        let t = DataTable::new("t", vec![stat_data::Column::numeric("x", vec![13.0; 12])]).unwrap();
        let request =
            TimeWeightedRequest { column: 0, kind: TimeWeightedKind::CUSUM_DEFAULT, target: Some((10.0, 1.0)) };
        let a = TimeWeightedAnalysis::run(&t, &RowStateManager::new(12), &request).unwrap();
        let SectionContent::Table(summary) = &a.sections()[0].content else { panic!() };
        let first = summary.rows.iter().find(|r| r[0] == "First signal").unwrap();
        assert!(first[1].ends_with("shift present from the first sample"), "{}", first[1]);
    }

    #[test]
    fn cusum_report_with_given_target() {
        let t = table();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([10], true);
        let request =
            TimeWeightedRequest { column: 0, kind: TimeWeightedKind::CUSUM_DEFAULT, target: Some((10.0, 1.0)) };
        let a = TimeWeightedAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.title(), "CUSUM Chart of x");
        let s = a.sections();
        assert_eq!(s.len(), 3);
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert_eq!(summary.cell(0, "Value"), Some("10"));
        assert_eq!(summary.cell(4, "Value"), Some("30"), "missing and excluded rows skipped");
        assert_eq!(summary.cell(5, "Value"), Some("1"), "rows excluded");
        let first = summary.rows.iter().find(|r| r[0] == "First signal").unwrap();
        assert!(first[1].starts_with("sample 29 (row 31)") && first[1].ends_with("after sample 22"), "{}", first[1]);
        let SectionContent::Plot(p) = &s[1].content else { panic!() };
        assert_eq!(p.points.len(), 60, "upper and lower sums for each sample");
        assert_eq!(p.annotations.as_ref().unwrap().items.len(), 2, "samples 29 and 30 signal");
        let SectionContent::Table(signals) = &s[2].content else { panic!() };
        assert_eq!(signals.rows.len(), 2);
        assert_eq!(signals.cell(0, "C⁺"), Some("5.28"));
        assert_eq!(a.signal_rows(), vec![30, 31]);
    }

    #[test]
    fn ewma_report_estimates_its_target() {
        let t = table();
        let rows = RowStateManager::new(t.n_rows());
        let request = TimeWeightedRequest {
            column: 0,
            kind: TimeWeightedKind::Ewma { lambda: 0.1, width: 2.7 },
            target: Some((10.0, 1.0)),
        };
        let mut no_outlier = rows.clone();
        no_outlier.set_excluded([10], true);
        let a = TimeWeightedAnalysis::run(&t, &no_outlier, &request).unwrap();
        assert_eq!(a.signal_rows(), vec![30, 31]);
        assert!(a.is_stale(&rows));
        // With the 99.0 outlier included and the target estimated, the chart still runs.
        let est = TimeWeightedAnalysis::run(&t, &rows, &TimeWeightedRequest { target: None, ..request }).unwrap();
        let SectionContent::Table(summary) = &est.sections()[0].content else { panic!() };
        assert!(summary.rows[0][0].contains("estimated"));
        assert!(matches!(&est.sections()[2].content, SectionContent::Note(_) | SectionContent::Table(_)));
        // Bad input.
        assert!(
            TimeWeightedAnalysis::run(&t, &rows, &TimeWeightedRequest { column: 3, ..request }).is_err(),
            "no such column"
        );
        let bad = TimeWeightedRequest { kind: TimeWeightedKind::Ewma { lambda: 0.0, width: 3.0 }, ..request };
        assert!(TimeWeightedAnalysis::run(&t, &rows, &bad).is_err());
    }
}
