//! Multivariate control charts of several measurement columns, bound to the
//! row state ([`stat_quality::multivariate`]): Hotelling T² of individual
//! observations or subgroups, the generalized variance |S| of subgroups, and
//! the MEWMA chart of individual observations.
//!
//! Rows are taken in table order, skipping excluded rows and rows missing a
//! value in any measurement column (or the subgroup ID). Subgroups are the
//! rows sharing a subgroup ID (in order of first appearance), or consecutive
//! runs of `subgroup_size` rows (an incomplete last run is left out). The
//! first `reference` points (all when `None`) estimate the mean vector and
//! covariance matrix; later points are monitored against them, with their
//! own (phase II) limits. Every plotted point records its dataset rows, so
//! brushing the chart selects them; for T² and MEWMA signals the report adds
//! each variable's contribution to the statistic.

use crate::format::format_number;
use crate::report::{
    Annotations, ChartPoint, ControlChartSpec, LimitStep, PhaseMark, PlotSpec, PointGroups, ReportSection, RowPoint,
    SectionContent, SectionKind, TextTable,
};
use stat_core::{ChiSquared, ContinuousDistribution, Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_quality::{
    ChartPhase, GeneralizedVarianceChart, HotellingChart, MewmaChart, MewmaLimit, MultivariatePoint,
    generalized_variance_chart, hotelling_t2_chart, mewma_chart,
};

const DIGITS: usize = 5;

/// Signals shown in the contribution plot (the largest statistics).
pub const CONTRIBUTION_PLOT_POINTS: usize = 8;

/// Which multivariate chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultivariateChartKind {
    /// Hotelling T² of individual observations or subgroup means.
    HotellingT2,
    /// Generalized variance |S| of subgroups.
    GeneralizedVariance,
    /// Multivariate EWMA of individual observations.
    Mewma,
}

impl MultivariateChartKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 3] = [Self::HotellingT2, Self::GeneralizedVariance, Self::Mewma];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::HotellingT2 => "Hotelling T²",
            Self::GeneralizedVariance => "Generalized variance |S|",
            Self::Mewma => "MEWMA",
        }
    }

    /// Stable key used in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::HotellingT2 => "hotelling-t2",
            Self::GeneralizedVariance => "generalized-variance",
            Self::Mewma => "mewma",
        }
    }

    /// The kind with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// What to chart.
#[derive(Debug, Clone, PartialEq)]
pub struct MultivariateChartRequest {
    /// Measurement columns (two or more).
    pub columns: Vec<usize>,
    /// Chart type.
    pub kind: MultivariateChartKind,
    /// Subgroup ID column (T² and |S|; ignored by MEWMA).
    pub subgroup: Option<usize>,
    /// Rows per subgroup without an ID column (1: individual observations;
    /// ignored by MEWMA).
    pub subgroup_size: usize,
    /// Column naming each point.
    pub label: Option<usize>,
    /// Points (subgroups or observations) in the reference sample, from the
    /// start; `None` uses every point.
    pub reference: Option<usize>,
    /// False-alarm probability of each T² limit.
    pub alpha: f64,
    /// MEWMA smoothing weight λ.
    pub lambda: f64,
    /// MEWMA upper limit (a value, or an in-control average run length).
    pub mewma_limit: MewmaLimit,
}

impl MultivariateChartRequest {
    /// Default false-alarm probability (that of three-sigma limits).
    pub const DEFAULT_ALPHA: f64 = 0.0027;
    /// Default MEWMA weight.
    pub const DEFAULT_LAMBDA: f64 = 0.1;
    /// Default MEWMA limit: an in-control average run length of 200.
    pub const DEFAULT_MEWMA_LIMIT: MewmaLimit = MewmaLimit::InControlArl(200.0);

