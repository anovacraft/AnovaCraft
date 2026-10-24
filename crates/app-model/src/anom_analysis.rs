//! Analysis of means (ANOM) bound to the row state
//! ([`stat_inference::anom`]): group means, proportions or rates against
//! decision limits around the overall value, for one grouping column or (for
//! means) a balanced two-way layout.
//!
//! Rows that are excluded or miss the response, size or a group value are
//! left out. Means: each group's rows. Proportions and rates: the event
//! counts (and trials or exposure, from a column or a constant per row) are
//! summed over each group's rows; without a group column every row is its
//! own group. Each charted point carries the dataset rows behind it, so
//! brushing a point selects its group.

use crate::attribute_chart::SubgroupSize;
use crate::counts::column_levels;
use crate::format::format_number;
use crate::report::{ChartPoint, ControlChartSpec, LimitStep, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager};
use stat_inference::anom::{AnomChart, AnomSide, AnomTwoWay, anom_means, anom_proportions, anom_rates, anom_two_way};

const DIGITS: usize = 5;

/// What kind of data is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomKind {
    /// Means of a continuous response (normal theory).
    Means,
    /// Binomial proportions: events out of trials.
    Proportions,
    /// Poisson rates: counts per unit of exposure.
    Rates,
}

impl AnomKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 3] = [Self::Means, Self::Proportions, Self::Rates];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Means => "Means (normal data)",
            Self::Proportions => "Proportions (binomial counts)",
            Self::Rates => "Rates (Poisson counts)",
        }
    }

    /// Stable key used in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Means => "means",
            Self::Proportions => "proportions",
            Self::Rates => "rates",
        }
    }

    /// The kind with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// What to analyse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnomRequest {
    /// Kind of data.
    pub kind: AnomKind,
    /// Response (means), event count (proportions) or count (rates) column.
    pub response: usize,
    /// Trials (proportions) or exposure (rates) per row; `None` for means.
    pub size: SubgroupSize,
    /// Grouping column (required for means).
    pub group: Option<usize>,
    /// Second grouping column for a two-way layout (means only).
    pub second: Option<usize>,
    /// Significance level α of the decision limits.
    pub alpha: f64,
}

impl AnomRequest {
    /// Default significance level.
    pub const DEFAULT_ALPHA: f64 = 0.05;

    /// One-way analysis of means of `response` by `group`.
    #[must_use]
    pub fn means(response: usize, group: usize) -> Self {
        Self {
            kind: AnomKind::Means,
            response,
            size: SubgroupSize::None,
            group: Some(group),
            second: None,
            alpha: Self::DEFAULT_ALPHA,
        }
    }

    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut c = vec![self.response];
        if let SubgroupSize::Column(s) = self.size {
            c.push(s);
        }
        c.extend(self.group);
        c.extend(self.second);
        c
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.response = f(self.response);
        if let SubgroupSize::Column(s) = &mut self.size {
            *s = f(*s);
        }
        self.group = self.group.map(f);
        self.second = self.second.map(f);
    }
}

/// One chart's points: label and dataset rows of each group or cell.
#[derive(Debug, Clone)]
struct Panel {
    /// Factor name(s) for the heading and axis.
    factor: String,
    labels: Vec<String>,
    rows: Vec<Vec<usize>>,
    chart: AnomChart,
}

