//! Attribute control charts from a count column (and a subgroup-size column
//! or a fixed size), one subgroup per row, bound to the row state
//! ([`stat_quality::attribute`]): p, np, c and u charts, Laney's p′ and u′
//! charts for overdispersed counts, and the rare-event g and t charts (one
//! row per gap between events).
//!
//! Rows with a count (and size) that are not excluded are charted in row
//! order. An optional **phase** column splits the rows into runs of equal
//! values, each with its own center line and limits (and its own σ_z on a
//! Laney chart); run rules restart at each phase. Run rules apply to the
//! plotted statistic (the transformed times on a t chart). Every point
//! carries its row, so brushing a point selects its row.

use crate::control_chart::RuleSet;
use crate::format::format_number;
use crate::report::{
    ChartPoint, ControlChartSpec, LimitStep, PhaseMark, ReportSection, SectionContent, SectionKind, TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_quality::{AttributeChart, AttributeChartKind, RuleViolation, attribute_chart, varying_violations};

/// Significant digits in report tables.
const DIGITS: usize = 5;

/// Where the subgroup sizes come from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SubgroupSize {
    /// Not needed (c, g and t charts).
    None,
    /// A column of sizes, one per row.
    Column(usize),
    /// The same size for every subgroup.
    Constant(f64),
}

/// What to chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeChartRequest {
    /// Chart type.
    pub kind: AttributeChartKind,
    /// Column of counts (nonconforming units, nonconformities, units between
    /// events, or times between events).
    pub count: usize,
    /// Subgroup sizes (units inspected) for p, np, u, p′ and u′ charts.
    pub size: SubgroupSize,
    /// Phase column: each run of equal values gets its own limits.
    pub phase: Option<usize>,
    /// Run rules applied (rule 1, a point beyond a limit, by default).
    pub rules: RuleSet,
}

impl AttributeChartRequest {
    /// A request without a phase column, applying rule 1 only.
    #[must_use]
    pub fn new(kind: AttributeChartKind, count: usize, size: SubgroupSize) -> Self {
        Self { kind, count, size, phase: None, rules: RuleSet::LIMITS_ONLY }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.count];
        if let SubgroupSize::Column(s) = self.size {
            c.push(s);
        }
        c.extend(self.phase);
        c
    }
}

/// One phase of the chart.
#[derive(Debug, Clone)]
struct Phase {
    name: String,
    start: usize,
    end: usize,
    chart: AttributeChart,
}

