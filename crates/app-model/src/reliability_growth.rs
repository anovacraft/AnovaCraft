//! Reliability growth analysis bound to a dataset
//! ([`stat_reliability::growth`]): the power-law (Crow–AMSAA) process fitted
//! to the cumulative failure times of one system under development, per
//! phase, with cumulative-failure, intensity and MTBF plots and the
//! Cramér–von Mises goodness-of-fit test.
//!
//! **Layout:** one row per failure, with its cumulative test time. An
//! optional event column marks rows that are not failures (value 0): such a
//! row records the end of the test (or of its phase) without a failure, so
//! that phase is time-terminated there. A phase without one ends at its last
//! failure (failure-terminated), unless it is the last phase and an end time
//! is given. An optional phase column splits the test where the design
//! changed; phases follow each other in time on the same cumulative clock,
//! each starting where the one before ended.
//!
//! Excluded rows and rows missing the time, event or phase are left out.
//! Each failure is drawn as a point linked to its row.

use crate::format::{format_number, format_p_value};
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, RowPoint, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_linear::natural_order;
use stat_reliability::growth::{BetaEstimator, PowerLawProcess, Termination, fit_power_law};

const DIGITS: usize = 5;

/// A phase's `(row, time)` failures and its end-of-test times.
type PhaseRecords = (Vec<(usize, f64)>, Vec<f64>);
/// Points per phase on the fitted curves.
const CURVE_POINTS: usize = 120;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

/// Columns and settings of a reliability growth analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct GrowthRequest {
    /// Cumulative test time of each failure (or end-of-test record).
    pub time: usize,
    /// Optional numeric column: 0 marks an end of test without a failure,
    /// any other value a failure.
    pub event: Option<usize>,
    /// Optional phase column.
    pub phase: Option<usize>,
    /// End of the test (time-terminated) when no end-of-test row says so;
    /// applies to the last phase.
    pub end_time: Option<f64>,
    /// Which shape estimate the fitted curves use.
    pub estimator: BetaEstimator,
    /// Confidence level of the intervals.
    pub confidence: f64,
}

impl GrowthRequest {
    /// Default confidence level.
    pub const DEFAULT_CONFIDENCE: f64 = 0.95;

