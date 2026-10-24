//! A control-chart analysis bound to a dataset and its row state.
//!
//! The chart is built from **column roles**: the measurement column, and
//! optionally a subgroup ID column, a phase column and a label column. Rows
//! are taken in table order, skipping excluded rows and rows with a missing
//! measurement, phase or subgroup ID (so excluding an out-of-control point
//! recomputes the limits without it, the usual Phase I practice).
//!
//! - **Phases** are runs of consecutive rows with the same phase value; each
//!   gets its own center line and limits, and run rules restart at each one
//!   (see [`stat_quality::phased`]).
//! - **Subgroups**, within a phase, are the rows sharing a subgroup ID (in
//!   order of first appearance), so subgroup sizes may vary; without an ID
//!   column they are consecutive runs of `subgroup_size` rows.
//! - **Labels** name each plotted point: the label column's value on the
//!   subgroup's first row, else the subgroup ID (else, on a short-run chart,
//!   the part number).
//! - **Part numbers** (short-run charts) are the values of the part column;
//!   each part gets its own mean and σ within each phase.
//!
//! Every plotted point records the dataset rows it summarizes, which is what
//! links chart brushing to the data grid: a moving range carries both
//! points' rows and a moving average every point it averages.
//!
//! Besides the X̄/R, X̄/S and individuals charts the analysis draws the
//! moving-average, three-way, zone and short-run charts of
//! [`stat_quality::extended`].
//!
//! **Options** (individuals, X̄–R and X̄–S charts): limits at `k`σ, moving
//! ranges over `w` values, and limits from standard (historical) values
//! typed in or read from a saved limits table ([`standard_from_table`]); see
//! [`stat_quality::phased`]. Any chart can show only its **last N
//! subgroups**, with limits from all subgroups (the earlier ones are only
//! hidden) or from those N alone. A **run chart** section plots every value
//! at its subgroup's position, joins the subgroup means or medians, and
//! draws the center line at the median (or mean) of all values.
//! [`saved_limits_table`] writes the limits, process center and σ of every
//! phase as a data table, which can later give the standard values.

use crate::format::format_number;
use crate::report::{
    ChartPoint, ControlChartSpec, LimitStep, PhaseMark, ReportSection, SectionContent, SectionKind, TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_quality::{
    ChartKind, ChartSettings, ChartStatistic, ControlLimits, PhasedChart, RuleViolation, RunRule, StandardValues,
    SubgroupUse, VaryingChart, ZONE_SIGNAL, phased_chart_with,
};

const DIGITS: usize = 6;

/// Default span of a moving-average chart.
pub const DEFAULT_SPAN: usize = 5;

/// Show only the last subgroups of a chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastSubgroups {
    /// Subgroups (points) shown.
    pub count: usize,
    /// Set the limits from those subgroups only (`false`: from all, the
    /// earlier ones hidden).
    pub limits_from_last: bool,
}

/// Which chart to draw, from which columns.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlChartRequest {
    /// Measurement column index.
    pub column: usize,
    /// Chart type.
    pub kind: ChartKind,
    /// Observations per subgroup when there is no subgroup ID column (ignored
    /// for individuals and short-run charts; 1 plots individual values on a
    /// moving-average or zone chart).
    pub subgroup_size: usize,
    /// Subgroup ID column: rows sharing a value form one subgroup.
    pub subgroup: Option<usize>,
    /// Phase column: each run of equal values gets its own limits.
    pub phase: Option<usize>,
    /// Label column naming each point.
    pub label: Option<usize>,
    /// Span `w` of a moving-average chart.
    pub span: usize,
    /// Part-number column of a short-run chart.
    pub part: Option<usize>,
    /// Limits at this many σ (3 by default; individuals, X̄–R and X̄–S).
    pub sigma_multiple: f64,
    /// Values per moving range of an individuals chart (2 by default).
    pub range_span: usize,
    /// Standard (historical) center and σ to set the limits from.
    pub standard: Option<StandardValues>,
    /// Show only the last subgroups.
    pub last: Option<LastSubgroups>,
}

impl ControlChartRequest {
    /// A request with no subgroup ID, phase, label or part column.
    #[must_use]
    pub fn new(column: usize, kind: ChartKind, subgroup_size: usize) -> Self {
        Self {
            column,
            kind,
            subgroup_size,
            subgroup: None,
            phase: None,
            label: None,
            span: DEFAULT_SPAN,
            part: None,
            sigma_multiple: 3.0,
            range_span: 2,
            standard: None,
            last: None,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.column];
        c.extend(self.subgroup);
        c.extend(self.phase);
        c.extend(self.label);
        c.extend(self.part);
        c
    }

    /// `true` when the chart plots individual values (one row per point).
    #[must_use]
    pub fn plots_individuals(&self) -> bool {
        match self.kind.subgroup_use() {
            SubgroupUse::Individuals => true,
            SubgroupUse::Subgroups => false,
            SubgroupUse::Either => self.subgroup.is_none() && self.subgroup_size < 2,
        }
    }
}

/// A set of the eight run rules, as a bit mask (bit `k − 1` for rule `k`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleSet(pub u8);

impl RuleSet {
    /// All eight rules.
    pub const ALL: Self = Self(0xff);
    /// Rule 1 only (a point beyond the limits).
    pub const LIMITS_ONLY: Self = Self(1);

    /// `true` if `rule` is in the set.
    #[must_use]
    pub fn contains(self, rule: RunRule) -> bool {
        self.0 & Self::bit(rule) != 0
    }

    /// Add or remove `rule`.
    pub fn set(&mut self, rule: RunRule, on: bool) {
        if on {
            self.0 |= Self::bit(rule);
        } else {
            self.0 &= !Self::bit(rule);
        }
    }

    /// The rules in the set, in rule order.
    #[must_use]
    pub fn rules(self) -> Vec<RunRule> {
        RunRule::ALL.into_iter().filter(|r| self.contains(*r)).collect()
    }

    /// The rule numbers in the set (for saving).
    #[must_use]
    pub fn numbers(self) -> Vec<u8> {
        self.rules().into_iter().map(RunRule::number).collect()
    }

    /// The set of the given rule numbers; numbers outside 1–8 are ignored.
    #[must_use]
    pub fn from_numbers(numbers: impl IntoIterator<Item = u8>) -> Self {
        Self(numbers.into_iter().filter(|n| (1..=8).contains(n)).fold(0, |m, n| m | 1 << (n - 1)))
    }

    fn bit(rule: RunRule) -> u8 {
        1 << (rule.number() - 1)
    }
}

/// User-toggleable parts of a control-chart report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlChartOptions {
    /// Show the dispersion (R, S or MR) chart.
    pub show_dispersion_chart: bool,
    /// Draw the 1σ and 2σ zone boundaries.
    pub show_zones: bool,
    /// Run rules applied to the location chart (all eight by default).
    pub location_rules: RuleSet,
    /// Run rules applied to the dispersion chart (rule 1 by default).
    pub dispersion_rules: RuleSet,
    /// Show the table of flagged points.
    pub show_violation_table: bool,
    /// Run rules applied to a three-way chart's within-subgroup chart (rule 1
    /// by default).
    pub within_rules: RuleSet,
    /// Show the run chart of every value.
    pub show_run_chart: bool,
    /// What the run chart joins and centers on.
    pub run_center: RunCenter,
}

/// The subgroup summary a run chart joins, and its center line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunCenter {
    /// Subgroup medians, center at the median of all values.
    #[default]
    Median,
    /// Subgroup means, center at the mean of all values.
    Mean,
}

