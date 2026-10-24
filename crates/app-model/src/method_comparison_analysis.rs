//! Method comparison bound to the row state
//! ([`stat_linear::method_comparison`]): Deming and Passing–Bablok
//! regression of a test method on a comparison method, with a scatter plot
//! against the line of identity and a difference (Bland–Altman) plot.
//!
//! Rows that are excluded or miss either measurement are left out.

use crate::format::{format_number, format_p_value};
use crate::report::{Annotations, PlotSpec, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_linear::{ComparisonEstimate, DemingFit, PassingBablokFit, deming_regression, passing_bablok};

const DIGITS: usize = 6;

/// What to analyse.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodComparisonRequest {
    /// Comparison (reference) method, the horizontal axis.
    pub reference: usize,
    /// Test method, the vertical axis.
    pub test: usize,
    /// Fit Deming regression.
    pub deming: bool,
    /// Fit Passing–Bablok regression.
    pub passing_bablok: bool,
    /// Error-variance ratio `Var(test error)/Var(reference error)` for
    /// Deming regression (1: orthogonal regression).
    pub variance_ratio: f64,
    /// Confidence level of the intervals.
    pub confidence_level: f64,
}

impl MethodComparisonRequest {
    /// Both fits, ratio 1, 95% intervals.
    #[must_use]
    pub fn new(reference: usize, test: usize) -> Self {
        Self { reference, test, deming: true, passing_bablok: true, variance_ratio: 1.0, confidence_level: 0.95 }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        vec![self.reference, self.test]
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.reference = f(self.reference);
        self.test = f(self.test);
    }
}

/// A computed method comparison.
#[derive(Debug, Clone)]
pub struct MethodComparisonAnalysis {
    request: MethodComparisonRequest,
    x_name: String,
    y_name: String,
    rows: Vec<usize>,
    x: Vec<f64>,
    y: Vec<f64>,
    deming: Option<DemingFit>,
    passing_bablok: Option<PassingBablokFit>,
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

fn mean_sd(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let m = v.iter().sum::<f64>() / n;
    let ss: f64 = v.iter().map(|x| (x - m) * (x - m)).sum();
    (m, if v.len() > 1 { (ss / (n - 1.0)).sqrt() } else { f64::NAN })
}

impl MethodComparisonAnalysis {
    /// Compute the fits.
    ///
    /// # Errors
    /// Missing or non-numeric columns, no fit chosen, and errors from the
    /// fits.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &MethodComparisonRequest) -> Result<Self> {
        if !request.deming && !request.passing_bablok {
            return Err(invalid("choose Deming regression, Passing–Bablok regression or both"));
        }
        let numeric = |i: usize| -> Result<(&str, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            Ok((c.name.as_str(), c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?))
        };
        let (x_name, xs) = numeric(request.reference)?;
        let (y_name, ys) = numeric(request.test)?;
        let (mut rows, mut x, mut y) = (Vec::new(), Vec::new(), Vec::new());
        let (mut n_excluded, mut n_missing) = (0, 0);
        for r in 0..table.n_rows() {
            if !(xs[r].is_finite() && ys[r].is_finite()) {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                rows.push(r);
                x.push(xs[r]);
                y.push(ys[r]);
            }
        }
        let deming = request
            .deming
            .then(|| deming_regression(&x, &y, request.variance_ratio, request.confidence_level))
            .transpose()?;
        let passing_bablok =
            request.passing_bablok.then(|| passing_bablok(&x, &y, request.confidence_level)).transpose()?;
        Ok(Self {
            request: request.clone(),
            x_name: x_name.to_owned(),
            y_name: y_name.to_owned(),
            rows,
            x,
            y,
            deming,
            passing_bablok,
            n_excluded,
            n_missing,
        })
    }

    /// Deming fit, if requested.
    #[must_use]
    pub fn deming(&self) -> Option<&DemingFit> {
        self.deming.as_ref()
    }