    /// A request with default settings (individual observations, except
    /// subgroups of five for the generalized variance chart).
    #[must_use]
    pub fn new(columns: Vec<usize>, kind: MultivariateChartKind) -> Self {
        Self {
            columns,
            kind,
            subgroup: None,
            subgroup_size: if kind == MultivariateChartKind::GeneralizedVariance { 5 } else { 1 },
            label: None,
            reference: None,
            alpha: Self::DEFAULT_ALPHA,
            lambda: Self::DEFAULT_LAMBDA,
            mewma_limit: Self::DEFAULT_MEWMA_LIMIT,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = self.columns.clone();
        c.extend(self.subgroup);
        c.extend(self.label);
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.columns.iter_mut().for_each(|c| *c = f(*c));
        self.subgroup = self.subgroup.map(f);
        self.label = self.label.map(f);
    }

    fn uses_subgroups(&self) -> bool {
        self.kind != MultivariateChartKind::Mewma && (self.subgroup.is_some() || self.subgroup_size > 1)
    }
}

#[derive(Debug, Clone)]
enum Chart {
    T2(HotellingChart),
    Variance(GeneralizedVarianceChart),
    Mewma(MewmaChart),
}

/// A computed multivariate chart.
#[derive(Debug, Clone)]
pub struct MultivariateChartAnalysis {
    request: MultivariateChartRequest,
    names: Vec<String>,
    chart: Chart,
    /// Dataset rows behind each point.
    point_rows: Vec<Vec<usize>>,
    labels: Vec<Option<String>>,
    n_excluded: usize,
    n_missing: usize,
    leftover: usize,
    exclusion_revision: u64,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl MultivariateChartAnalysis {
    /// Compute the chart.
    ///
    /// # Errors
    /// Fewer than two measurement columns, a missing or non-numeric column, a
    /// subgroup size of zero, no complete points, and errors from the chart
    /// (see [`stat_quality::multivariate`]).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &MultivariateChartRequest) -> Result<Self> {
        if request.columns.len() < 2 {
            return Err(invalid("choose at least two measurement columns"));
        }
        let mut names = Vec::new();
        let mut values: Vec<&[f64]> = Vec::new();
        for &i in &request.columns {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            values.push(c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?);
            names.push(c.name.clone());
        }
        let role = |c: Option<usize>| -> Result<Option<&stat_data::Column>> {
            c.map(|i| table.column(i).ok_or_else(|| invalid(format!("no column {i}")))).transpose()
        };
        let subgrouped = request.uses_subgroups();
        let id_col = if subgrouped { role(request.subgroup)? } else { None };
        let label_col = role(request.label)?;
        let (mut n_excluded, mut n_missing) = (0, 0);
        let mut usable = Vec::new();
        for r in 0..table.n_rows() {
            let complete = values.iter().all(|v| v[r].is_finite()) && id_col.is_none_or(|c| !c.level_text(r).is_empty());
            if !complete {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                usable.push(r);
            }
        }
        let mut leftover = 0;
        let point_rows: Vec<Vec<usize>> = match (subgrouped, id_col) {
            (false, _) => usable.iter().map(|&r| vec![r]).collect(),
            (true, Some(c)) => {
                let mut ids: Vec<String> = Vec::new();
                let mut members: Vec<Vec<usize>> = Vec::new();
                for &r in &usable {
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
            }
            (true, None) => {
                let size = request.subgroup_size;
                if size == 0 {
                    return Err(invalid("the subgroup size must be at least 1"));
                }
                leftover = usable.len() % size;
                usable.chunks_exact(size).map(<[usize]>::to_vec).collect()
            }
        };
        if point_rows.is_empty() {
            return Err(invalid("no complete rows to chart"));
        }
        let labels = point_rows
            .iter()
            .map(|rows| label_col.or(id_col).map(|c| c.level_text(rows[0])).filter(|s| !s.is_empty()))
            .collect();
        let groups: Vec<Vec<Vec<f64>>> = point_rows
            .iter()
            .map(|rows| rows.iter().map(|&r| values.iter().map(|v| v[r]).collect()).collect())
            .collect();
        let chart = match request.kind {
            MultivariateChartKind::HotellingT2 => {
                Chart::T2(hotelling_t2_chart(&groups, request.reference, request.alpha)?)
            }
            MultivariateChartKind::GeneralizedVariance => {
                if !subgrouped {
                    return Err(invalid(
                        "a generalized variance chart needs subgroups: give a subgroup column or size",
                    ));
                }
                Chart::Variance(generalized_variance_chart(&groups, request.reference, 3.0)?)
            }
            MultivariateChartKind::Mewma => {
                let obs: Vec<Vec<f64>> = groups.into_iter().map(|mut g| g.remove(0)).collect();
                Chart::Mewma(mewma_chart(&obs, request.reference, request.lambda, request.mewma_limit)?)
            }
        };
        Ok(Self {
            request: request.clone(),
            names,
            chart,
            point_rows,
            labels,
            n_excluded,
            n_missing,
            leftover,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &MultivariateChartRequest {
        &self.request
    }

    /// `true` if row exclusion has changed since the chart was computed.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title, e.g. `"Hotelling T² Chart of Width, Height"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("{} Chart of {}", self.request.kind.label(), self.names.join(", "))
    }

    fn points(&self) -> &[MultivariatePoint] {
        match &self.chart {
            Chart::T2(c) => &c.points,
            Chart::Variance(c) => &c.points,
            Chart::Mewma(c) => &c.points,
        }
    }

    fn reference_count(&self) -> usize {
        match &self.chart {
            Chart::T2(c) => c.reference_count,
            Chart::Variance(c) => c.reference_count,
            Chart::Mewma(c) => c.reference_count,
        }
    }

    fn contributions(&self, i: usize) -> Option<Vec<f64>> {
        match &self.chart {
            Chart::T2(c) => c.contributions(i).ok(),
            Chart::Mewma(c) => c.contributions(i).ok(),
            Chart::Variance(_) => None,
        }
    }

    /// Indices of the points outside their limits.
    #[must_use]
    pub fn signals(&self) -> Vec<usize> {
        self.points().iter().enumerate().filter(|(_, p)| p.signal).map(|(i, _)| i).collect()
    }

    /// Dataset rows of the points outside their limits.
    #[must_use]
    pub fn signal_rows(&self) -> Vec<usize> {
        self.signals().into_iter().flat_map(|i| self.point_rows[i].iter().copied()).collect()
    }

    /// Report sections: summary, chart, signals, contributions (T² and
    /// MEWMA) and the reference estimates.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary(), self.chart_section(), self.signal_table()];
        if !matches!(self.chart, Chart::Variance(_)) {
            out.extend(self.contribution_sections());
        }
        out.push(self.estimates());
        out
    }

    fn statistic_label(&self) -> &'static str {
        match self.chart {
            Chart::T2(_) => "T²",
            Chart::Variance(_) => "|S|",
            Chart::Mewma(_) => "MEWMA T²",
        }
    }

    fn point_name(&self, i: usize) -> String {
        match &self.labels[i] {
            Some(l) => format!("{} ({l})", i + 1),
            None => (i + 1).to_string(),
        }
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        let total = self.points().len();
        let m = self.reference_count();
        push("Chart", self.request.kind.label().to_owned());
        push("Variables", self.names.join(", "));
        let n = self.point_rows.first().map_or(1, Vec::len);
        match &self.chart {
            Chart::Mewma(_) => push("Observations", total.to_string()),
            _ if n == 1 && !self.request.uses_subgroups() => push("Observations", total.to_string()),
            _ => {
                push("Subgroups", total.to_string());
                push("Subgroup size", subgroup_size_text(&self.point_rows));
            }
        }
        push("Reference points (phase I)", m.to_string());
        if m < total {
            push("Monitored points (phase II)", (total - m).to_string());
        }
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        if self.leftover > 0 {
            push("Rows left over (incomplete last subgroup)", self.leftover.to_string());
        }
        match &self.chart {
            Chart::T2(c) => {
                push("False-alarm probability α", format_number(c.alpha, DIGITS));
                let dist = if c.subgroup_size == 1 { "scaled beta" } else { "scaled F" };
                push(&format!("Upper limit, reference points ({dist})"), format_number(c.reference_limit, DIGITS));
                if c.monitoring_limit.is_finite() {
                    push("Upper limit, monitored points (scaled F)", format_number(c.monitoring_limit, DIGITS));
                }
            }
            Chart::Variance(c) => {
                push("b₁", format_number(c.constants.0, DIGITS));
                push("b₂", format_number(c.constants.1, DIGITS));
                push("Center line |S̄|", format_number(c.center, DIGITS));
                push("Lower control limit", format_number(c.lower, DIGITS));
                push("Upper control limit", format_number(c.upper, DIGITS));
            }
            Chart::Mewma(c) => {
                push("Smoothing weight λ", format_number(c.lambda, DIGITS));
                push("Upper control limit H", format_number(c.limit, DIGITS));
                if let Some(arl) = c.target_arl {
                    push("H chosen for an in-control average run length of", format_number(arl, DIGITS));
                }
            }
        }
        push("Points outside the limits", self.signals().len().to_string());
        section(SectionKind::MultivariateSummary, "Chart Summary", SectionContent::Table(t))
    }

    fn chart_section(&self) -> ReportSection {
        let pts = self.points();
        let total = pts.len();
        let m = self.reference_count();
        let points = pts
            .iter()
            .enumerate()
            .map(|(i, p)| ChartPoint {
                sample: i + 1,
                value: p.statistic,
                rows: self.point_rows[i].clone(),
                rules: if p.signal { vec![1] } else { Vec::new() },
                label: self.labels[i].clone(),
                note: None,
                signal: false,
            })
            .collect();
        let center = match &self.chart {
            Chart::Variance(c) => c.center,
            _ => f64::NAN,
        };
        let lower = |p: &MultivariatePoint| if p.lower > 0.0 { p.lower } else { f64::NAN };
        let step = |from: usize, to: usize, p: &MultivariatePoint| LimitStep {
            from: from as f64 + 0.5,
            to: to as f64 + 0.5,
            center,
            lower: lower(p),
            upper: p.upper,
            zone_sigma: 0.0,
        };
        let (steps, phases) = if m < total {
            (
                vec![step(0, m, &pts[0]), step(m, total, &pts[m])],
                vec![
                    PhaseMark { at: 0.5, label: "Reference".to_owned() },
                    PhaseMark { at: m as f64 + 0.5, label: "Monitoring".to_owned() },
                ],
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let x_label = if self.request.uses_subgroups() { "Subgroup" } else { "Observation" };
        let spec = ControlChartSpec {
            x_label: x_label.to_owned(),
            y_label: self.statistic_label().to_owned(),
            x_ticks: None,
            flag_label: Some("Outside the limits".to_owned()),
            limit_label: None,
            points,
            center,
            lower: lower(&pts[0]),
            upper: pts[0].upper,
            zone_sigma: 0.0,
            steps,
            phases,
        };
        let title = format!("{} Chart", self.request.kind.label());
        section(SectionKind::MultivariateChart, title, SectionContent::ControlChart(spec))
    }

    fn rows_text(&self, i: usize) -> String {
        let rows = &self.point_rows[i];
        match (rows.first(), rows.last()) {
            (Some(a), Some(b)) if a == b => (a + 1).to_string(),
            (Some(a), Some(b)) => format!("{}–{}", a + 1, b + 1),
            _ => "—".to_owned(),
        }
    }

    fn signal_table(&self) -> ReportSection {
        let title = "Points Outside the Limits";
        let signals = self.signals();
        if signals.is_empty() {
            return section(
                SectionKind::MultivariateSignals,
                title,
                SectionContent::Note("None: every point lies within its control limits.".to_owned()),
            );
        }
        let with_contrib = !matches!(self.chart, Chart::Variance(_));
        let mut header = vec!["Point", "Rows", "Phase", self.statistic_label(), "Limit crossed"];
        if with_contrib {
            header.push("Largest contribution");
        }
        let mut t = TextTable::new(header);
        for i in signals {
            let p = self.points()[i];
            let mut row = vec![
                self.point_name(i),
                self.rows_text(i),
                match p.phase {
                    ChartPhase::Reference => "reference",
                    ChartPhase::Monitoring => "monitoring",
                }
                .to_owned(),
                format_number(p.statistic, DIGITS),
                if p.statistic > p.upper {
                    format!("upper ({})", format_number(p.upper, DIGITS))
                } else {
                    format!("lower ({})", format_number(p.lower, DIGITS))
                },
            ];
            if with_contrib {
                let top = self.contributions(i).and_then(|c| {
                    c.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(j, _)| self.names[j].clone())
                });
                row.push(top.unwrap_or_else(|| "—".to_owned()));
            }
            t.push(row);
        }
        section(SectionKind::MultivariateSignals, title, SectionContent::Table(t))
    }

    /// The χ²(1) upper-α point that contributions are compared with (α of
    /// the request, as for the T² limits).
    fn contribution_guide(&self) -> f64 {
        ChiSquared::new(1.0).and_then(|d| d.quantile(1.0 - self.request.alpha)).unwrap_or(f64::NAN)
    }

    fn contribution_sections(&self) -> Vec<ReportSection> {
        let table_title = "Contributions to Out-of-Control Points";
        let signals = self.signals();
        if signals.is_empty() {
            return vec![section(
                SectionKind::MultivariateContributions,
                table_title,
                SectionContent::Note(
                    "No point is outside the limits, so there are no contributions to show.".to_owned(),
                ),
            )];
        }
        let guide = self.contribution_guide();
        let mut header = vec!["Point".to_owned(), self.statistic_label().to_owned()];
        header.extend(self.names.iter().cloned());
        let mut t = TextTable::new(header);
        let mut computed: Vec<(usize, Vec<f64>)> = Vec::new();
        for &i in &signals {
            let Some(c) = self.contributions(i) else { continue };
            let mut row = vec![self.point_name(i), format_number(self.points()[i].statistic, DIGITS)];
            row.extend(c.iter().map(|v| {
                let text = format_number(*v, 4);
                if *v > guide { format!("{text} ▲") } else { text }
            }));
            t.push(row);
            computed.push((i, c));
        }
        let note = format!(
            "Each value is the drop in {} when that variable is left out (Runger, Alt & Montgomery 1996). \
             ▲ marks values above {}, the χ² value with 1 degree of freedom exceeded with probability α = {}; \
             such a variable is a likely driver of the signal.",
            self.statistic_label(),
            format_number(guide, 4),
            format_number(self.request.alpha, 4),
        );
        // The plot: the largest signals, one profile each across the variables.
        computed.sort_by(|a, b| self.points()[b.0].statistic.total_cmp(&self.points()[a.0].statistic));
        computed.truncate(CONTRIBUTION_PLOT_POINTS);
        computed.sort_by_key(|c| c.0);
        let p = self.names.len();
        let mut points = Vec::new();
        let mut membership = Vec::new();
        let mut lines = Vec::new();
        let mut labels = Vec::new();
        for (g, (i, c)) in computed.iter().enumerate() {
            let row = self.point_rows[*i][0];
            let line: Vec<[f64; 2]> = c.iter().enumerate().map(|(j, v)| [(j + 1) as f64, *v]).collect();
            for &[x, y] in &line {
                points.push(RowPoint { row, x, y });
                membership.push(g);
            }
            lines.push(line);
            labels.push(format!("Point {}", self.point_name(*i)));
        }
        let right = p as f64 + 0.5;
        let plot = PlotSpec {
            x_label: "Variable".to_owned(),
            y_label: format!("Contribution to {}", self.statistic_label()),
            points,
            groups: Some(PointGroups { title: "Point".to_owned(), labels, membership, lines }),
            x_ticks: Some(self.names.iter().enumerate().map(|(j, n)| ((j + 1) as f64, n.clone())).collect()),
            segments: if guide.is_finite() { vec![vec![[0.5, guide], [right, guide]]] } else { Vec::new() },
            annotations: guide.is_finite().then(|| Annotations {
                groups: Vec::new(),
                items: Vec::new(),
                labels: vec![(right, guide, "χ²₁ guide".to_owned())],
            }),
            ..PlotSpec::default()
        };
        let plot_title = if signals.len() > CONTRIBUTION_PLOT_POINTS {
            format!("Contribution Plot (the {CONTRIBUTION_PLOT_POINTS} largest signals)")
        } else {
            "Contribution Plot".to_owned()
        };
        vec![
            section(SectionKind::MultivariateContributions, table_title, SectionContent::Table(t)),
            section(SectionKind::MultivariateContributionNote, "About Contributions", SectionContent::Note(note)),
            section(SectionKind::MultivariateContributionPlot, plot_title, SectionContent::Plot(Box::new(plot))),
        ]
    }

    fn estimates(&self) -> ReportSection {
        let (mean, cov) = match &self.chart {
            Chart::T2(c) => (&c.mean, &c.covariance),
            Chart::Mewma(c) => (&c.mean, &c.covariance),
            Chart::Variance(c) => (&c.mean, &c.covariance),
        };
        let mut header = vec!["Variable".to_owned(), "Mean".to_owned()];
        header.extend(self.names.iter().map(|n| format!("Cov. with {n}")));
        let mut t = TextTable::new(header);
        for (j, name) in self.names.iter().enumerate() {
            let mut row = vec![name.clone(), format_number(mean[j], DIGITS)];
            row.extend(cov[j].iter().map(|v| format_number(*v, DIGITS)));
            t.push(row);
        }
        let title = match self.chart {
            Chart::T2(ref c) if c.subgroup_size > 1 => "Reference Grand Mean and Average Subgroup Covariance",
            Chart::Variance(_) => "Reference Grand Mean and Average Subgroup Covariance",
            _ => "Reference Mean and Covariance",
        };
        section(SectionKind::MultivariateEstimates, title, SectionContent::Table(t))
    }
}

/// The subgroup size, or its range (`"4–5"`) when the subgroups differ.
fn subgroup_size_text(point_rows: &[Vec<usize>]) -> String {
    let smallest = point_rows.iter().map(Vec::len).min().unwrap_or(1);
    let largest = point_rows.iter().map(Vec::len).max().unwrap_or(1);
    if smallest == largest { largest.to_string() } else { format!("{smallest}–{largest}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::bottle_filling;

    fn request(kind: MultivariateChartKind) -> MultivariateChartRequest {
        MultivariateChartRequest { reference: Some(20), ..MultivariateChartRequest::new(vec![1, 2, 3], kind) }
    }

    #[test]
    fn subgroups_by_id_or_size_agree_and_points_carry_rows() {
        let t = bottle_filling();
        let rows = RowStateManager::new(t.n_rows());
        let by_id = MultivariateChartRequest { subgroup: Some(0), ..request(MultivariateChartKind::HotellingT2) };
        let by_size = MultivariateChartRequest { subgroup_size: 4, ..request(MultivariateChartKind::HotellingT2) };
        let a = MultivariateChartAnalysis::run(&t, &rows, &by_id).unwrap();
        let b = MultivariateChartAnalysis::run(&t, &rows, &by_size).unwrap();
        assert_eq!(a.signals(), b.signals());
        assert_eq!(a.signal_rows(), [88, 89, 90, 91, 96, 97, 98, 99]);
        let s = a.sections();
        let SectionContent::ControlChart(c) = &s[1].content else { panic!("chart") };
        assert_eq!(c.points.len(), 25);
        assert_eq!(c.points[22].rows, [88, 89, 90, 91]);
        assert_eq!(c.points[22].label.as_deref(), Some("23"));
        assert_eq!(c.steps.len(), 2, "reference and monitoring limits");
        assert!(c.center.is_nan() && c.lower.is_nan());
        assert_eq!(c.phases.len(), 2);
        let plot = s.iter().find(|x| x.kind == SectionKind::MultivariateContributionPlot).unwrap();
        let SectionContent::Plot(p) = &plot.content else { panic!("contribution plot") };
        assert_eq!(p.points.len(), 2 * 3);
        let g = p.groups.as_ref().unwrap();
        assert!(g.key_label(0).starts_with("Point "), "{}", g.key_label(0));
        assert!(a.title().starts_with("Hotelling T² Chart of Fill volume"));
    }

    #[test]
    fn exclusion_and_missing_values() {
        let t = bottle_filling();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([0], true);
        let r = request(MultivariateChartKind::Mewma);
        let a = MultivariateChartAnalysis::run(&t, &rows, &r).unwrap();
        assert_eq!(a.points().len(), 99);
        assert!(a.is_stale(&RowStateManager::new(t.n_rows())));
        let SectionContent::Table(summary) = &a.sections()[0].content else { panic!() };
        assert!(summary.rows.iter().any(|row| row[0] == "Rows excluded" && row[1] == "1"));
        // Subgroups of four with one row excluded: the last run is incomplete.
        let size = MultivariateChartRequest { subgroup_size: 4, ..request(MultivariateChartKind::GeneralizedVariance) };
        let g = MultivariateChartAnalysis::run(&t, &rows, &size).unwrap();
        assert_eq!(g.points().len(), 24);
        assert_eq!(g.leftover, 3);
        // Equal subgroups print one size; unequal ones their range.
        let by_id = MultivariateChartRequest { subgroup: Some(0), ..request(MultivariateChartKind::HotellingT2) };
        let full = MultivariateChartAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &by_id).unwrap();
        let SectionContent::Table(summary) = &full.sections()[0].content else { panic!() };
        assert!(summary.rows.iter().any(|row| row[0] == "Subgroup size" && row[1] == "4"));
        assert_eq!(subgroup_size_text(&[vec![0; 5], vec![0; 5], vec![0; 4]]), "4–5");
    }

    #[test]
    fn errors() {
        let t = bottle_filling();
        let rows = RowStateManager::new(t.n_rows());
        let one = MultivariateChartRequest::new(vec![1], MultivariateChartKind::HotellingT2);
        assert!(MultivariateChartAnalysis::run(&t, &rows, &one).is_err());
        let gv = MultivariateChartRequest { subgroup_size: 1, ..request(MultivariateChartKind::GeneralizedVariance) };
        assert!(MultivariateChartAnalysis::run(&t, &rows, &gv).is_err(), "|S| needs subgroups");
        let missing = MultivariateChartRequest::new(vec![1, 9], MultivariateChartKind::HotellingT2);
        assert!(MultivariateChartAnalysis::run(&t, &rows, &missing).is_err());
        for k in MultivariateChartKind::ALL {
            assert_eq!(MultivariateChartKind::from_key(k.key()), Some(k));
        }
    }
}