impl RunCenter {
    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Median => "median",
            Self::Mean => "mean",
        }
    }

    /// The center named `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Median, Self::Mean].into_iter().find(|c| c.key() == key)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Median => "Median",
            Self::Mean => "Mean",
        }
    }
}

impl Default for ControlChartOptions {
    fn default() -> Self {
        Self {
            show_dispersion_chart: true,
            show_zones: true,
            location_rules: RuleSet::ALL,
            dispersion_rules: RuleSet::LIMITS_ONLY,
            show_violation_table: true,
            within_rules: RuleSet::LIMITS_ONLY,
            show_run_chart: false,
            run_center: RunCenter::Median,
        }
    }
}

/// Which of a report's charts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Location,
    Dispersion,
    Within,
}

/// A computed chart pair plus the row mapping needed to report and brush it.
#[derive(Debug, Clone)]
pub struct ControlChartAnalysis {
    request: ControlChartRequest,
    column_name: String,
    chart: PhasedChart,
    /// Dataset rows behind each point (subgroup).
    point_rows: Vec<Vec<usize>>,
    /// Measurements of each point's rows.
    point_values: Vec<Vec<f64>>,
    /// Subgroups computed and the number hidden before the shown ones.
    n_points: usize,
    n_hidden: usize,
    /// Label of each point.
    labels: Vec<Option<String>>,
    /// Phase names, one per phase (empty without a phase column).
    phase_names: Vec<String>,
    /// Part numbers of a short-run chart, indexed as in the chart's parts.
    part_names: Vec<String>,
    /// Name of the part column.
    part_column: Option<String>,
    /// Name of the phase column.
    phase_column: Option<String>,
    n_excluded: usize,
    n_missing: usize,
    leftover: usize,
    /// Subgroups left out for having too few values.
    n_short: usize,
    exclusion_revision: u64,
}

/// Rows of one phase, before subgrouping.
struct PhaseRows {
    name: String,
    rows: Vec<usize>,
}

impl ControlChartAnalysis {
    /// Compute the chart for `request`, honouring row exclusion.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a missing or non-numeric
    /// measurement column, a missing role column, a bad subgroup size or too
    /// little data; errors from the chart computation.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ControlChartRequest) -> Result<Self> {
        let col = table.column(request.column).ok_or_else(|| invalid(format!("no column {}", request.column)))?;
        let values = col.as_numeric().ok_or_else(|| invalid(format!("column {:?} is not numeric", col.name)))?;
        let role = |c: Option<usize>| -> Result<Option<&stat_data::Column>> {
            c.map(|i| table.column(i).ok_or_else(|| invalid(format!("no column {i}")))).transpose()
        };
        let (subgroup_col, phase_col, label_col) =
            (role(request.subgroup)?, role(request.phase)?, role(request.label)?);
        let short_run = request.kind == ChartKind::ShortRun;
        let part_col = if short_run {
            Some(role(request.part)?.ok_or_else(|| invalid("a short-run chart needs a part-number column"))?)
        } else {
            None
        };
        let individuals = request.plots_individuals();
        let subgroup_col = if individuals { None } else { subgroup_col };

        // Usable rows, split into phases.
        let mut phases: Vec<PhaseRows> = Vec::new();
        let (mut n_excluded, mut n_missing) = (0, 0);
        for (r, v) in values.iter().enumerate() {
            let phase = phase_col.map(|c| c.level_text(r)).unwrap_or_default();
            let id_missing = subgroup_col.is_some_and(|c| c.level_text(r).is_empty())
                || part_col.is_some_and(|c| c.level_text(r).is_empty());
            if !v.is_finite() || id_missing || (phase_col.is_some() && phase.is_empty()) {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                match phases.last_mut() {
                    Some(p) if p.name == phase => p.rows.push(r),
                    _ => phases.push(PhaseRows { name: phase, rows: vec![r] }),
                }
            }
        }