    /// A request for failure times in `time`, failure-terminated, one phase.
    #[must_use]
    pub fn new(time: usize) -> Self {
        Self {
            time,
            event: None,
            phase: None,
            end_time: None,
            estimator: BetaEstimator::Unbiased,
            confidence: Self::DEFAULT_CONFIDENCE,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        std::iter::once(self.time).chain(self.event).chain(self.phase).collect()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.time = f(self.time);
        self.event = self.event.map(f);
        self.phase = self.phase.map(f);
    }
}

/// One fitted phase with its failure rows.
#[derive(Debug, Clone)]
pub struct GrowthPhase {
    /// Phase label (empty without a phase column).
    pub label: String,
    /// `(row, time)` of each failure, in time order.
    pub failures: Vec<(usize, f64)>,
    /// Failures before the phase started.
    pub prior_failures: usize,
    /// The fit.
    pub fit: PowerLawProcess,
}

/// A computed reliability growth analysis.
#[derive(Debug, Clone)]
pub struct GrowthAnalysis {
    request: GrowthRequest,
    time_name: String,
    phase_name: Option<String>,
    phases: Vec<GrowthPhase>,
    n_rows: usize,
    n_skipped: usize,
}

impl GrowthAnalysis {
    /// Fit the power-law process to each phase.
    ///
    /// # Errors
    /// Missing or non-numeric columns, no usable rows, an end time before a
    /// failure, overlapping phases, and errors from
    /// [`stat_reliability::growth::fit_power_law`] (named by phase).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &GrowthRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let numeric = |i: usize| -> Result<&[f64]> {
            let c = column(i)?;
            c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))
        };
        let time_col = column(request.time)?;
        let times = numeric(request.time)?;
        let events = request.event.map(numeric).transpose()?;
        let phase_col = request.phase.map(column).transpose()?;
        if request.event == Some(request.time) || request.phase == Some(request.time) {
            return Err(invalid("the event and phase columns must differ from the time column"));
        }
        // label → (failure rows, end-of-test rows)
        let mut labels: Vec<String> = Vec::new();
        let mut members: Vec<PhaseRecords> = Vec::new();
        let mut n_rows = 0;
        for r in 0..table.n_rows() {
            let t = times[r];
            let e = events.map_or(1.0, |e| e[r]);
            let label = phase_col.map(|c| c.level_text(r)).unwrap_or_default();
            if states.is_excluded(r)
                || !t.is_finite()
                || t <= 0.0
                || !e.is_finite()
                || (phase_col.is_some() && label.is_empty())
            {
                continue;
            }
            n_rows += 1;
            let k = labels.iter().position(|l| *l == label).unwrap_or_else(|| {
                labels.push(label);
                members.push((Vec::new(), Vec::new()));
                labels.len() - 1
            });
            if e != 0.0 {
                members[k].0.push((r, t));
            } else {
                members[k].1.push(t);
            }
        }
        if n_rows == 0 {
            return Err(invalid("no rows with a usable time (finite and positive)"));
        }
        // Phases in time order (by their earliest record).
        let first = |m: &PhaseRecords| m.0.iter().map(|x| x.1).chain(m.1.iter().copied()).fold(f64::INFINITY, f64::min);
        let mut order: Vec<usize> = (0..labels.len()).collect();
        order.sort_by(|&a, &b| {
            first(&members[a]).total_cmp(&first(&members[b])).then(natural_order(&labels[a], &labels[b]))
        });
        let mut phases = Vec::new();
        let (mut start, mut prior) = (0.0, 0usize);
        let last = order.len() - 1;
        for (k, &g) in order.iter().enumerate() {
            let (mut failures, ends) = std::mem::take(&mut members[g]);
            failures.sort_by(|a, b| a.1.total_cmp(&b.1));
            let marked_end = ends.iter().copied().fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))));
            let end = marked_end.or(if k == last { request.end_time } else { None });
            let name = if labels[g].is_empty() { "the test".to_owned() } else { format!("phase {}", labels[g]) };
            let fit = fit_power_law(
                &failures.iter().map(|x| x.1).collect::<Vec<_>>(),
                start,
                end,
                request.estimator,
                request.confidence,
            )
            .map_err(|e| invalid(format!("{name}: {e}")))?;
            start = fit.end;
            let n = failures.len();
            phases.push(GrowthPhase { label: labels[g].clone(), failures, prior_failures: prior, fit });
            prior += n;
        }
        Ok(Self {
            request: request.clone(),
            time_name: time_col.name.clone(),
            phase_name: phase_col.map(|c| c.name.clone()),
            phases,
            n_rows,
            n_skipped: table.n_rows() - n_rows,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &GrowthRequest {
        &self.request
    }

    /// The fitted phases, in time order.
    #[must_use]
    pub fn phases(&self) -> &[GrowthPhase] {
        &self.phases
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.phase_name {
            Some(p) => format!("Reliability Growth: {} by {p}", self.time_name),
            None => format!("Reliability Growth: {}", self.time_name),
        }
    }

    fn phase_heading(&self, p: &GrowthPhase) -> String {
        if p.label.is_empty() { "Estimate".to_owned() } else { p.label.clone() }
    }

    /// Report sections: estimates, the three plots and goodness of fit.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        vec![
            self.estimates(),
            self.cumulative_plot(),
            self.curve_plot(false),
            self.curve_plot(true),
            self.goodness_of_fit(),
        ]
    }

    fn level(&self) -> String {
        format!("{}%", format_number(self.request.confidence * 100.0, 4))
    }

    fn estimates(&self) -> ReportSection {
        let mut columns = vec!["Statistic".to_owned()];
        columns.extend(self.phases.iter().map(|p| self.phase_heading(p)));
        let mut t = TextTable::new(columns);
        let level = self.level();
        let mut row = |name: String, f: &dyn Fn(&GrowthPhase) -> String| {
            let mut cells = vec![name];
            cells.extend(self.phases.iter().map(f));
            t.push(cells);
        };
        let num = |v: f64| format_number(v, DIGITS);
        row("Start".into(), &|p| num(p.fit.start));
        row("End".into(), &|p| num(p.fit.end));
        row("Ended by".into(), &|p| {
            match p.fit.termination {
                Termination::Time => "time",
                Termination::Failure => "failure",
            }
            .to_owned()
        });
        row("Failures".into(), &|p| p.fit.n.to_string());
        row("Shape β (maximum likelihood)".into(), &|p| num(p.fit.beta_mle));
        row("Shape β used".into(), &|p| format!("{} ({})", num(p.fit.beta), p.fit.estimator.label()));
        row(format!("β lower {level}"), &|p| num(p.fit.beta_interval.0));
        row(format!("β upper {level}"), &|p| num(p.fit.beta_interval.1));
        row("β interval".into(), &|p| {
            if p.fit.beta_interval_exact { "exact (χ²)" } else { "Wald (log scale)" }.to_owned()
        });
        row("Scale λ".into(), &|p| num(p.fit.lambda()));
        row("Intensity at end".into(), &|p| num(p.fit.intensity(p.fit.end)));
        row("MTBF at end".into(), &|p| num(p.fit.mtbf(p.fit.end)));
        row(format!("MTBF lower {level}"), &|p| num(p.fit.mtbf_interval(p.fit.end).0));
        row(format!("MTBF upper {level}"), &|p| num(p.fit.mtbf_interval(p.fit.end).1));
        row("Trend".into(), &|p| {
            let (lo, hi) = p.fit.beta_interval;
            if hi < 1.0 {
                "growing (β < 1)"
            } else if lo > 1.0 {
                "deteriorating (β > 1)"
            } else {
                "no clear trend (interval holds 1)"
            }
            .to_owned()
        });
        let skipped = if self.n_skipped > 0 {
            format!("; {} rows excluded or incomplete", self.n_skipped)
        } else {
            String::new()
        };
        section(
            SectionKind::GrowthEstimates,
            format!(
                "Power-Law Estimates (intensity u(t) = λβt^(β−1); {} rows{skipped}; MTBF limits: Wald, log scale)",
                self.n_rows
            ),
            SectionContent::Table(t),
        )
    }

    fn grid(p: &GrowthPhase) -> Vec<f64> {
        let (s, e) = (p.fit.start, p.fit.end);
        // Start just after zero: the intensity is infinite there when β < 1.
        let s0 = if s > 0.0 { s } else { e * 1e-3 };
        (0..=CURVE_POINTS).map(|i| s0 + (e - s0) * i as f64 / CURVE_POINTS as f64).collect()
    }

    fn grouped(&self) -> bool {
        self.phases.len() > 1
    }

    fn groups(&self, lines: Vec<Polyline>) -> Option<PointGroups> {
        self.grouped().then(|| PointGroups {
            title: self.phase_name.clone().unwrap_or_default(),
            labels: self.phases.iter().map(|p| p.label.clone()).collect(),
            membership: self
                .phases
                .iter()
                .enumerate()
                .flat_map(|(k, p)| std::iter::repeat_n(k, p.failures.len()))
                .collect(),
            lines,
        })
    }

    fn cumulative_plot(&self) -> ReportSection {
        let mut points = Vec::new();
        let mut lines = Vec::new();
        for p in &self.phases {
            for (i, &(row, t)) in p.failures.iter().enumerate() {
                points.push(RowPoint { row, x: t, y: (p.prior_failures + i + 1) as f64 });
            }
            let offset = p.prior_failures as f64;
            let mut line: Polyline = vec![[p.fit.start, offset]];
            line.extend(Self::grid(p).into_iter().map(|t| [t, offset + p.fit.expected_failures(t)]));
            lines.push(line);
        }
        let line = (!self.grouped()).then(|| lines[0].clone());
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: "Cumulative failures".to_owned(),
            points,
            line,
            line_name: Some("Fitted λt^β".to_owned()),
            point_name: Some("Failures".to_owned()),
            groups: self.groups(lines),
            ..PlotSpec::default()
        };
        section(
            SectionKind::GrowthCumulative,
            "Cumulative Failures (points: observed; line: fitted λt^β)",
            SectionContent::Plot(Box::new(spec)),
        )
    }

    /// Intensity (`mtbf == false`) or MTBF plot with Wald limits.
    fn curve_plot(&self, mtbf: bool) -> ReportSection {
        let value = |p: &GrowthPhase, t: f64| if mtbf { p.fit.mtbf(t) } else { p.fit.intensity(t) };
        let limits = |p: &GrowthPhase, t: f64| if mtbf { p.fit.mtbf_interval(t) } else { p.fit.intensity_interval(t) };
        let mut points = Vec::new();
        let (mut lines, mut lower, mut upper) = (Vec::new(), Vec::new(), Vec::new());
        for p in &self.phases {
            for &(row, t) in &p.failures {
                points.push(RowPoint { row, x: t, y: value(p, t) });
            }
            let grid = Self::grid(p);
            // Leave out the steep start of a curve from zero, which would
            // dwarf the rest (the first 2% of the phase).
            let shown: Vec<f64> =
                if p.fit.start > 0.0 { grid } else { grid.into_iter().filter(|&t| t >= 0.02 * p.fit.end).collect() };
            lines.push(shown.iter().map(|&t| [t, value(p, t)]).collect::<Polyline>());
            for &t in &shown {
                let (lo, hi) = limits(p, t);
                lower.push([t, lo]);
                upper.push([t, hi]);
            }
        }
        let line = (!self.grouped()).then(|| lines[0].clone());
        let (kind, title, y_label) = if mtbf {
            (
                SectionKind::GrowthMtbf,
                "Instantaneous MTBF (1/u(t); markers: failures; dashed: confidence limits)",
                "MTBF",
            )
        } else {
            (
                SectionKind::GrowthIntensity,
                "Failure Intensity (u(t) = λβt^(β−1); markers: failures; dashed: confidence limits)",
                "Failure intensity",
            )
        };
        let spec = PlotSpec {
            x_label: self.time_name.clone(),
            y_label: y_label.to_owned(),
            points,
            line,
            line_name: Some(if mtbf { "Fitted MTBF" } else { "Fitted intensity" }.to_owned()),
            band: Some((lower, upper)),
            band_name: Some(format!("{} confidence limits", self.level())),
            point_name: Some("Failures".to_owned()),
            groups: self.groups(lines),
            ..PlotSpec::default()
        };
        section(kind, format!("{title}, {} limits", self.level()), SectionContent::Plot(Box::new(spec)))
    }

    fn goodness_of_fit(&self) -> ReportSection {
        let mut t =
            TextTable::new(["Phase", "M", "Cramér–von Mises C²", "Critical value", "α", "p-value", "Conclusion"]);
        for p in &self.phases {
            let label = if p.label.is_empty() { "All".to_owned() } else { p.label.clone() };
            match &p.fit.cramer_von_mises {
                Some(c) => t.push([
                    label,
                    c.m.to_string(),
                    format_number(c.statistic, 4),
                    format_number(c.critical_value, 4),
                    format_number(c.alpha, 3),
                    format_p_value(c.p_value),
                    if c.statistic > c.critical_value {
                        "the power law fits poorly"
                    } else {
                        "consistent with the power law"
                    }
                    .to_owned(),
                ]),
                None => t.push([
                    label,
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    if p.fit.start > 0.0 { "not available for a phase after the first" } else { "too few failures" }
                        .to_owned(),
                ]),
            }
        }
        section(
            SectionKind::GrowthGoodnessOfFit,
            "Goodness of Fit (Cramér–von Mises with the unbiased β; critical value and p-value simulated)",
            SectionContent::Table(t),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::reliability_growth;

    fn col(t: &DataTable, name: &str) -> usize {
        t.find(name).unwrap()
    }

    #[test]
    fn single_phase_matches_engine() {
        let t = reliability_growth();
        let rows = RowStateManager::new(t.n_rows());
        let request = GrowthRequest { event: Some(col(&t, "Failure")), ..GrowthRequest::new(col(&t, "Test hours")) };
        let a = GrowthAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.phases().len(), 1);
        let p = &a.phases()[0];
        // The end-of-test rows make the test time-terminated at the latest one.
        assert_eq!(p.fit.termination, Termination::Time);
        let times: Vec<f64> = p.failures.iter().map(|x| x.1).collect();
        let direct = fit_power_law(&times, 0.0, Some(p.fit.end), BetaEstimator::Unbiased, 0.95).unwrap();
        assert_eq!(p.fit, direct);
        let s = a.sections();
        assert_eq!(s.len(), 5);
        let SectionContent::Plot(plot) = &s[1].content else { panic!() };
        assert_eq!(plot.points.len(), p.failures.len());
        assert_eq!(plot.points.last().unwrap().y, p.failures.len() as f64);
        let line = plot.line.as_ref().unwrap();
        assert!((line.last().unwrap()[1] - p.failures.len() as f64).abs() < 1e-9);
    }

    #[test]
    fn phases_follow_each_other() {
        let t = reliability_growth();
        let rows = RowStateManager::new(t.n_rows());
        let request = GrowthRequest {
            event: Some(col(&t, "Failure")),
            phase: Some(col(&t, "Phase")),
            ..GrowthRequest::new(col(&t, "Test hours"))
        };
        let a = GrowthAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.phases().len(), 2);
        let (p1, p2) = (&a.phases()[0], &a.phases()[1]);
        assert_eq!(p1.fit.start, 0.0);
        assert_eq!(p2.fit.start, p1.fit.end);
        assert_eq!(p2.prior_failures, p1.failures.len());
        // The design change shows: the second phase's MTBF at its end beats
        // the first's.
        assert!(p2.fit.mtbf(p2.fit.end) > p1.fit.mtbf(p1.fit.end));
        // Both phases show growth (β < 1) and fit the power law.
        assert!(p1.fit.beta < 1.0 && p2.fit.beta < 1.0, "{} {}", p1.fit.beta, p2.fit.beta);
        assert!(p1.fit.cramer_von_mises.unwrap().p_value > 0.05);
        let s = a.sections();
        let SectionContent::Plot(plot) = &s[2].content else { panic!() };
        assert_eq!(plot.groups.as_ref().unwrap().lines.len(), 2);
        assert!(a.title().contains("by Phase"));
        let SectionContent::Table(gof) = &s[4].content else { panic!() };
        assert_eq!(gof.rows.len(), 2);
        assert_eq!(gof.rows[1][1], "—");
    }

    #[test]
    fn exclusion_and_errors() {
        let t = reliability_growth();
        let mut rows = RowStateManager::new(t.n_rows());
        let time = col(&t, "Test hours");
        let a = GrowthAnalysis::run(&t, &rows, &GrowthRequest::new(time)).unwrap();
        // Without the event column the end-of-test rows count as failures.
        let n_all = a.phases()[0].failures.len();
        rows.set_excluded([0], true);
        let b = GrowthAnalysis::run(&t, &rows, &GrowthRequest::new(time)).unwrap();
        assert_eq!(b.phases()[0].failures.len(), n_all - 1);
        let bad = GrowthRequest { end_time: Some(1.0), ..GrowthRequest::new(time) };
        assert!(GrowthAnalysis::run(&t, &rows, &bad).is_err());
        let same = GrowthRequest { event: Some(time), ..GrowthRequest::new(time) };
        assert!(GrowthAnalysis::run(&t, &rows, &same).is_err());
    }
}