/// A computed attribute chart.
#[derive(Debug, Clone)]
pub struct AttributeChartAnalysis {
    request: AttributeChartRequest,
    name: String,
    /// Dataset row of each subgroup.
    rows: Vec<usize>,
    counts: Vec<f64>,
    sizes: Vec<f64>,
    phases: Vec<Phase>,
    n_excluded: usize,
    n_missing: usize,
    exclusion_revision: u64,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl AttributeChartAnalysis {
    /// Compute the chart.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for missing or non-numeric columns, a
    /// chart that needs sizes without any, and any error from
    /// [`attribute_chart`] (in any phase).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &AttributeChartRequest) -> Result<Self> {
        let numeric = |i: usize| -> Result<(String, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
            Ok((c.name.clone(), v))
        };
        let (name, counts_all) = numeric(request.count)?;
        let size_column = match request.size {
            SubgroupSize::Column(i) if request.kind.needs_size() => {
                if i == request.count {
                    return Err(invalid("the size column must differ from the count column"));
                }
                Some(numeric(i)?.1)
            }
            _ => None,
        };
        if request.kind.needs_size() && request.size == SubgroupSize::None {
            return Err(invalid(format!("a {} chart needs subgroup sizes", request.kind.short())));
        }
        let phase_col =
            request.phase.map(|i| table.column(i).ok_or_else(|| invalid(format!("no column {i}")))).transpose()?;
        let phase_of = |r: usize| phase_col.map(|c| c.level_text(r)).unwrap_or_default();
        let present: Vec<usize> = (0..table.n_rows())
            .filter(|&r| {
                counts_all[r].is_finite()
                    && size_column.is_none_or(|s| s[r].is_finite())
                    && (phase_col.is_none() || !phase_of(r).is_empty())
            })
            .collect();
        let rows: Vec<usize> = present.iter().copied().filter(|&r| !states.is_excluded(r)).collect();
        if rows.is_empty() {
            return Err(invalid("no subgroups to chart"));
        }
        let counts: Vec<f64> = rows.iter().map(|&r| counts_all[r]).collect();
        // Checked here, naming the column and row: inside a phase the error
        // read as if the phase were at fault.
        let whole_counts = request.kind != AttributeChartKind::T;
        if let Some(&r) = rows.iter().find(|&&r| whole_counts && (counts_all[r] < 0.0 || counts_all[r].fract() != 0.0))
        {
            return Err(invalid(format!(
                "{name} must hold whole, non-negative counts (row {} has {})",
                r + 1,
                format_number(counts_all[r], 6)
            )));
        }
        let sizes: Vec<f64> = match (request.kind.needs_size(), request.size) {
            (true, SubgroupSize::Column(_)) => rows.iter().map(|&r| size_column.map_or(f64::NAN, |s| s[r])).collect(),
            (true, SubgroupSize::Constant(n)) => vec![n; rows.len()],
            _ => vec![1.0; rows.len()],
        };
        let mut phases = Vec::new();
        let mut start = 0;
        while start < rows.len() {
            let label = phase_of(rows[start]);
            let end = (start..rows.len()).find(|&i| phase_of(rows[i]) != label).unwrap_or(rows.len());
            let chart = attribute_chart(
                request.kind,
                &counts[start..end],
                request.kind.needs_size().then_some(&sizes[start..end]),
            )
            .map_err(|e| match (e, phase_col.is_some()) {
                (StatError::InvalidArgument { message }, true) => invalid(format!("phase {label:?}: {message}")),
                (e, _) => e,
            })?;
            phases.push(Phase { name: label, start, end, chart });
            start = end;
        }
        Ok(Self {
            request: *request,
            name,
            n_excluded: present.len() - rows.len(),
            n_missing: table.n_rows() - present.len(),
            rows,
            counts,
            sizes,
            phases,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &AttributeChartRequest {
        &self.request
    }

    /// `true` if row exclusion has changed since the chart was computed.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title, e.g. `"p Chart of Defectives"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("{} Chart of {}", self.request.kind.short(), self.name)
    }

    /// The chart of each phase, in time order (one without a phase column).
    #[must_use]
    pub fn charts(&self) -> Vec<&AttributeChart> {
        self.phases.iter().map(|p| &p.chart).collect()
    }

    fn points(&self) -> impl Iterator<Item = (usize, &stat_quality::AttributePoint)> {
        self.phases.iter().flat_map(|p| p.chart.points.iter().enumerate().map(move |(i, q)| (p.start + i, q)))
    }

    /// Dataset rows of the subgroups outside their limits.
    #[must_use]
    pub fn signal_rows(&self) -> Vec<usize> {
        self.points().filter(|(_, p)| p.signal).map(|(i, _)| self.rows[i]).collect()
    }

    /// Run-rule violations under the request's rule set, on the scale where
    /// the rules apply ([`AttributeChart::rule_scale`]).
    #[must_use]
    pub fn violations(&self) -> Vec<RuleViolation> {
        let (mut values, mut limits) = (Vec::new(), Vec::new());
        for p in &self.phases {
            let (v, l) = p.chart.rule_scale();
            values.extend(v);
            limits.extend(l);
        }
        let spans: Vec<(usize, usize)> = self.phases.iter().map(|p| (p.start, p.end)).collect();
        varying_violations(&values, &limits, &self.request.rules.rules(), &spans)
    }

    /// Report sections: summary, chart, flagged subgroups.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let violations = self.violations();
        vec![self.summary(), self.plot(&violations), self.signals(&violations)]
    }

    fn phased(&self) -> bool {
        self.request.phase.is_some()
    }

    fn summary(&self) -> ReportSection {
        let phased = self.phased();
        let kind = self.request.kind;
        let mut t =
            TextTable::new(if phased { vec!["Phase", "Statistic", "Value"] } else { vec!["Statistic", "Value"] });
        let mut push = |phase: &str, label: &str, value: String| {
            let mut row = Vec::with_capacity(3);
            if phased {
                row.push(phase.to_owned());
            }
            row.extend([label.to_owned(), value]);
            t.push(row);
        };
        push("", "Chart", kind.label().to_owned());
        push("", "Subgroups", self.rows.len().to_string());
        if self.n_excluded > 0 {
            push("", "Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("", "Rows without a value", self.n_missing.to_string());
        }
        for p in &self.phases {
            let c = &p.chart;
            let name = p.name.as_str();
            if phased {
                push(name, "Subgroups", (p.end - p.start).to_string());
            }
            let total: f64 = self.counts[p.start..p.end].iter().sum();
            let total_label = match kind {
                AttributeChartKind::G => "Total units between events",
                AttributeChartKind::T => "Total time",
                _ => "Total count",
            };
            push(name, total_label, format_number(total, DIGITS));
            if kind.needs_size() {
                push(name, "Total units inspected", format_number(self.sizes[p.start..p.end].iter().sum(), DIGITS));
            }
            if let Some(s) = c.sigma_z {
                push(name, "Laney σ_z (spread of standardized points; 1 = binomial/Poisson)", format_number(s, DIGITS));
            }
            if let Some((m, s)) = c.transformed {
                push(name, "Mean of transformed times t^(1/3.6)", format_number(m, DIGITS));
                push(name, "σ of transformed times (moving range)", format_number(s, DIGITS));
            }
            push(name, "Center line", format_number(c.center, DIGITS));
            let (lo, hi) = (c.points[0].lower, c.points[0].upper);
            if c.points.iter().all(|q| q.lower == lo && q.upper == hi) {
                push(name, "Lower control limit", format_number(lo, DIGITS));
                push(name, "Upper control limit", format_number(hi, DIGITS));
            } else {
                push(name, "Control limits", "vary with subgroup size (see the chart)".to_owned());
            }
        }
        push("", "Subgroups outside the limits", self.signal_rows().len().to_string());
        section(SectionKind::AttributeSummary, "Chart Summary", SectionContent::Table(t))
    }

    fn plot(&self, violations: &[RuleViolation]) -> ReportSection {
        let kind = self.request.kind;
        let points: Vec<ChartPoint> = self
            .points()
            .map(|(i, p)| {
                let mut rules: Vec<u8> = violations.iter().filter(|v| v.index == i).map(|v| v.rule.number()).collect();
                rules.dedup();
                ChartPoint {
                    sample: i + 1,
                    value: p.statistic,
                    rows: vec![self.rows[i]],
                    rules,
                    label: None,
                    note: None,
                    signal: false,
                }
            })
            .collect();
        // Limits as steps, so varying limits show per subgroup.
        let mut steps: Vec<LimitStep> = Vec::new();
        for (i, p) in self.points() {
            let center =
                self.phases.iter().find(|q| (q.start..q.end).contains(&i)).map_or(f64::NAN, |q| q.chart.center);
            let x = i as f64 + 1.0;
            // σ of the plotted statistic, not (UCL − CL)/3, so a p chart
            // whose upper limit is capped at one keeps its true zones.
            let zone_sigma = if kind == AttributeChartKind::T { f64::NAN } else { p.sigma };
            match steps.last_mut() {
                Some(s) if s.center == center && s.lower == p.lower && s.upper == p.upper => s.to = x + 0.5,
                _ => steps.push(LimitStep {
                    from: x - 0.5,
                    to: x + 0.5,
                    center,
                    lower: p.lower,
                    upper: p.upper,
                    zone_sigma,
                }),
            }
        }
        let first = steps[0];
        let phases = if self.phased() {
            self.phases.iter().map(|p| PhaseMark { at: p.start as f64 + 0.5, label: p.name.clone() }).collect()
        } else {
            Vec::new()
        };
        let spec = ControlChartSpec {
            x_label: "Subgroup".to_owned(),
            x_ticks: None,
            flag_label: None,
            limit_label: None,
            y_label: kind.statistic_label().to_owned(),
            points,
            center: first.center,
            lower: first.lower,
            upper: first.upper,
            zone_sigma: first.zone_sigma,
            steps: if steps.len() > 1 { steps } else { Vec::new() },
            phases,
        };
        section(SectionKind::AttributeChart, kind.label(), SectionContent::ControlChart(spec))
    }

    fn signals(&self, violations: &[RuleViolation]) -> ReportSection {
        let kind = self.request.kind;
        let only_limits = self.request.rules == RuleSet::LIMITS_ONLY;
        let title = if only_limits { "Subgroups Outside the Limits" } else { "Flagged Subgroups" };
        let phased = self.phased();
        let mut header = vec!["Subgroup", "Row"];
        if phased {
            header.push("Phase");
        }
        header.extend(["Count", "Size", "Statistic", "Rules", "Pattern"]);
        let mut t = TextTable::new(header);
        let mut indices: Vec<usize> = violations.iter().map(|v| v.index).collect();
        indices.dedup();
        let points: Vec<(usize, &stat_quality::AttributePoint)> = self.points().collect();
        for i in indices {
            let p = points[i].1;
            let same: Vec<&RuleViolation> = violations.iter().filter(|v| v.index == i).collect();
            let pattern: Vec<String> = same
                .iter()
                .map(|v| match v.rule.number() {
                    1 if p.statistic > p.upper => "Above the upper limit".to_owned(),
                    1 if kind == AttributeChartKind::T => "Below the lower limit (events closer together)".to_owned(),
                    1 => "Below the lower limit".to_owned(),
                    _ => v.rule.description().to_owned(),
                })
                .collect();
            let mut row = vec![(i + 1).to_string(), (self.rows[i] + 1).to_string()];
            if phased {
                row.push(
                    self.phases
                        .iter()
                        .find(|q| (q.start..q.end).contains(&i))
                        .map(|q| q.name.clone())
                        .unwrap_or_default(),
                );
            }
            row.extend([
                format_number(self.counts[i], DIGITS),
                if kind.needs_size() { format_number(self.sizes[i], DIGITS) } else { "—".to_owned() },
                format_number(p.statistic, DIGITS),
                same.iter().map(|v| v.rule.number().to_string()).collect::<Vec<_>>().join(", "),
                pattern.join("; "),
            ]);
            t.push(row);
        }
        if t.rows.is_empty() {
            let note = if only_limits {
                "None: every subgroup lies within its control limits."
            } else {
                "None: no subgroup violates the run rules applied."
            };
            return section(SectionKind::AttributeSignals, title, SectionContent::Note(note.to_owned()));
        }
        section(SectionKind::AttributeSignals, title, SectionContent::Table(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let d = [
            12.0, 15.0, 8.0, 10.0, 4.0, 7.0, 16.0, 9.0, 14.0, 10.0, 5.0, 6.0, 17.0, 12.0, 22.0, 8.0, 10.0, 5.0, 13.0,
            11.0, 20.0, 18.0, 24.0, 15.0, 9.0, 12.0, 7.0, 13.0, 9.0, 6.0,
        ];
        DataTable::new(
            "cans",
            vec![Column::numeric("Defective", d.to_vec()), Column::numeric("Inspected", vec![50.0; 30])],
        )
        .unwrap()
    }

    #[test]
    fn p_chart_report() {
        let t = table();
        let rows = RowStateManager::new(30);
        let request = AttributeChartRequest::new(AttributeChartKind::P, 0, SubgroupSize::Column(1));
        let a = AttributeChartAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.title(), "p Chart of Defective");
        assert_eq!(a.signal_rows(), [14, 22]);
        let s = a.sections();
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert_eq!(summary.rows.iter().find(|r| r[0] == "Upper control limit").unwrap()[1], "0.41024");
        let SectionContent::ControlChart(p) = &s[1].content else { panic!() };
        assert_eq!(p.points.len(), 30);
        assert!(p.steps.is_empty(), "equal sizes: one set of limits");
        assert_eq!(p.points.iter().filter(|q| q.flagged()).count(), 2);
        assert_eq!(p.points[14].rows, vec![14]);
        let SectionContent::Table(sig) = &s[2].content else { panic!() };
        assert_eq!(sig.rows.len(), 2);
        assert_eq!(sig.cell(0, "Pattern"), Some("Above the upper limit"));
        // The same chart with a constant size; then excluding the two signals.
        let constant = AttributeChartRequest { size: SubgroupSize::Constant(50.0), ..request };
        let b = AttributeChartAnalysis::run(&t, &rows, &constant).unwrap();
        assert_eq!(b.signal_rows(), [14, 22]);
        let mut ex = RowStateManager::new(30);
        ex.set_excluded([14, 22], true);
        let c = AttributeChartAnalysis::run(&t, &ex, &request).unwrap();
        assert!(c.is_stale(&rows));
        assert!(matches!(c.sections()[2].content, SectionContent::Note(_) | SectionContent::Table(_)));
    }

    #[test]
    fn c_chart_and_errors() {
        let t = table();
        let rows = RowStateManager::new(30);
        let c = AttributeChartRequest::new(AttributeChartKind::C, 0, SubgroupSize::None);
        assert!(AttributeChartAnalysis::run(&t, &rows, &c).is_ok());
        let u_no_size = AttributeChartRequest { kind: AttributeChartKind::U, ..c };
        assert!(AttributeChartAnalysis::run(&t, &rows, &u_no_size).is_err());
        let same = AttributeChartRequest::new(AttributeChartKind::P, 0, SubgroupSize::Column(0));
        assert!(AttributeChartAnalysis::run(&t, &rows, &same).is_err());
        // Fractional counts name the column and row, not a phase.
        let mut d = t.column(0).unwrap().as_numeric().unwrap().to_vec();
        d[2] = 8.5;
        let frac = DataTable::new("f", vec![Column::numeric("Defective", d)]).unwrap();
        let e = AttributeChartAnalysis::run(&frac, &rows, &c).unwrap_err().to_string();
        assert!(e.starts_with("Defective must hold whole") && e.contains("row 3 has 8.5"), "{e}");
    }

    #[test]
    fn phases_rules_and_new_kinds() {
        // Two phases of 15 subgroups: the second has a lower defect rate.
        let t = table();
        let mut cols = t.columns().to_vec();
        let phase: Vec<String> = (0..30).map(|i| if i < 15 { "A" } else { "B" }.to_owned()).collect();
        cols.push(Column::text("Phase", phase));
        let t = DataTable::new("cans", cols).unwrap();
        let rows = RowStateManager::new(30);
        let request = AttributeChartRequest {
            phase: Some(2),
            rules: RuleSet::ALL,
            ..AttributeChartRequest::new(AttributeChartKind::LaneyP, 0, SubgroupSize::Column(1))
        };
        let a = AttributeChartAnalysis::run(&t, &rows, &request).unwrap();
        assert_eq!(a.charts().len(), 2);
        let s = a.sections();
        let SectionContent::ControlChart(spec) = &s[1].content else { panic!() };
        assert_eq!(spec.phases.len(), 2);
        assert_eq!(spec.steps.len(), 2, "each phase has its own limits");
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        assert_eq!(summary.columns[0], "Phase");
        assert_eq!(summary.rows.iter().filter(|r| r[1].starts_with("Laney σ_z")).count(), 2);
        assert_eq!(request.columns(), [0, 1, 2]);
        // g and t charts read one column.
        let g = AttributeChartRequest::new(AttributeChartKind::G, 0, SubgroupSize::Column(1));
        let ga = AttributeChartAnalysis::run(&t, &rows, &g).unwrap();
        assert_eq!(ga.title(), "g Chart of Defective");
        let tt = AttributeChartRequest::new(AttributeChartKind::T, 0, SubgroupSize::None);
        let ta = AttributeChartAnalysis::run(&t, &rows, &tt).unwrap();
        let SectionContent::ControlChart(spec) = &ta.sections()[1].content else { panic!() };
        assert!(spec.zone_sigma.is_nan(), "no symmetric zones on the time scale");
    }
}