    /// Passing–Bablok fit, if requested.
    #[must_use]
    pub fn passing_bablok(&self) -> Option<&PassingBablokFit> {
        self.passing_bablok.as_ref()
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Method Comparison: {} vs {}", self.y_name, self.x_name)
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        if let Some(d) = &self.deming {
            out.push(self.estimates_section(
                SectionKind::DemingEstimates,
                format!(
                    "Deming Regression (error-variance ratio {}; jackknife intervals)",
                    format_number(d.variance_ratio, 4)
                ),
                [&d.intercept, &d.slope],
            ));
        }
        if let Some(p) = &self.passing_bablok {
            out.push(self.estimates_section(
                SectionKind::PassingBablokEstimates,
                "Passing–Bablok Regression".to_owned(),
                [&p.intercept, &p.slope],
            ));
            if let Some(note) = &p.note {
                out.push(section(
                    SectionKind::PassingBablokNote,
                    "Passing–Bablok Note",
                    SectionContent::Note(note.clone()),
                ));
            }
            out.push(self.linearity(p));
        }
        out.push(self.scatter());
        out.push(self.differences());
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut row = |a: &str, b: String| t.push([a.to_owned(), b]);
        row("Pairs", self.x.len().to_string());
        if self.n_excluded > 0 {
            row("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            row("Rows with missing values", self.n_missing.to_string());
        }
        let (mx, sx) = mean_sd(&self.x);
        let (my, sy) = mean_sd(&self.y);
        row(
            &format!("Mean (SD) of {}", self.x_name),
            format!("{} ({})", format_number(mx, DIGITS), format_number(sx, 4)),
        );
        row(
            &format!("Mean (SD) of {}", self.y_name),
            format!("{} ({})", format_number(my, DIGITS), format_number(sy, 4)),
        );
        let lo = self.x.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = self.x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        row(
            &format!("Range of {}", self.x_name),
            format!("{} to {}", format_number(lo, DIGITS), format_number(hi, DIGITS)),
        );
        let d: Vec<f64> = self.x.iter().zip(&self.y).map(|(a, b)| b - a).collect();
        let (md, sd) = mean_sd(&d);
        row("Mean difference (test − comparison)", format_number(md, DIGITS));
        row("SD of differences", format_number(sd, DIGITS));
        section(SectionKind::MethodComparisonSummary, "Data Summary", SectionContent::Table(t))
    }

    fn estimates_section(&self, kind: SectionKind, title: String, est: [&ComparisonEstimate; 2]) -> ReportSection {
        let level = format_number(100.0 * self.request.confidence_level, 4);
        let mut t = TextTable::new([
            "Term".to_owned(),
            "Estimate".to_owned(),
            "Std Error".to_owned(),
            format!("Lower {level}%"),
            format!("Upper {level}%"),
            "Agreement".to_owned(),
        ]);
        for (e, ideal, bias) in [(est[0], 0.0, "constant"), (est[1], 1.0, "proportional")] {
            let covers = e.lower <= ideal && ideal <= e.upper;
            let verdict = if covers {
                format!("interval contains {ideal}: no {bias} difference shown")
            } else {
                format!("interval excludes {ideal}: {bias} difference")
            };
            t.push([
                e.name.clone(),
                format_number(e.estimate, DIGITS),
                e.std_error.map_or_else(String::new, |s| format_number(s, DIGITS)),
                format_number(e.lower, DIGITS),
                format_number(e.upper, DIGITS),
                verdict,
            ]);
        }
        section(kind, title, SectionContent::Table(t))
    }

    fn linearity(&self, p: &PassingBablokFit) -> ReportSection {
        let content = match &p.linearity {
            Some(l) => {
                let mut t = TextTable::new(["Statistic", "Value"]);
                t.push(["Points above / below the line".to_owned(), format!("{} / {}", l.n_above, l.n_below)]);
                t.push(["Largest |cumulative sum|".to_owned(), format_number(l.max_cusum, 4)]);
                t.push(["Test statistic".to_owned(), format_number(l.statistic, 4)]);
                t.push(["p-value (Kolmogorov)".to_owned(), format_p_value(l.p_value)]);
                t.push(["Conclusion".to_owned(), linearity_conclusion(l.p_value, self.request.confidence_level)]);
                SectionContent::Table(t)
            }
            None => {
                SectionContent::Note("The linearity test needs points on both sides of the fitted line.".to_owned())
            }
        };
        section(SectionKind::LinearityTest, "Linearity Test (cumulative sum)", content)
    }

    fn scatter(&self) -> ReportSection {
        let points: Vec<RowPoint> =
            self.rows.iter().zip(self.x.iter().zip(&self.y)).map(|(&row, (&x, &y))| RowPoint { row, x, y }).collect();
        let lo = self.x.iter().chain(&self.y).copied().fold(f64::INFINITY, f64::min);
        let hi = self.x.iter().chain(&self.y).copied().fold(f64::NEG_INFINITY, f64::max);
        let (xl, xh) = (
            self.x.iter().copied().fold(f64::INFINITY, f64::min),
            self.x.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        let mut segments = vec![vec![[lo, lo], [hi, hi]]];
        let mut labels = vec![(hi, hi, "y = x".to_owned())];
        let mut line = None;
        if let Some(d) = &self.deming {
            line = Some(vec![[xl, d.predict(xl)], [xh, d.predict(xh)]]);
            labels.push((xh, d.predict(xh), "Deming".to_owned()));
        }
        if let Some(p) = &self.passing_bablok {
            let pl = vec![[xl, p.predict(xl)], [xh, p.predict(xh)]];
            if line.is_none() {
                line = Some(pl);
            } else {
                segments.push(pl);
            }
            labels.push((xl, p.predict(xl), "Passing–Bablok".to_owned()));
        }
        let line_name = if self.deming.is_some() { "Deming fit" } else { "Passing–Bablok fit" };
        let spec = PlotSpec {
            x_label: self.x_name.clone(),
            y_label: self.y_name.clone(),
            points,
            line,
            line_name: Some(line_name.to_owned()),
            segments,
            annotations: Some(Annotations { groups: Vec::new(), items: Vec::new(), labels }),
            ..PlotSpec::default()
        };
        section(
            SectionKind::MethodComparisonPlot,
            "Scatter Plot with Fitted Lines",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    /// Differences against means (Bland & Altman 1986, *Lancet* i:307) with
    /// the mean difference and limits of agreement `mean ± 1.96 SD`.
    fn differences(&self) -> ReportSection {
        let d: Vec<f64> = self.x.iter().zip(&self.y).map(|(a, b)| b - a).collect();
        let m: Vec<f64> = self.x.iter().zip(&self.y).map(|(a, b)| 0.5 * (a + b)).collect();
        let (md, sd) = mean_sd(&d);
        let lo = m.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = m.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let points = self.rows.iter().zip(m.iter().zip(&d)).map(|(&row, (&x, &y))| RowPoint { row, x, y }).collect();
        let (la, ua) = (md - 1.96 * sd, md + 1.96 * sd);
        let spec = PlotSpec {
            x_label: format!("Mean of {} and {}", self.x_name, self.y_name),
            y_label: format!("{} − {}", self.y_name, self.x_name),
            points,
            line: Some(vec![[lo, md], [hi, md]]),
            line_name: Some("Mean difference".to_owned()),
            segments: vec![vec![[lo, 0.0], [hi, 0.0]], vec![[lo, la], [hi, la]], vec![[lo, ua], [hi, ua]]],
            annotations: Some(Annotations {
                groups: Vec::new(),
                items: Vec::new(),
                labels: vec![
                    (hi, md, format!("mean {}", format_number(md, 4))),
                    (hi, la, format!("−1.96 SD {}", format_number(la, 4))),
                    (hi, ua, format!("+1.96 SD {}", format_number(ua, 4))),
                ],
            }),
            ..PlotSpec::default()
        };
        section(
            SectionKind::MethodDifferencePlot,
            "Difference Plot (limits of agreement)",
            SectionContent::Plot(Box::new(spec)),
        )
    }
}

/// The linearity test's verdict at significance α = 1 − confidence level.
fn linearity_conclusion(p_value: f64, confidence_level: f64) -> String {
    let alpha = 1.0 - confidence_level;
    if p_value < alpha {
        format!(
            "deviation from linearity (p < {}): Passing–Bablok regression is not applicable",
            format_number(alpha, 4)
        )
    } else {
        "no significant deviation from linearity".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    #[test]
    fn both_fits_and_plots() {
        let x = vec![73.4, 76.0, 71.3, 85.0, 97.2, 118.7, 128.8, 133.3, 140.5, 143.9, f64::NAN];
        let y = vec![87.5, 81.7, 83.1, 86.7, 99.4, 129.2, 134.5, 145.5, 151.0, 158.6, 3.0];
        let t = DataTable::new("m", vec![Column::numeric("Lab", x), Column::numeric("Meter", y)]).unwrap();
        let a =
            MethodComparisonAnalysis::run(&t, &RowStateManager::new(11), &MethodComparisonRequest::new(0, 1)).unwrap();
        assert_eq!(a.title(), "Method Comparison: Meter vs Lab");
        let s = a.sections();
        let kinds: Vec<SectionKind> = s.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::MethodComparisonSummary,
                SectionKind::DemingEstimates,
                SectionKind::PassingBablokEstimates,
                SectionKind::LinearityTest,
                SectionKind::MethodComparisonPlot,
                SectionKind::MethodDifferencePlot,
            ]
        );
        let SectionContent::Table(sum) = &s[0].content else { panic!() };
        assert_eq!(sum.cell(0, "Value"), Some("10"));
        let SectionContent::Plot(p) = &s[4].content else { panic!() };
        assert_eq!(p.points.len(), 10);
        assert_eq!(p.segments.len(), 2, "identity and the second line");
        assert_eq!(p.line_label(), "Deming fit");
        let SectionContent::Plot(d) = &s[5].content else { panic!() };
        assert_eq!(d.line_label(), "Mean difference");
        let only = MethodComparisonRequest { deming: false, ..MethodComparisonRequest::new(0, 1) };
        let b = MethodComparisonAnalysis::run(&t, &RowStateManager::new(11), &only).unwrap();
        assert!(b.deming().is_none() && b.passing_bablok().is_some());
        let none = MethodComparisonRequest { passing_bablok: false, ..only };
        assert!(MethodComparisonAnalysis::run(&t, &RowStateManager::new(11), &none).is_err());
    }

    #[test]
    fn negative_relation_shows_the_passing_bablok_note() {
        let x: Vec<f64> = (1..=20).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|v| -3.0 * v + 0.01 * (v * 1.7).sin()).collect();
        let t = DataTable::new("m", vec![Column::numeric("Lab", x), Column::numeric("Meter", y)]).unwrap();
        let only = MethodComparisonRequest { deming: false, ..MethodComparisonRequest::new(0, 1) };
        let a = MethodComparisonAnalysis::run(&t, &RowStateManager::new(20), &only).unwrap();
        let s = a.sections();
        let note = s.iter().find(|s| s.kind == SectionKind::PassingBablokNote).expect("note section");
        let SectionContent::Note(text) = &note.content else { panic!() };
        assert!(text.contains("not positively related"), "{text}");
        let est = s.iter().find(|s| s.kind == SectionKind::PassingBablokEstimates).unwrap();
        let SectionContent::Table(t) = &est.content else { panic!() };
        assert_eq!(t.rows[1][4], "∞");
    }

    #[test]
    fn linearity_verdict_follows_the_confidence_level() {
        assert_eq!(linearity_conclusion(0.03, 0.99), "no significant deviation from linearity");
        assert!(linearity_conclusion(0.03, 0.95).starts_with("deviation from linearity (p < 0.05)"));
        assert!(linearity_conclusion(0.005, 0.99).starts_with("deviation from linearity (p < 0.01)"));
    }
}