        // Subgroups within each phase.
        let min_size = if individuals { 1 } else { 2 };
        let (mut groups, mut point_rows, mut labels, mut phase_of) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut leftover, mut n_short) = (0, 0);
        for (p, phase) in phases.iter().enumerate() {
            let rows_of_phase: Vec<Vec<usize>> = if individuals {
                phase.rows.iter().map(|&r| vec![r]).collect()
            } else if let Some(c) = subgroup_col {
                let mut ids: Vec<String> = Vec::new();
                let mut members: Vec<Vec<usize>> = Vec::new();
                for &r in &phase.rows {
                    let id = c.level_text(r);
                    match ids.iter().position(|x| *x == id) {
                        Some(k) => members[k].push(r),
                        None => {
                            ids.push(id);
                            members.push(vec![r]);
                        }
                    }
                }
                members
            } else {
                let size = request.subgroup_size;
                if size < 2 {
                    return Err(invalid("subgroups need at least 2 observations; use an individuals chart for 1"));
                }
                leftover += phase.rows.len() % size;
                phase.rows.chunks_exact(size).map(<[usize]>::to_vec).collect()
            };
            for rows in rows_of_phase {
                if rows.len() < min_size {
                    n_short += 1;
                    continue;
                }
                let first = rows[0];
                let label = label_col
                    .map(|c| c.level_text(first))
                    .or_else(|| subgroup_col.map(|c| c.level_text(first)))
                    .or_else(|| part_col.map(|c| c.level_text(first)))
                    .filter(|s| !s.is_empty());
                groups.push(rows.iter().map(|&r| values[r]).collect::<Vec<f64>>());
                point_rows.push(rows);
                labels.push(label);
                phase_of.push(p);
            }
        }
        if groups.is_empty() {
            return Err(invalid("no complete subgroups to chart"));
        }
        let n_points = groups.len();
        let cut = match request.last {
            Some(l) if l.count < 2 => return Err(invalid("show at least the last 2 subgroups")),
            Some(l) => n_points.saturating_sub(l.count),
            None => 0,
        };
        if request.last.is_some_and(|l| l.limits_from_last) && cut > 0 {
            groups.drain(..cut);
            point_rows.drain(..cut);
            labels.drain(..cut);
            phase_of.drain(..cut);
        }
        // Part numbers, indexed in order of first appearance.
        let mut part_names: Vec<String> = Vec::new();
        let parts = part_col.map(|c| {
            point_rows
                .iter()
                .map(|rows| {
                    let name = c.level_text(rows[0]);
                    part_names.iter().position(|n| *n == name).unwrap_or_else(|| {
                        part_names.push(name);
                        part_names.len() - 1
                    })
                })
                .collect::<Vec<usize>>()
        });
        let settings = ChartSettings {
            span: request.span,
            parts,
            sigma_multiple: request.sigma_multiple,
            range_span: request.range_span,
            standard: request.standard,
        };
        let mut chart = phased_chart_with(request.kind, &groups, &phase_of, &settings).map_err(|e| match e {
            StatError::InvalidArgument { message } => {
                let mut message = if phase_col.is_some() && message.contains("only one point") {
                    format!("{message} (each phase needs at least 2 points)")
                } else {
                    message
                };
                // Name parts by their values rather than their index.
                for (k, name) in part_names.iter().enumerate().rev() {
                    message = message.replace(&format!("part {} ", k + 1), &format!("part {name:?} "));
                }
                if phase_col.is_none() {
                    message = message.replace(" in phase 1", "");
                }
                invalid(message)
            }
            e => e,
        })?;
        // Hide the subgroups before the last N (limits from all of them).
        let hide = if request.last.is_some_and(|l| l.limits_from_last) { 0 } else { cut };
        if hide > 0 {
            keep_last(&mut chart, hide);
            groups.drain(..hide);
            point_rows.drain(..hide);
            labels.drain(..hide);
            phase_of.drain(..hide);
        }
        // Name each phase that kept at least one point.
        let phase_names = if phase_col.is_some() {
            let mut used = phase_of;
            used.dedup();
            used.into_iter().map(|p| phases[p].name.clone()).collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            request: request.clone(),
            column_name: col.name.clone(),
            chart,
            point_rows,
            point_values: groups,
            n_points,
            n_hidden: cut,
            labels,
            phase_names,
            part_names,
            part_column: part_col.map(|c| c.name.clone()),
            phase_column: phase_col.map(|c| c.name.clone()),
            n_excluded,
            n_missing,
            leftover,
            n_short,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &ControlChartRequest {
        &self.request
    }

    /// The computed charts.
    #[must_use]
    pub fn charts(&self) -> &PhasedChart {
        &self.chart
    }

    /// Phase names in time order (empty without a phase column).
    #[must_use]
    pub fn phase_names(&self) -> &[String] {
        &self.phase_names
    }

    /// `true` if row exclusion changed since the chart was computed.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Part numbers of a short-run chart, in order of first appearance.
    #[must_use]
    pub fn part_names(&self) -> &[String] {
        &self.part_names
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        let kind = match self.chart.kind {
            ChartKind::MovingAverage => format!("Moving Average (span {})", self.request.span),
            k => k.label().to_owned(),
        };
        match &self.phase_column {
            Some(phase) => format!("Control Chart: {kind} — {} by phase of {phase}", self.column_name),
            None => format!("Control Chart: {kind} — {}", self.column_name),
        }
    }

    /// The location-chart rules that apply under `options`: only rule 1 on a
    /// moving-average or zone chart (see [`ChartKind::pattern_rules_apply`]).
    #[must_use]
    pub fn effective_location_rules(&self, options: &ControlChartOptions) -> RuleSet {
        if self.chart.kind.pattern_rules_apply() {
            options.location_rules
        } else {
            RuleSet(options.location_rules.0 & RuleSet::LIMITS_ONLY.0)
        }
    }

    /// Run-rule violations on the location chart, under `options`.
    #[must_use]
    pub fn location_violations(&self, options: &ControlChartOptions) -> Vec<RuleViolation> {
        self.chart.location.violations(&self.effective_location_rules(options).rules(), &self.chart.phases)
    }

    /// Run-rule violations on the dispersion chart, under `options`.
    #[must_use]
    pub fn dispersion_violations(&self, options: &ControlChartOptions) -> Vec<RuleViolation> {
        self.chart.dispersion.violations(&options.dispersion_rules.rules(), &self.chart.phases)
    }

    /// Run-rule violations on a three-way chart's within-subgroup chart.
    #[must_use]
    pub fn within_violations(&self, options: &ControlChartOptions) -> Vec<RuleViolation> {
        self.chart
            .within
            .as_ref()
            .map_or_else(Vec::new, |w| w.violations(&options.within_rules.rules(), &self.chart.phases))
    }

    /// Indices of the location points where a zone chart signals.
    #[must_use]
    pub fn zone_signals(&self) -> Vec<usize> {
        self.chart.zones.iter().enumerate().filter(|(_, z)| z.signal).map(|(i, _)| i).collect()
    }

    /// Report sections, honouring `options`.
    #[must_use]
    pub fn sections(&self, options: &ControlChartOptions) -> Vec<ReportSection> {
        let loc_v = self.location_violations(options);
        let disp_v = if options.show_dispersion_chart { self.dispersion_violations(options) } else { Vec::new() };
        let within_v = if options.show_dispersion_chart { self.within_violations(options) } else { Vec::new() };
        let mut out = vec![self.limits_table()];
        let loc_title = match self.chart.kind {
            ChartKind::Zone => format!("Zone Chart of {}", self.chart.location.statistic.label()),
            ChartKind::ThreeWayRange | ChartKind::ThreeWayStdDev => "Subgroup Means Chart (between)".to_owned(),
            _ => format!("{} Chart", self.chart.location.statistic.label()),
        };
        out.push(section(
            SectionKind::LocationChart,
            &loc_title,
            SectionContent::ControlChart(self.chart_spec(&self.chart.location, &loc_v, Role::Location)),
        ));
        if options.show_dispersion_chart {
            let title = format!("{} Chart", self.chart.dispersion.statistic.label());
            out.push(section(
                SectionKind::DispersionChart,
                &title,
                SectionContent::ControlChart(self.chart_spec(&self.chart.dispersion, &disp_v, Role::Dispersion)),
            ));
            if let Some(w) = &self.chart.within {
                let title = format!("Within-Subgroup {} Chart", w.statistic.label());
                out.push(section(
                    SectionKind::WithinChart,
                    &title,
                    SectionContent::ControlChart(self.chart_spec(w, &within_v, Role::Within)),
                ));
            }
        }
        if options.show_run_chart {
            out.push(self.run_chart(options.run_center));
        }
        if !self.chart.parts.is_empty() {
            out.push(self.part_table());
        }
        if options.show_violation_table {
            out.push(self.violation_table(&loc_v, &disp_v, &within_v));
        }
        out
    }

    /// Every value at its subgroup's position, the subgroup means or
    /// medians joined, and a center line at the median or mean of all values.
    fn run_chart(&self, center: RunCenter) -> ReportSection {
        let x = |i: usize| (i + 1) as f64;
        let points: Vec<crate::report::RowPoint> = self
            .point_rows
            .iter()
            .zip(&self.point_values)
            .enumerate()
            .flat_map(|(i, (rows, values))| {
                rows.iter().zip(values).map(move |(&row, &y)| crate::report::RowPoint { row, x: x(i), y })
            })
            .collect();
        let summary = |v: &[f64]| match center {
            RunCenter::Mean => v.iter().sum::<f64>() / v.len() as f64,
            RunCenter::Median => median(v),
        };
        let all: Vec<f64> = self.point_values.iter().flatten().copied().collect();
        let middle = summary(&all);
        let single = self.point_values.iter().all(|g| g.len() == 1);
        let line = Some(self.point_values.iter().enumerate().map(|(i, g)| [x(i), summary(g)]).collect());
        let (lo, hi) = (0.5, self.point_values.len() as f64 + 0.5);
        let label = format!("{} {}", center.label(), format_number(middle, DIGITS));
        let line_name =
            if single { "Values in order".to_owned() } else { format!("Subgroup {}s", center.label().to_lowercase()) };
        section(
            SectionKind::RunChart,
            &format!("Run Chart of {}", self.column_name),
            SectionContent::Plot(Box::new(crate::report::PlotSpec {
                x_label: if single { "Observation" } else { "Subgroup" }.to_owned(),
                y_label: self.column_name.clone(),
                points,
                line,
                line_name: Some(line_name),
                segments: vec![vec![[lo, middle], [hi, middle]]],
                annotations: Some(crate::report::Annotations {
                    groups: Vec::new(),
                    items: Vec::new(),
                    labels: vec![(hi, middle, label)],
                }),
                ..crate::report::PlotSpec::default()
            })),
        )
    }

    /// The limits as a data table: one row per phase and chart with the
    /// center, limits, process center and σ (see [`standard_from_table`]).
    #[must_use]
    pub fn limits_data_table(&self) -> DataTable {
        let (mut measurement, mut phase, mut chart) = (Vec::new(), Vec::new(), Vec::new());
        let (mut center, mut lower, mut upper) = (Vec::new(), Vec::new(), Vec::new());
        let (mut process_center, mut process_sigma, mut size, mut multiple) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (k, p) in self.chart.phases.iter().enumerate() {
            for c in [&self.chart.location, &self.chart.dispersion].into_iter().chain(self.chart.within.as_ref()) {
                let Some(l) = c.limits.get(p.start) else { continue };
                measurement.push(self.column_name.clone());
                phase.push(self.phase_names.get(k).cloned().unwrap_or_default());
                chart.push(c.statistic.label().to_owned());
                center.push(l.center);
                lower.push(l.lower);
                upper.push(l.upper);
                process_center.push(p.center);
                process_sigma.push(p.sigma);
                size.push(if p.sizes.0 == p.sizes.1 { p.sizes.0 as f64 } else { f64::NAN });
                multiple.push(c.sigma_multiple);
            }
        }
        let mut columns = vec![stat_data::Column::text("Measurement", measurement)];
        if self.phased() {
            columns.push(stat_data::Column::text("Phase", phase));
        }
        columns.extend([
            stat_data::Column::text("Chart", chart),
            stat_data::Column::numeric("Center", center),
            stat_data::Column::numeric("Lower limit", lower),
            stat_data::Column::numeric("Upper limit", upper),
            stat_data::Column::numeric(PROCESS_CENTER, process_center),
            stat_data::Column::numeric(PROCESS_SIGMA, process_sigma),
            stat_data::Column::numeric("Subgroup size", size),
            stat_data::Column::numeric("Sigma multiple", multiple),
        ]);
        DataTable::new(format!("Control limits of {}", self.column_name), columns).expect("columns of equal length")
    }

    fn part_table(&self) -> ReportSection {
        let phased = self.phased();
        let part = self.part_column.clone().unwrap_or_else(|| "Part".to_owned());
        let mut header = vec![part, "N".to_owned(), "Mean".to_owned(), "σ (moving range)".to_owned()];
        if phased {
            header.insert(0, "Phase".to_owned());
        }
        let mut t = TextTable::new(header);
        for p in &self.chart.parts {
            let mut row = Vec::with_capacity(5);
            if phased {
                row.push(self.phase_names.get(p.phase).cloned().unwrap_or_default());
            }
            row.extend([
                self.part_names.get(p.part).cloned().unwrap_or_default(),
                p.n.to_string(),
                format_number(p.mean, DIGITS),
                format_number(p.sigma, DIGITS),
            ]);
            t.push(row);
        }
        section(SectionKind::PartEstimates, "Part Estimates", SectionContent::Table(t))
    }

    fn phased(&self) -> bool {
        !self.phase_names.is_empty()
    }

    fn limits_table(&self) -> ReportSection {
        let phased = self.phased();
        let mut header = vec!["Chart", "Center", "Lower limit", "Upper limit"];
        if phased {
            header.insert(0, "Phase");
        }
        let mut t = TextTable::new(header);
        let mut push = |phase: &str, cells: [String; 4]| {
            let mut row: Vec<String> = Vec::with_capacity(5);
            if phased {
                row.push(phase.to_owned());
            }
            row.extend(cells);
            t.push(row);
        };
        let varies_in_size = self.chart.phases.iter().any(|p| p.sizes.0 != p.sizes.1);
        for (k, p) in self.chart.phases.iter().enumerate() {
            let name = self.phase_names.get(k).map_or("", String::as_str);
            for c in [&self.chart.location, &self.chart.dispersion].into_iter().chain(self.chart.within.as_ref()) {
                let span = &c.limits[p.start..p.end];
                let range = |f: fn(&ControlLimits) -> f64| {
                    let (lo, hi) =
                        span.iter().map(f).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
                    if lo == hi {
                        format_number(lo, DIGITS)
                    } else {
                        format!("{} to {}", format_number(lo, DIGITS), format_number(hi, DIGITS))
                    }
                };
                push(
                    name,
                    [c.statistic.label().to_owned(), range(|l| l.center), range(|l| l.lower), range(|l| l.upper)],
                );
            }
            if self.chart.kind == ChartKind::ShortRun {
                let parts = self.chart.parts.iter().filter(|q| q.phase == k).count();
                push(name, info("Part numbers (each standardized by its own mean and σ)", parts.to_string()));
            } else if self.request.standard.is_some() {
                push(name, info("Process center (standard value)", format_number(p.center, DIGITS)));
                push(name, info("Process σ (standard value)", format_number(p.sigma, DIGITS)));
            } else {
                push(name, info("Process σ (within)", format_number(p.sigma, DIGITS)));
            }
            if let Some(b) = p.between_sigma {
                push(name, info("σ of subgroup means (from their moving ranges)", format_number(b, DIGITS)));
            }
            if self.chart.kind == ChartKind::MovingAverage {
                push(name, info("Moving-average span", self.request.span.to_string()));
            }
            if self.chart.kind == ChartKind::Zone {
                let signals = self.chart.zones[p.start..p.end].iter().filter(|z| z.signal).count();
                push(name, info(&format!("Zone signals (cumulative score ≥ {ZONE_SIGNAL})"), signals.to_string()));
            }
            if p.sizes.1 > 1 {
                let size = if p.sizes.0 == p.sizes.1 {
                    p.sizes.0.to_string()
                } else {
                    format!("{} to {}", p.sizes.0, p.sizes.1)
                };
                push(name, info("Subgroup size", size));
            }
            if phased {
                push(name, info("Points plotted", (p.end - p.start).to_string()));
            }
        }
        if !phased {
            push("", info("Points plotted", self.chart.location.values.len().to_string()));
        }
        if self.request.sigma_multiple != 3.0 {
            push("", info("Limits at", format!("{}σ", format_number(self.request.sigma_multiple, 4))));
        }
        if self.request.range_span != 2 && self.chart.kind == ChartKind::IndividualsAndMovingRange {
            push("", info("Values per moving range", self.request.range_span.to_string()));
        }
        if self.n_hidden > 0 {
            let shown = self.n_points - self.n_hidden;
            let how = if self.request.last.is_some_and(|l| l.limits_from_last) {
                format!("last {shown} of {} subgroups; limits from these only", self.n_points)
            } else {
                format!("last {shown} of {} subgroups; limits from all", self.n_points)
            };
            push("", info("Subgroups shown", how));
        }
        if varies_in_size {
            push("", info("Limits vary with subgroup size", "yes".to_owned()));
        }
        if self.leftover > 0 {
            push("", info("Trailing values not in a full subgroup", self.leftover.to_string()));
        }
        if self.n_short > 0 {
            push("", info("Subgroups with too few values (left out)", self.n_short.to_string()));
        }
        if self.n_excluded > 0 {
            push("", info("Rows excluded", self.n_excluded.to_string()));
        }
        if self.n_missing > 0 {
            push("", info("Rows with missing values", self.n_missing.to_string()));
        }
        section(SectionKind::ControlLimits, "Control Limits", SectionContent::Table(t))
    }

    /// Rows behind point `i` of a chart: a moving range spans two points, a
    /// moving average the points it averages (within the phase).
    fn rows_for(&self, i: usize, chart: &VaryingChart) -> Vec<usize> {
        let first = match chart.statistic {
            ChartStatistic::MovingRange => i.saturating_sub(self.request.range_span.max(2) - 1),
            ChartStatistic::MeanMovingRange | ChartStatistic::StandardizedMovingRange => i.saturating_sub(1),
            ChartStatistic::MovingAverage => {
                let start = self.chart.phases.iter().find(|p| (p.start..p.end).contains(&i)).map_or(0, |p| p.start);
                (i + 1).saturating_sub(self.request.span).max(start)
            }
            _ => i,
        };
        (first..=i).flat_map(|k| self.point_rows[k].iter().copied()).collect()
    }

    fn chart_spec(&self, chart: &VaryingChart, violations: &[RuleViolation], role: Role) -> ControlChartSpec {
        let zones = role == Role::Location && !self.chart.zones.is_empty();
        let points = chart
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                v.map(|value| {
                    let mut rules: Vec<u8> =
                        violations.iter().filter(|x| x.index == i).map(|x| x.rule.number()).collect();
                    rules.dedup();
                    let zone = zones.then(|| self.chart.zones[i]);
                    ChartPoint {
                        sample: i + 1,
                        value,
                        rows: self.rows_for(i, chart),
                        rules,
                        label: self.labels[i].clone(),
                        note: zone.map(|z| z.cumulative.to_string()),
                        signal: zone.is_some_and(|z| z.signal),
                    }
                })
            })
            .collect();
        let steps = limit_steps_at(&chart.limits, chart.sigma_multiple);
        let first = steps.first().copied().unwrap_or(LimitStep {
            from: 0.5,
            to: 1.5,
            center: f64::NAN,
            lower: f64::NAN,
            upper: f64::NAN,
            zone_sigma: f64::NAN,
        });
        let phases = self
            .chart
            .phases
            .iter()
            .zip(&self.phase_names)
            .map(|(p, name)| PhaseMark { at: p.start as f64 + 0.5, label: name.clone() })
            .collect();
        ControlChartSpec {
            x_label: "Sample".to_owned(),
            y_label: chart.statistic.label().to_owned(),
            x_ticks: None,
            flag_label: None,
            limit_label: None,
            points,
            center: first.center,
            lower: first.lower,
            upper: first.upper,
            zone_sigma: first.zone_sigma,
            steps: if steps.len() > 1 { steps } else { Vec::new() },
            phases,
        }
    }

    fn violation_table(
        &self,
        location: &[RuleViolation],
        dispersion: &[RuleViolation],
        within: &[RuleViolation],
    ) -> ReportSection {
        let zone_signals = self.zone_signals();
        if location.is_empty() && dispersion.is_empty() && within.is_empty() && zone_signals.is_empty() {
            let note = if self.chart.kind == ChartKind::Zone {
                "No cumulative zone score reaches 8, and no point violates the run rules applied."
            } else {
                "No points violate the run rules applied."
            };
            return section(SectionKind::RuleViolations, "Run Rule Violations", SectionContent::Note(note.to_owned()));
        }
        let has_labels = self.labels.iter().any(Option::is_some);
        let phased = self.phased();
        let mut header = vec!["Sample"];
        if has_labels {
            header.push("Label");
        }
        if phased {
            header.push("Phase");
        }
        header.extend(["Chart", "Value", "Rules", "Pattern"]);
        let mut t = TextTable::new(header);
        let mut charts = vec![(&self.chart.location, location, true), (&self.chart.dispersion, dispersion, false)];
        if let Some(w) = &self.chart.within {
            charts.push((w, within, false));
        }
        for (chart, list, is_location) in charts {
            // Points flagged by a rule or (on the location chart) by the zone score.
            let mut indices: Vec<usize> = list.iter().map(|v| v.index).collect();
            if is_location {
                indices.extend(&zone_signals);
            }
            indices.sort_unstable();
            indices.dedup();
            for index in indices {
                let same: Vec<&RuleViolation> = list.iter().filter(|v| v.index == index).collect();
                let value = chart.values[index].unwrap_or(f64::NAN);
                let mut row = vec![(index + 1).to_string()];
                if has_labels {
                    row.push(self.labels[index].clone().unwrap_or_default());
                }
                if phased {
                    let k = self.chart.phases.iter().position(|p| (p.start..p.end).contains(&index)).unwrap_or(0);
                    row.push(self.phase_names.get(k).cloned().unwrap_or_default());
                }
                let mut numbers: Vec<String> = same.iter().map(|v| v.rule.number().to_string()).collect();
                let mut patterns: Vec<String> = same.iter().map(|v| v.rule.description().to_owned()).collect();
                if is_location && self.chart.zones.get(index).is_some_and(|z| z.signal) {
                    numbers.push("zone".to_owned());
                    patterns.push(format!(
                        "Cumulative zone score {} (≥ {ZONE_SIGNAL}) on one side of the center line",
                        self.chart.zones[index].cumulative
                    ));
                }
                row.extend([
                    chart.statistic.label().to_owned(),
                    format_number(value, DIGITS),
                    numbers.join(", "),
                    patterns.join("; "),
                ]);
                t.push(row);
            }
        }
        section(SectionKind::RuleViolations, "Run Rule Violations", SectionContent::Table(t))
    }
}