/// A computed analysis of means.
#[derive(Debug, Clone)]
pub struct AnomAnalysis {
    request: AnomRequest,
    response_name: String,
    /// One panel, or first factor, second factor and (with replication)
    /// interaction.
    panels: Vec<Panel>,
    two_way: Option<(usize, f64)>,
    /// Totals of the size column (trials or exposure), for the summary.
    total_size: f64,
    n_rows: usize,
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

impl AnomAnalysis {
    /// Compute the analysis.
    ///
    /// # Errors
    /// Missing or non-numeric columns, a means analysis without a group
    /// column, a second factor for proportions or rates, too few groups,
    /// unbalanced two-way data, and errors from [`stat_inference::anom`].
    pub fn run(table: &DataTable, states: &RowStateManager, request: &AnomRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let numeric = |i: usize| -> Result<&[f64]> {
            let c = column(i)?;
            c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))
        };
        let response_col = column(request.response)?;
        let response = numeric(request.response)?;
        let size_col = match request.size {
            SubgroupSize::Column(i) => Some(numeric(i)?),
            _ => None,
        };
        let group_col = request.group.map(column).transpose()?;
        let second_col = request.second.map(column).transpose()?;
        if request.kind == AnomKind::Means {
            if group_col.is_none() {
                return Err(invalid("choose a group column"));
            }
        } else {
            if request.second.is_some() {
                return Err(invalid("a two-way analysis of means is for means only"));
            }
            if request.size == SubgroupSize::None {
                return Err(invalid("give the trials or exposure (a column or a constant)"));
            }
        }
        let size_at = |r: usize| match request.size {
            SubgroupSize::Column(_) => size_col.map_or(f64::NAN, |s| s[r]),
            SubgroupSize::Constant(n) => n,
            SubgroupSize::None => 1.0,
        };
        let (mut n_excluded, mut n_missing) = (0, 0);
        let mut usable = Vec::new();
        for r in 0..table.n_rows() {
            let complete = response[r].is_finite()
                && size_at(r).is_finite()
                && group_col.is_none_or(|c| !c.level_text(r).is_empty())
                && second_col.is_none_or(|c| !c.level_text(r).is_empty());
            if !complete {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                usable.push(r);
            }
        }
        if usable.is_empty() {
            return Err(invalid("no usable rows"));
        }
        // Levels in natural order, keeping only those with usable rows.
        let levels_of = |col: usize, c: &stat_data::Column| -> Vec<String> {
            column_levels(table, col).into_iter().filter(|l| usable.iter().any(|&r| c.level_text(r) == *l)).collect()
        };
        let total_size: f64 = usable.iter().map(|&r| size_at(r)).sum();
        let mut two_way = None;
        let panels = match (request.kind, group_col, second_col) {
            (_, Some(g), Some(h)) => {
                let (gi, hi) = (request.group.unwrap_or(0), request.second.unwrap_or(0));
                if gi == hi {
                    return Err(invalid("the two group columns must differ"));
                }
                let (la, lb) = (levels_of(gi, g), levels_of(hi, h));
                let mut cell_rows = vec![vec![Vec::new(); lb.len()]; la.len()];
                for &r in &usable {
                    let i = la.iter().position(|l| *l == g.level_text(r)).unwrap_or(0);
                    let j = lb.iter().position(|l| *l == h.level_text(r)).unwrap_or(0);
                    cell_rows[i][j].push(r);
                }
                let cells: Vec<Vec<Vec<f64>>> = cell_rows
                    .iter()
                    .map(|row| row.iter().map(|rows| rows.iter().map(|&r| response[r]).collect()).collect())
                    .collect();
                let AnomTwoWay { first, second, interaction, replicates, mse } = anom_two_way(&cells, request.alpha)?;
                two_way = Some((replicates, mse));
                let row_rows: Vec<Vec<usize>> = cell_rows.iter().map(|row| row.concat()).collect();
                let col_rows: Vec<Vec<usize>> =
                    (0..lb.len()).map(|j| cell_rows.iter().flat_map(|row| row[j].iter().copied()).collect()).collect();
                let mut panels = vec![
                    Panel { factor: g.name.clone(), labels: la.clone(), rows: row_rows, chart: first },
                    Panel { factor: h.name.clone(), labels: lb.clone(), rows: col_rows, chart: second },
                ];
                if let Some(chart) = interaction {
                    let labels = la.iter().flat_map(|a| lb.iter().map(move |b| format!("{a} × {b}"))).collect();
                    let rows = cell_rows.into_iter().flatten().collect();
                    panels.push(Panel { factor: format!("{} × {}", g.name, h.name), labels, rows, chart });
                }
                panels
            }
            (_, Some(g), None) => {
                let gi = request.group.unwrap_or(0);
                let labels = levels_of(gi, g);
                let mut rows = vec![Vec::new(); labels.len()];
                for &r in &usable {
                    let k = labels.iter().position(|l| *l == g.level_text(r)).unwrap_or(0);
                    rows[k].push(r);
                }
                let chart = Self::one_way(request, &rows, response, &size_at)?;
                vec![Panel { factor: g.name.clone(), labels, rows, chart }]
            }
            (_, None, _) => {
                let rows: Vec<Vec<usize>> = usable.iter().map(|&r| vec![r]).collect();
                let labels = usable.iter().map(|r| format!("Row {}", r + 1)).collect();
                let chart = Self::one_way(request, &rows, response, &size_at)?;
                vec![Panel { factor: "Row".to_owned(), labels, rows, chart }]
            }
        };
        Ok(Self {
            request: *request,
            response_name: response_col.name.clone(),
            panels,
            two_way,
            total_size,
            n_rows: usable.len(),
            n_excluded,
            n_missing,
            exclusion_revision: states.exclusion_revision(),
        })
    }

    fn one_way(
        request: &AnomRequest,
        rows: &[Vec<usize>],
        response: &[f64],
        size_at: &dyn Fn(usize) -> f64,
    ) -> Result<AnomChart> {
        match request.kind {
            AnomKind::Means => {
                let groups: Vec<Vec<f64>> = rows.iter().map(|g| g.iter().map(|&r| response[r]).collect()).collect();
                anom_means(&groups, request.alpha)
            }
            AnomKind::Proportions | AnomKind::Rates => {
                let counts: Vec<f64> = rows.iter().map(|g| g.iter().map(|&r| response[r]).sum()).collect();
                let sizes: Vec<f64> = rows.iter().map(|g| g.iter().map(|&r| size_at(r)).sum()).collect();
                if request.kind == AnomKind::Proportions {
                    anom_proportions(&counts, &sizes, request.alpha)
                } else {
                    anom_rates(&counts, &sizes, request.alpha)
                }
            }
        }
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &AnomRequest {
        &self.request
    }

    /// `true` if row exclusion has changed since the analysis was computed.
    #[must_use]
    pub fn is_stale(&self, states: &RowStateManager) -> bool {
        states.exclusion_revision() != self.exclusion_revision
    }

    /// Report title, e.g. `"Analysis of Means: Yield by Machine"`.
    #[must_use]
    pub fn title(&self) -> String {
        let what = match self.request.kind {
            AnomKind::Means => "Analysis of Means",
            AnomKind::Proportions => "Analysis of Means for Proportions",
            AnomKind::Rates => "Analysis of Means for Rates",
        };
        let by: Vec<&str> = self.panels.iter().take(2).map(|p| p.factor.as_str()).collect();
        format!("{what}: {} by {}", self.response_name, by.join(" and "))
    }

    /// Labels of the groups outside the decision limits, per chart.
    #[must_use]
    pub fn outside(&self) -> Vec<Vec<String>> {
        self.panels
            .iter()
            .map(|p| {
                p.chart
                    .groups
                    .iter()
                    .zip(&p.labels)
                    .filter(|(g, _)| g.signal.is_some())
                    .map(|(_, l)| l.clone())
                    .collect()
            })
            .collect()
    }

    /// The critical value of each chart.
    #[must_use]
    pub fn critical_values(&self) -> Vec<f64> {
        self.panels.iter().map(|p| p.chart.critical_value).collect()
    }

    fn value_label(&self) -> String {
        match self.request.kind {
            AnomKind::Means => format!("Mean of {}", self.response_name),
            AnomKind::Proportions => format!("Proportion ({})", self.response_name),
            AnomKind::Rates => format!("Rate ({} per unit)", self.response_name),
        }
    }

    /// Report sections: summary, then a chart and a table for each factor
    /// (and the interaction), and any note on the normal approximation.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let mut out = vec![self.summary()];
        let kinds = [
            (SectionKind::AnomChart, SectionKind::AnomGroups),
            (SectionKind::AnomSecondChart, SectionKind::AnomSecondGroups),
            (SectionKind::AnomInteractionChart, SectionKind::AnomInteractionGroups),
        ];
        for (k, panel) in self.panels.iter().enumerate() {
            let interaction = k == 2;
            out.push(self.chart(panel, kinds[k].0, interaction));
            out.push(self.table(panel, kinds[k].1, interaction));
        }
        if let Some(note) = self.approximation_note() {
            out.push(section(SectionKind::AnomNote, "Normal Approximation", SectionContent::Note(note)));
        }
        out
    }

    fn summary(&self) -> ReportSection {
        let mut t = TextTable::new(["Statistic", "Value"]);
        let mut push = |k: &str, v: String| t.push([k.to_owned(), v]);
        let main = &self.panels[0].chart;
        push("Data", self.request.kind.label().to_owned());
        push("Rows used", self.n_rows.to_string());
        if self.n_excluded > 0 {
            push("Rows excluded", self.n_excluded.to_string());
        }
        if self.n_missing > 0 {
            push("Rows with a missing value", self.n_missing.to_string());
        }
        match self.request.kind {
            AnomKind::Means => {
                push("Overall mean", format_number(main.center, DIGITS));
                push("Root mean square error s", format_number(main.scale, DIGITS));
                push("Error degrees of freedom", format_number(main.df, DIGITS));
            }
            AnomKind::Proportions => {
                push("Total trials", format_number(self.total_size, DIGITS));
                push("Overall proportion", format_number(main.center, DIGITS));
            }
            AnomKind::Rates => {
                push("Total exposure", format_number(self.total_size, DIGITS));
                push("Overall rate", format_number(main.center, DIGITS));
            }
        }
        if let Some((r, _)) = self.two_way {
            push("Observations per cell", r.to_string());
        }
        push("Significance level α", format_number(main.alpha, DIGITS));
        for p in &self.panels {
            let name = if self.panels.len() > 1 {
                format!("Critical value h ({})", p.factor)
            } else {
                "Critical value h".to_owned()
            };
            push(&name, format_number(p.chart.critical_value, DIGITS));
        }
        let outside: usize = self.outside().iter().map(Vec::len).sum();
        push("Points outside the decision limits", outside.to_string());
        section(SectionKind::AnomSummary, "Summary", SectionContent::Table(t))
    }

    fn chart(&self, panel: &Panel, kind: SectionKind, interaction: bool) -> ReportSection {
        let c = &panel.chart;
        let points: Vec<ChartPoint> = c
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| ChartPoint {
                sample: i + 1,
                value: g.value,
                rows: panel.rows[i].clone(),
                rules: if g.signal.is_some() { vec![1] } else { Vec::new() },
                label: Some(panel.labels[i].clone()),
                note: None,
                signal: false,
            })
            .collect();
        let equal = c.groups.iter().all(|g| g.lower == c.groups[0].lower && g.upper == c.groups[0].upper);
        // Limits per group when they differ (unequal sizes), as short steps.
        let steps = if equal {
            Vec::new()
        } else {
            c.groups
                .iter()
                .enumerate()
                .map(|(i, g)| LimitStep {
                    from: i as f64 + 0.6,
                    to: i as f64 + 1.4,
                    center: c.center,
                    lower: g.lower,
                    upper: g.upper,
                    zone_sigma: 0.0,
                })
                .collect()
        };
        let y_label =
            if interaction { format!("{} interaction effect", self.response_name) } else { self.value_label() };
        let spec = ControlChartSpec {
            x_label: panel.factor.clone(),
            y_label,
            x_ticks: Some(panel.labels.iter().enumerate().map(|(i, l)| ((i + 1) as f64, l.clone())).collect()),
            flag_label: Some("Outside the decision limits".to_owned()),
            limit_label: Some("Decision limits".to_owned()),
            points,
            center: c.center,
            lower: c.groups[0].lower,
            upper: c.groups[0].upper,
            zone_sigma: 0.0,
            steps,
            phases: Vec::new(),
        };
        let title = if interaction {
            format!("Interaction Effects: {}", panel.factor)
        } else {
            format!("Analysis of Means Chart: {}", panel.factor)
        };
        section(kind, title, SectionContent::ControlChart(spec))
    }

    fn table(&self, panel: &Panel, kind: SectionKind, interaction: bool) -> ReportSection {
        let size_heading = match (self.request.kind, interaction) {
            (_, true) | (AnomKind::Means, _) => "N",
            (AnomKind::Proportions, _) => "Trials",
            (AnomKind::Rates, _) => "Exposure",
        };
        let value_heading = match (self.request.kind, interaction) {
            (_, true) => "Effect",
            (AnomKind::Means, _) => "Mean",
            (AnomKind::Proportions, _) => "Proportion",
            (AnomKind::Rates, _) => "Rate",
        };
        let mut t = TextTable::new([
            panel.factor.as_str(),
            size_heading,
            value_heading,
            "Lower decision limit",
            "Upper decision limit",
            "Outside",
        ]);
        for (g, label) in panel.chart.groups.iter().zip(&panel.labels) {
            t.push([
                label.clone(),
                format_number(g.size, DIGITS),
                format_number(g.value, DIGITS),
                format_number(g.lower, DIGITS),
                format_number(g.upper, DIGITS),
                match g.signal {
                    Some(AnomSide::Above) => "above",
                    Some(AnomSide::Below) => "below",
                    None => "—",
                }
                .to_owned(),
            ]);
        }
        let title = if interaction {
            format!("Interaction Effects Table: {}", panel.factor)
        } else {
            format!("Groups: {}", panel.factor)
        };
        section(kind, title, SectionContent::Table(t))
    }

    /// A caution when the normal approximation behind proportion or rate
    /// limits is doubtful (fewer than five expected events or non-events).
    fn approximation_note(&self) -> Option<String> {
        let c = &self.panels[0].chart;
        let small: Vec<&str> = c
            .groups
            .iter()
            .zip(&self.panels[0].labels)
            .filter(|(g, _)| match self.request.kind {
                AnomKind::Means => false,
                AnomKind::Proportions => g.size * c.center < 5.0 || g.size * (1.0 - c.center) < 5.0,
                AnomKind::Rates => g.size * c.center < 5.0,
            })
            .map(|(_, l)| l.as_str())
            .collect();
        if small.is_empty() {
            return None;
        }
        Some(format!(
            "The limits use the normal approximation, which is rough when fewer than five events (or non-events) \
             are expected in a group. Groups affected: {}.",
            small.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::heat_treatment;

    #[test]
    fn one_way_points_carry_group_rows() {
        let t = heat_treatment();
        let rows = RowStateManager::new(t.n_rows());
        let a = AnomAnalysis::run(&t, &rows, &AnomRequest::means(2, 0)).unwrap();
        assert_eq!(a.title(), "Analysis of Means: Hardness (HRC) by Furnace");
        let s = a.sections();
        let SectionContent::ControlChart(c) = &s[1].content else { panic!("chart") };
        assert_eq!(c.points.len(), 4);
        assert_eq!(c.points[0].rows, (0..15).collect::<Vec<_>>());
        assert_eq!(c.x_ticks.as_ref().unwrap()[2].1, "Furnace 3");
        assert!(c.steps.is_empty(), "equal group sizes share one pair of limits");
        let SectionContent::Table(groups) = &s[2].content else { panic!("table") };
        assert_eq!(groups.rows.len(), 4);
        // The limits from the engine's critical value and the reported s.
        let h = a.critical_values()[0];
        let SectionContent::Table(summary) = &s[0].content else { panic!() };
        let rmse: f64 = summary.rows.iter().find(|r| r[0] == "Root mean square error s").unwrap()[1].parse().unwrap();
        assert!((c.upper - (c.center + h * rmse * (3.0_f64 / 60.0).sqrt())).abs() < 1e-3);
    }

    #[test]
    fn exclusion_unequal_sizes_and_errors() {
        let t = heat_treatment();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([0, 1, 2], true);
        let a = AnomAnalysis::run(&t, &rows, &AnomRequest::means(2, 0)).unwrap();
        let SectionContent::ControlChart(c) = &a.sections()[1].content else { panic!() };
        assert_eq!(c.points[0].rows.len(), 12);
        assert_eq!(c.steps.len(), 4, "unequal sizes give each group its own limits");
        assert!(a.is_stale(&RowStateManager::new(t.n_rows())));
        // Unbalanced two-way data are refused.
        let two = AnomRequest { second: Some(1), ..AnomRequest::means(2, 0) };
        assert!(AnomAnalysis::run(&t, &rows, &two).is_err());
        // Proportions need sizes; means need a group; two-way is for means.
        let p = AnomRequest { kind: AnomKind::Proportions, ..AnomRequest::means(4, 0) };
        assert!(AnomAnalysis::run(&t, &rows, &p).is_err());
        let no_group = AnomRequest { group: None, ..AnomRequest::means(2, 0) };
        assert!(AnomAnalysis::run(&t, &rows, &no_group).is_err());
        let p2 = AnomRequest { second: Some(1), size: SubgroupSize::Column(3), ..p };
        assert!(AnomAnalysis::run(&t, &rows, &p2).is_err());
        for k in AnomKind::ALL {
            assert_eq!(AnomKind::from_key(k.key()), Some(k));
        }
    }

    #[test]
    fn rows_as_groups_and_approximation_note() {
        let t = heat_treatment();
        let rows = RowStateManager::new(t.n_rows());
        // Each lot its own group: few expected pits per lot.
        let r = AnomRequest {
            kind: AnomKind::Rates,
            response: 6,
            size: SubgroupSize::Column(5),
            group: None,
            second: None,
            alpha: 0.01,
        };
        let a = AnomAnalysis::run(&t, &rows, &r).unwrap();
        let s = a.sections();
        let SectionContent::ControlChart(c) = &s[1].content else { panic!() };
        assert_eq!(c.points.len(), 60);
        assert_eq!(c.points[5].rows, [5]);
        assert!(s.iter().any(|x| x.kind == SectionKind::AnomNote));
    }
}