/// Runs of points sharing the same limits, as horizontal steps.
pub(crate) fn limit_steps(limits: &[ControlLimits]) -> Vec<LimitStep> {
    limit_steps_at(limits, 3.0)
}

/// [`limit_steps`] for limits at `multiple` σ.
pub(crate) fn limit_steps_at(limits: &[ControlLimits], multiple: f64) -> Vec<LimitStep> {
    let mut out: Vec<LimitStep> = Vec::new();
    for (i, l) in limits.iter().enumerate() {
        let x = i as f64 + 1.0;
        match out.last_mut() {
            Some(s) if s.center == l.center && s.lower == l.lower && s.upper == l.upper => s.to = x + 0.5,
            _ => out.push(LimitStep {
                from: x - 0.5,
                to: x + 0.5,
                center: l.center,
                lower: l.lower,
                upper: l.upper,
                zone_sigma: (l.upper - l.center) / multiple,
            }),
        }
    }
    out
}

/// Column of a saved limits table holding the process center.
pub const PROCESS_CENTER: &str = "Process center";
/// Column of a saved limits table holding the process σ.
pub const PROCESS_SIGMA: &str = "Process sigma";

/// Drop the first `cut` points of every chart of `chart`, with the phases
/// (and zone scores) that held them.
fn keep_last(chart: &mut PhasedChart, cut: usize) {
    for c in [&mut chart.location, &mut chart.dispersion].into_iter().chain(chart.within.as_mut()) {
        c.values.drain(..cut);
        c.limits.drain(..cut);
    }
    if !chart.zones.is_empty() {
        chart.zones.drain(..cut);
    }
    chart.phases.retain(|p| p.end > cut);
    for p in &mut chart.phases {
        p.start = p.start.saturating_sub(cut);
        p.end -= cut;
    }
}

fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n == 0 {
        f64::NAN
    } else if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// The standard values for measurement `name` in a limits table saved by
/// [`ControlChartAnalysis::limits_data_table`] (or any table with
/// "Measurement", "Process center" and "Process sigma" columns): the last row
/// for that measurement with a finite center and a positive σ.
#[must_use]
pub fn standard_from_table(table: &DataTable, name: &str) -> Option<StandardValues> {
    let m = table.column(table.find("Measurement")?)?;
    let c = table.column(table.find(PROCESS_CENTER)?)?.as_numeric()?;
    let s = table.column(table.find(PROCESS_SIGMA)?)?.as_numeric()?;
    (0..table.n_rows())
        .rev()
        .find(|&r| m.level_text(r) == name && c[r].is_finite() && s[r].is_finite() && s[r] > 0.0)
        .map(|r| StandardValues { center: c[r], sigma: s[r] })
}

/// The limits table of a control-chart report (`None` for any other
/// report; an error when the chart cannot be computed).
#[must_use]
pub fn saved_limits_table(
    saved: &crate::SavedReport,
    table: &DataTable,
    rows: &RowStateManager,
) -> Option<Result<DataTable>> {
    let crate::SavedReport::ControlChart { request, .. } = saved else { return None };
    Some(ControlChartAnalysis::run(table, rows, request).map(|a| a.limits_data_table()))
}

fn info(label: &str, value: String) -> [String; 4] {
    [label.to_owned(), value, String::new(), String::new()]
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
    use stat_data::Column;

    /// NIST flow-rate example plus a deliberate outlier in row 4.
    fn table() -> DataTable {
        let x = vec![49.6, 47.6, 49.9, 51.3, 47.8, 51.2, 52.6, 52.4, 53.6, 52.1, f64::NAN, 50.5];
        DataTable::new("t", vec![Column::numeric("flow", x)]).unwrap()
    }

    fn imr() -> ControlChartRequest {
        ControlChartRequest::new(0, ChartKind::IndividualsAndMovingRange, 1)
    }

    fn texts(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn individuals_report_maps_points_to_rows() {
        let t = table();
        let states = RowStateManager::new(t.n_rows());
        let a = ControlChartAnalysis::run(&t, &states, &imr()).unwrap();
        assert_eq!(a.title(), "Control Chart: Individuals and Moving Range — flow");
        let s = a.sections(&ControlChartOptions::default());
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::ControlLimits,
                SectionKind::LocationChart,
                SectionKind::DispersionChart,
                SectionKind::RuleViolations
            ]
        );
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        assert_eq!(loc.points.len(), 11, "the missing value is skipped");
        assert!(loc.steps.is_empty() && loc.phases.is_empty(), "one set of limits");
        // The point after the missing row maps to row 11, not row 10.
        assert_eq!(loc.points[10].rows, vec![11]);
        let SectionContent::ControlChart(mr) = &s[2].content else { panic!() };
        assert_eq!(mr.points.len(), 10, "first moving range undefined");
        assert_eq!(mr.points[0].sample, 2);
        assert_eq!(mr.points[0].rows, vec![0, 1], "a moving range spans two rows");
        assert_eq!(mr.points[9].rows, vec![9, 11]);
        let SectionContent::Table(limits) = &s[0].content else { panic!() };
        assert!(limits.rows.iter().any(|r| r[0] == "Rows with missing values" && r[1] == "1"));
    }

    #[test]
    fn violations_are_flagged_and_exclusion_recomputes() {
        // A process with one wild value.
        let mut x: Vec<f64> = (0..20).map(|i| 10.0 + if i % 2 == 0 { 0.3 } else { -0.3 }).collect();
        x[12] = 25.0;
        let t = DataTable::new("t", vec![Column::numeric("y", x)]).unwrap();
        let mut states = RowStateManager::new(t.n_rows());
        let a = ControlChartAnalysis::run(&t, &states, &imr()).unwrap();
        let options = ControlChartOptions::default();
        let v = a.location_violations(&options);
        assert!(v.iter().any(|v| v.index == 12 && v.rule == RunRule::BeyondLimits));
        let s = a.sections(&options);
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        assert_eq!(loc.points[12].rules.first(), Some(&1));
        let SectionContent::Table(tab) = &s[3].content else { panic!("expected a violation table") };
        assert!(tab.rows.iter().any(|r| r[0] == "13" && r[3].starts_with('1')));

        // Turning rule 1 off on the location chart removes that flag.
        let mut fewer = options;
        fewer.location_rules.set(RunRule::BeyondLimits, false);
        assert!(!a.location_violations(&fewer).iter().any(|v| v.rule == RunRule::BeyondLimits));

        // Excluding the outlier makes the analysis stale; the recomputed chart has
        // 19 points and tighter limits.
        states.set_excluded([12], true);
        assert!(a.is_stale(&states));
        let b = ControlChartAnalysis::run(&t, &states, &imr()).unwrap();
        assert_eq!(b.charts().location.values.len(), 19);
        assert!(b.charts().location.limits[0].upper < a.charts().location.limits[0].upper);
    }

    #[test]
    fn subgroup_charts_group_consecutive_rows() {
        let x: Vec<f64> = (0..13).map(|i| f64::from(i % 5)).collect();
        let t = DataTable::new("t", vec![Column::numeric("y", x)]).unwrap();
        let states = RowStateManager::new(t.n_rows());
        let req = ControlChartRequest::new(0, ChartKind::MeanAndRange, 4);
        let a = ControlChartAnalysis::run(&t, &states, &req).unwrap();
        let s = a.sections(&ControlChartOptions { show_violation_table: false, ..Default::default() });
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        assert_eq!(loc.points.len(), 3);
        assert_eq!(loc.points[1].rows, vec![4, 5, 6, 7]);
        let SectionContent::Table(limits) = &s[0].content else { panic!() };
        assert!(limits.rows.iter().any(|r| r[0].starts_with("Trailing") && r[1] == "1"));
        assert_eq!(s.len(), 3);
        let bad = ControlChartRequest { subgroup_size: 1, ..req };
        assert!(ControlChartAnalysis::run(&t, &states, &bad).is_err());
    }

    #[test]
    fn subgroup_id_column_allows_varying_sizes_and_labels_points() {
        // Lots A (3 values), B (2), C (4), with rows of A and B interleaved.
        let y = vec![10.0, 11.0, 12.0, 9.0, 10.5, 11.5, 10.0, 10.2, 9.8, 10.4];
        let id = texts(&["A", "A", "B", "A", "B", "C", "C", "C", "C", "D"]);
        let t = DataTable::new("t", vec![Column::numeric("y", y.clone()), Column::text("lot", id)]).unwrap();
        let states = RowStateManager::new(t.n_rows());
        let req = ControlChartRequest { subgroup: Some(1), ..ControlChartRequest::new(0, ChartKind::MeanAndRange, 5) };
        let a = ControlChartAnalysis::run(&t, &states, &req).unwrap();
        let s = a.sections(&ControlChartOptions::default());
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        assert_eq!(loc.points.len(), 3, "lot D has one value and is left out");
        assert_eq!(loc.points[0].rows, vec![0, 1, 3]);
        assert_eq!(loc.points[1].rows, vec![2, 4]);
        assert_eq!(loc.points[1].label.as_deref(), Some("B"));
        assert!((loc.points[0].value - 10.0).abs() < 1e-12);
        // The limits depend on the subgroup size: three steps of width one.
        assert_eq!(loc.steps.len(), 3);
        assert!((loc.steps[1].to - 2.5).abs() < 1e-12);
        let narrow = loc.steps[2].upper - loc.steps[2].center;
        let wide = loc.steps[1].upper - loc.steps[1].center;
        assert!(wide > narrow, "a smaller subgroup has wider limits");
        let SectionContent::Table(limits) = &s[0].content else { panic!() };
        assert!(limits.rows.iter().any(|r| r[0] == "Subgroup size" && r[1] == "2 to 4"));
        assert!(limits.rows.iter().any(|r| r[0].starts_with("Subgroups with too few") && r[1] == "1"));
        // A label column overrides the subgroup ID as the label.
        let labelled = DataTable::new(
            "t",
            vec![
                Column::numeric("y", y),
                Column::text("lot", texts(&["A", "A", "B", "A", "B", "C", "C", "C", "C", "D"])),
                Column::text("when", texts(&["Mon", "x", "Tue", "x", "x", "Wed", "x", "x", "x", "x"])),
            ],
        )
        .unwrap();
        let req = ControlChartRequest { label: Some(2), ..req };
        let a = ControlChartAnalysis::run(&labelled, &states, &req).unwrap();
        let s = a.sections(&ControlChartOptions::default());
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        let names: Vec<_> = loc.points.iter().map(|p| p.label.clone().unwrap()).collect();
        assert_eq!(names, ["Mon", "Tue", "Wed"]);
    }

    #[test]
    fn phase_column_gives_each_phase_its_limits() {
        let mut y: Vec<f64> = (0..10).map(|i| 10.0 + if i % 2 == 0 { 0.5 } else { -0.5 }).collect();
        y.extend((0..10).map(|i| 20.0 + if i % 2 == 0 { 0.1 } else { -0.1 }));
        let phase: Vec<String> = (0..20).map(|i| if i < 10 { "Before" } else { "After" }.to_owned()).collect();
        let t = DataTable::new("t", vec![Column::numeric("y", y), Column::text("stage", phase)]).unwrap();
        let states = RowStateManager::new(t.n_rows());
        let req = ControlChartRequest { phase: Some(1), ..imr() };
        let a = ControlChartAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(a.phase_names(), ["Before", "After"]);
        let s = a.sections(&ControlChartOptions::default());
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        assert_eq!(loc.steps.len(), 2);
        assert!((loc.steps[0].center - 10.0).abs() < 1e-12 && (loc.steps[1].center - 20.0).abs() < 1e-12);
        assert!((loc.steps[1].from - 10.5).abs() < 1e-12);
        assert_eq!(loc.phases.len(), 2);
        assert_eq!(loc.phases[1], PhaseMark { at: 10.5, label: "After".to_owned() });
        // The shift does not flag anything: each phase is in control on its own.
        assert!(loc.points.iter().all(|p| p.rules.is_empty()));
        let SectionContent::ControlChart(mr) = &s[2].content else { panic!() };
        assert_eq!(mr.points.len(), 18, "no moving range across the phase boundary");
        let SectionContent::Table(limits) = &s[0].content else { panic!() };
        assert_eq!(limits.columns[0], "Phase");
        assert!(limits.rows.iter().any(|r| r[0] == "After" && r[1] == "Individual value"));
        // Without the phase column the shift is a run of violations.
        let one = ControlChartAnalysis::run(&t, &states, &imr()).unwrap();
        assert!(!one.location_violations(&ControlChartOptions::default()).is_empty());
    }

    #[test]
    fn moving_average_links_the_averaged_rows_and_keeps_rule_one_only() {
        let x: Vec<f64> = (0..12).map(|i| 10.0 + f64::from(i % 3)).collect();
        let t = DataTable::new("t", vec![Column::numeric("y", x)]).unwrap();
        let states = RowStateManager::new(t.n_rows());
        let req = ControlChartRequest { span: 3, ..ControlChartRequest::new(0, ChartKind::MovingAverage, 1) };
        assert!(req.plots_individuals());
        let a = ControlChartAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(a.title(), "Control Chart: Moving Average (span 3) — y");
        let options = ControlChartOptions::default();
        assert_eq!(a.effective_location_rules(&options), RuleSet::LIMITS_ONLY);
        let s = a.sections(&options);
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        assert_eq!(loc.y_label, "Moving average");
        assert_eq!(loc.points[0].rows, vec![0]);
        assert_eq!(loc.points[5].rows, vec![3, 4, 5]);
        assert_eq!(loc.steps.len(), 3, "limits narrow over the first w points");
        let SectionContent::Table(limits) = &s[0].content else { panic!() };
        assert!(limits.rows.iter().any(|r| r[0] == "Moving-average span" && r[1] == "3"));
        // With subgroups of 4 the companion chart is a range chart.
        let sub = ControlChartRequest { subgroup_size: 4, ..req };
        assert!(!sub.plots_individuals());
        let b = ControlChartAnalysis::run(&t, &states, &sub).unwrap();
        assert_eq!(b.charts().dispersion.statistic, ChartStatistic::SubgroupRange);
    }

    #[test]
    fn three_way_adds_a_within_chart() {
        // Subgroups of 3 whose means wander more than the within spread allows.
        let shifts = [0.0, 2.0, -1.0, 3.0, 1.0, -2.0, 0.5, 2.5];
        let y: Vec<f64> = shifts.iter().flat_map(|s| [10.0 + s, 10.5 + s, 9.7 + s]).collect();
        let t = DataTable::new("t", vec![Column::numeric("y", y)]).unwrap();
        let states = RowStateManager::new(t.n_rows());
        let options = ControlChartOptions::default();
        let three =
            ControlChartAnalysis::run(&t, &states, &ControlChartRequest::new(0, ChartKind::ThreeWayRange, 3)).unwrap();
        let s = three.sections(&options);
        let kinds: Vec<_> = s.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::ControlLimits,
                SectionKind::LocationChart,
                SectionKind::DispersionChart,
                SectionKind::WithinChart,
                SectionKind::RuleViolations
            ]
        );
        assert_eq!(s[2].title, "Moving range of means Chart");
        assert_eq!(s[3].title, "Within-Subgroup Range Chart");
        let SectionContent::ControlChart(mr) = &s[2].content else { panic!() };
        assert_eq!(mr.points[0].rows, vec![0, 1, 2, 3, 4, 5], "a moving range of means spans two subgroups");
        let SectionContent::Table(limits) = &s[0].content else { panic!() };
        assert!(limits.rows.iter().any(|r| r[0].starts_with("σ of subgroup means")));
        assert!(three.location_violations(&options).is_empty());
        // The X̄ chart with within-subgroup limits flags most means.
        let xbar =
            ControlChartAnalysis::run(&t, &states, &ControlChartRequest::new(0, ChartKind::MeanAndRange, 3)).unwrap();
        assert!(xbar.location_violations(&options).len() > 3);
        // Hiding the dispersion charts hides the within chart too.
        let hidden = three.sections(&ControlChartOptions { show_dispersion_chart: false, ..options });
        assert!(hidden.iter().all(|x| x.kind != SectionKind::WithinChart));
    }

    #[test]
    fn zone_chart_notes_scores_and_flags_signals() {
        let mut x: Vec<f64> = (0..20).map(|i| 10.0 + if i % 2 == 0 { 0.4 } else { -0.4 }).collect();
        // Four points in a row between 1σ and 2σ above the center: 2 + 2 + 2 + 2 = 8 at the fourth.
        for v in &mut x[10..14] {
            *v = 11.5;
        }
        let t = DataTable::new("t", vec![Column::numeric("y", x)]).unwrap();
        let states = RowStateManager::new(t.n_rows());
        let a = ControlChartAnalysis::run(&t, &states, &ControlChartRequest::new(0, ChartKind::Zone, 1)).unwrap();
        let signals = a.zone_signals();
        assert!(!signals.is_empty());
        let s = a.sections(&ControlChartOptions::default());
        assert_eq!(s[1].title, "Zone Chart of Individual value");
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        assert!(loc.points.iter().all(|p| p.note.is_some()));
        assert!(loc.points[signals[0]].signal && loc.points[signals[0]].flagged());
        let SectionContent::Table(v) = &s[3].content else { panic!("expected flagged points") };
        assert!(v.rows.iter().any(|r| r[3].contains("zone")), "{:?}", v.rows);
    }

    #[test]
    fn short_run_standardizes_each_part() {
        let y = vec![10.0, 50.0, 10.4, 51.0, 9.8, 49.5, 50.5, 10.2, f64::NAN];
        let part = texts(&["A-1", "B-2", "A-1", "B-2", "A-1", "B-2", "B-2", "A-1", "A-1"]);
        let t = DataTable::new("t", vec![Column::numeric("y", y), Column::text("Part", part)]).unwrap();
        let states = RowStateManager::new(t.n_rows());
        let req = ControlChartRequest { part: Some(1), ..ControlChartRequest::new(0, ChartKind::ShortRun, 1) };
        assert_eq!(req.columns(), [0, 1]);
        let a = ControlChartAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(a.part_names(), ["A-1", "B-2"]);
        let s = a.sections(&ControlChartOptions::default());
        let SectionContent::ControlChart(z) = &s[1].content else { panic!() };
        assert_eq!(z.y_label, "Standardized value (Z)");
        assert_eq!((z.center, z.lower, z.upper), (0.0, -3.0, 3.0));
        assert_eq!(z.points[1].label.as_deref(), Some("B-2"), "points are labelled by part");
        let part_table = s.iter().find(|x| x.kind == SectionKind::PartEstimates).unwrap();
        let SectionContent::Table(p) = &part_table.content else { panic!() };
        assert_eq!(p.columns[0], "Part");
        assert_eq!(p.rows.len(), 2);
        assert_eq!(p.rows[0][1], "4");
        // A part with a single value is named in the error.
        let mut lone = states.clone();
        lone.set_excluded([0, 2, 4], true);
        let e = ControlChartAnalysis::run(&t, &lone, &req).unwrap_err().to_string();
        assert!(e.contains("\"A-1\"") && !e.contains("phase"), "{e}");
        // The part column is required.
        let none = ControlChartRequest { part: None, ..req };
        assert!(ControlChartAnalysis::run(&t, &states, &none).is_err());
    }

    #[test]
    fn last_subgroups_options_run_chart_and_saved_limits() {
        let t = table();
        let states = RowStateManager::new(t.n_rows());
        let all = ControlChartAnalysis::run(&t, &states, &imr()).unwrap();
        // The last 6 of 11 values, limits from all: the same limits, fewer points.
        let shown = ControlChartRequest { last: Some(LastSubgroups { count: 6, limits_from_last: false }), ..imr() };
        let a = ControlChartAnalysis::run(&t, &states, &shown).unwrap();
        assert_eq!(a.charts().location.values.len(), 6);
        assert_eq!(a.charts().location.limits[0], all.charts().location.limits[0]);
        assert_eq!(a.charts().location.values[0], all.charts().location.values[5]);
        assert_eq!(a.charts().dispersion.values[0], all.charts().dispersion.values[5], "its moving range is kept");
        let s = a.sections(&ControlChartOptions::default());
        let SectionContent::ControlChart(loc) = &s[1].content else { panic!() };
        assert_eq!(loc.points[0].rows, vec![5]);
        let SectionContent::Table(limits) = &s[0].content else { panic!() };
        assert!(limits.rows.iter().any(|r| r[0] == "Subgroups shown" && r[1].contains("last 6 of 11")));
        // Limits from the last 6 only: their own mean.
        let own = ControlChartRequest { last: Some(LastSubgroups { count: 6, limits_from_last: true }), ..imr() };
        let b = ControlChartAnalysis::run(&t, &states, &own).unwrap();
        let last6 = [51.2, 52.6, 52.4, 53.6, 52.1, 50.5];
        assert!((b.charts().location.limits[0].center - last6.iter().sum::<f64>() / 6.0).abs() < 1e-12);
        assert!(
            ControlChartAnalysis::run(
                &t,
                &states,
                &ControlChartRequest { last: Some(LastSubgroups { count: 1, limits_from_last: false }), ..imr() }
            )
            .is_err()
        );

        // Moving ranges of 3 link three rows; limits at 2σ.
        let wide = ControlChartRequest { range_span: 3, sigma_multiple: 2.0, ..imr() };
        let w = ControlChartAnalysis::run(&t, &states, &wide).unwrap();
        let s = w.sections(&ControlChartOptions::default());
        let SectionContent::ControlChart(mr) = &s[2].content else { panic!() };
        assert_eq!(mr.points[0].rows, vec![0, 1, 2]);
        assert!((mr.points[0].value - (49.9 - 47.6)).abs() < 1e-12);
        let sigma = w.charts().phases[0].sigma;
        assert!((loc_half_width(&w) - 2.0 * sigma).abs() < 1e-12);

        // The saved limits table gives back the process center and σ as standard values.
        let saved = all.limits_data_table();
        assert_eq!(saved.n_rows(), 2);
        let sv = standard_from_table(&saved, "flow").unwrap();
        assert_eq!(sv.center, all.charts().phases[0].center);
        assert_eq!(sv.sigma, all.charts().phases[0].sigma);
        assert!(standard_from_table(&saved, "other").is_none());
        let historical = ControlChartRequest { standard: Some(StandardValues { center: 50.0, sigma: 1.0 }), ..imr() };
        let h = ControlChartAnalysis::run(&t, &states, &historical).unwrap();
        let l = h.charts().location.limits[0];
        assert_eq!((l.center, l.lower, l.upper), (50.0, 47.0, 53.0));
        let s = h.sections(&ControlChartOptions::default());
        let SectionContent::Table(limits) = &s[0].content else { panic!() };
        assert!(limits.rows.iter().any(|r| r[0] == "Process σ (standard value)" && r[1] == "1"));

        // Run chart: every value, the center line at the median.
        let options = ControlChartOptions { show_run_chart: true, ..ControlChartOptions::default() };
        let s = all.sections(&options);
        let run = s.iter().find(|x| x.kind == SectionKind::RunChart).unwrap();
        let SectionContent::Plot(p) = &run.content else { panic!() };
        assert_eq!(p.points.len(), 11);
        let mut v: Vec<f64> =
            t.column(0).unwrap().as_numeric().unwrap().iter().copied().filter(|x| x.is_finite()).collect();
        v.sort_by(f64::total_cmp);
        assert_eq!(p.segments[0][0][1], v[5]);
        // Subgroups: the line joins the subgroup means.
        let sub = ControlChartRequest::new(0, ChartKind::MeanAndRange, 3);
        let g = ControlChartAnalysis::run(&t, &states, &sub).unwrap();
        let options = ControlChartOptions { run_center: RunCenter::Mean, ..options };
        let s = g.sections(&options);
        let run = s.iter().find(|x| x.kind == SectionKind::RunChart).unwrap();
        let SectionContent::Plot(p) = &run.content else { panic!() };
        assert_eq!(p.points.len(), 9);
        assert!((p.line.as_ref().unwrap()[0][1] - (49.6 + 47.6 + 49.9) / 3.0).abs() < 1e-12);
    }

    fn loc_half_width(a: &ControlChartAnalysis) -> f64 {
        let l = a.charts().location.limits[0];
        l.upper - l.center
    }
}
