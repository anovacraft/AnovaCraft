//! Pareto chart of defect categories (Juran), bound to the row state
//! ([`stat_quality::pareto`]).
//!
//! Each row names a category; bars count rows, or add up a frequency column,
//! ranked largest first with a cumulative-percentage line. Categories below
//! a threshold share can be pooled into an "Other" bar, placed last. A
//! grouping column draws one Pareto chart per level side by side, each
//! ranked on its own (Juran's stratified Pareto). Every bar carries its
//! dataset rows, so clicking a bar selects them in every linked view.

use crate::format::format_number;
use crate::report::{PlotPane, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager, natural_order};
use stat_quality::{ParetoOptions, pareto_table};

/// Significant digits in report tables.
const DIGITS: usize = 6;

/// What to chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParetoRequest {
    /// Column naming each row's category (any type).
    pub category: usize,
    /// Numeric column of frequencies (`None`: each row counts once).
    pub frequency: Option<usize>,
    /// Column whose levels each get a panel (`None`: one chart).
    pub group: Option<usize>,
}

/// Display options of a Pareto chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParetoChartOptions {
    /// Pool categories whose share of their panel's total is below this
    /// percentage into an "Other" bar (0 keeps every category).
    pub other_percent: f64,
    /// Bar heights as percentages of the panel's total instead of counts.
    pub percent_scale: bool,
}

impl Default for ParetoChartOptions {
    fn default() -> Self {
        Self { other_percent: 0.0, percent_scale: false }
    }
}

/// One bar of a [`ParetoChartSpec`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParetoBar {
    /// Category name ("Other" for the pooled bar).
    pub label: String,
    /// Count or total frequency.
    pub value: f64,
    /// Dataset rows in the category.
    pub rows: Vec<usize>,
    /// Each row's weight (1, or its frequency), parallel to `rows`.
    pub weights: Vec<f64>,
    /// Share of the panel's total, 0 to 1.
    pub share: f64,
    /// Cumulative share up to and including this bar, 0 to 1.
    pub cumulative: f64,
    /// Categories pooled into this bar (empty unless it is "Other").
    pub pooled: Vec<String>,
}

impl ParetoBar {
    /// Weight of the selected rows in the bar.
    #[must_use]
    pub fn selected_value(&self, rows: &RowStateManager) -> f64 {
        self.rows
            .iter()
            .zip(&self.weights)
            .filter(|(r, _)| **r < rows.len() && rows.is_selected(**r))
            .map(|(_, w)| w)
            .sum()
    }
}

/// One panel (a level of the grouping column, or the whole data).
#[derive(Debug, Clone, PartialEq)]
pub struct ParetoPanel {
    /// Heading (empty without a grouping column).
    pub label: String,
    /// Total count or frequency.
    pub total: f64,
    /// Bars, largest first ("Other" last).
    pub bars: Vec<ParetoBar>,
}

/// Data for drawing a Pareto chart: bars at x = 1, 2, … within each panel,
/// panels side by side with a gap of one bar.
#[derive(Debug, Clone, PartialEq)]
pub struct ParetoChartSpec {
    /// The category column.
    pub category: String,
    /// Vertical axis label ("Count", "Percent of total", …).
    pub y_label: String,
    /// Bars show percentages of the panel's total.
    pub percent_scale: bool,
    /// Panels.
    pub panels: Vec<ParetoPanel>,
}

/// Width of a bar (bars sit one unit apart).
pub const PARETO_BAR_WIDTH: f64 = 0.8;

impl ParetoChartSpec {
    /// Horizontal position of each bar, per panel.
    #[must_use]
    pub fn bar_x(&self) -> Vec<Vec<f64>> {
        let mut offset = 0.0;
        self.panels
            .iter()
            .map(|p| {
                let xs = (0..p.bars.len()).map(|i| offset + i as f64 + 1.0).collect();
                offset += p.bars.len() as f64 + 1.0;
                xs
            })
            .collect()
    }

    /// Height of a bar: its value, or its percentage of the panel's total.
    #[must_use]
    pub fn height(&self, bar: &ParetoBar) -> f64 {
        if self.percent_scale { 100.0 * bar.share } else { bar.value }
    }

    /// Height of the cumulative line at a bar, on the bars' scale.
    #[must_use]
    pub fn cumulative_height(&self, panel: &ParetoPanel, bar: &ParetoBar) -> f64 {
        if self.percent_scale { 100.0 * bar.cumulative } else { bar.cumulative * panel.total }
    }

    /// Height of the selected rows' part of a bar.
    #[must_use]
    pub fn selected_height(&self, bar: &ParetoBar, rows: &RowStateManager) -> f64 {
        let v = bar.selected_value(rows);
        if bar.value > 0.0 { self.height(bar) * v / bar.value } else { 0.0 }
    }

    /// Labelled ticks: each bar's category.
    #[must_use]
    pub fn ticks(&self) -> Vec<(f64, String)> {
        self.bar_x()
            .iter()
            .zip(&self.panels)
            .flat_map(|(xs, p)| xs.iter().zip(&p.bars).map(|(&x, b)| (x, b.label.clone())))
            .collect()
    }

    /// Panes of a grouped chart (empty for one panel without a heading).
    #[must_use]
    pub fn panes(&self) -> Vec<PlotPane> {
        if self.panels.len() < 2 && self.panels.iter().all(|p| p.label.is_empty()) {
            return Vec::new();
        }
        self.bar_x()
            .iter()
            .zip(&self.panels)
            .map(|(xs, p)| PlotPane {
                label: p.label.clone(),
                x_min: xs.first().map_or(0.5, |x| x - 0.5),
                x_max: xs.last().map_or(0.5, |x| x + 0.5),
            })
            .collect()
    }

    /// The tallest point drawn (bar or cumulative line).
    #[must_use]
    pub fn top(&self) -> f64 {
        self.panels
            .iter()
            .flat_map(|p| p.bars.iter().map(move |b| self.height(b).max(self.cumulative_height(p, b))))
            .fold(0.0, f64::max)
    }

    /// Every bar with its position: `(panel, bar, x)`.
    pub fn placed(&self) -> impl Iterator<Item = (&ParetoPanel, &ParetoBar, f64)> {
        self.bar_x()
            .into_iter()
            .zip(&self.panels)
            .flat_map(|(xs, p)| p.bars.iter().zip(xs).map(move |(b, x)| (p, b, x)).collect::<Vec<_>>())
    }

    /// The bar under `(x, y)` (within its width, between 0 and its height,
    /// with `slack` added above so short bars stay easy to hit).
    #[must_use]
    pub fn bar_at(&self, x: f64, y: f64, slack: f64) -> Option<&ParetoBar> {
        self.placed()
            .find(|(_, b, bx)| (x - bx).abs() <= PARETO_BAR_WIDTH / 2.0 && y >= -slack && y <= self.height(b) + slack)
            .map(|(_, b, _)| b)
    }

    /// Rows of the bars that overlap the box `[x0, x1] × [y0, y1]`.
    #[must_use]
    pub fn rows_in_region(&self, x: (f64, f64), y: (f64, f64)) -> Vec<usize> {
        let (x0, x1) = (x.0.min(x.1), x.0.max(x.1));
        let (y0, y1) = (y.0.min(y.1), y.0.max(y.1));
        self.placed()
            .filter(|(_, b, bx)| {
                bx + PARETO_BAR_WIDTH / 2.0 >= x0
                    && bx - PARETO_BAR_WIDTH / 2.0 <= x1
                    && y1 >= 0.0
                    && y0 <= self.height(b)
            })
            .flat_map(|(_, b, _)| b.rows.iter().copied())
            .collect()
    }

    /// The bars as a table: (group,) category, count, percent, cumulative
    /// percent.
    #[must_use]
    pub fn table(&self) -> TextTable {
        let grouped = !self.panes().is_empty();
        let value_heading = if self.y_label.starts_with("Percent") { "Count".to_owned() } else { self.y_label.clone() };
        let mut columns = vec![self.category.clone(), value_heading, "Percent".to_owned(), "Cumulative %".to_owned()];
        if grouped {
            columns.insert(0, "Group".to_owned());
        }
        let mut t = TextTable::new(columns);
        for p in &self.panels {
            for b in &p.bars {
                let label = if b.pooled.is_empty() {
                    b.label.clone()
                } else {
                    format!("{} ({} categories)", b.label, b.pooled.len())
                };
                let mut row = vec![
                    label,
                    format_number(b.value, DIGITS),
                    format_number(100.0 * b.share, 4),
                    format_number(100.0 * b.cumulative, 4),
                ];
                if grouped {
                    row.insert(0, p.label.clone());
                }
                t.push(row);
            }
        }
        t
    }
}

/// One panel's observations before ranking: `(level, categories, weights, rows)`.
type PanelData = (String, Vec<String>, Vec<f64>, Vec<usize>);

/// A computed Pareto chart.
#[derive(Debug, Clone)]
pub struct ParetoAnalysis {
    request: ParetoRequest,
    category: String,
    frequency: Option<String>,
    group: Option<String>,
    /// Panels before pooling into "Other": `(label, labels, weights, rows)`.
    data: Vec<PanelData>,
    n_rows: usize,
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl ParetoAnalysis {
    /// Gather the categories of the rows that are not excluded and have a
    /// category (and frequency, and group).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a missing column, a non-numeric
    /// frequency column, a negative frequency, or no usable rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ParetoRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let category = column(request.category)?;
        let frequency = match request.frequency {
            Some(i) => {
                let c = column(i)?;
                let v =
                    c.as_numeric().ok_or_else(|| invalid(format!("frequency column {} must be numeric", c.name)))?;
                Some((c.name.clone(), v))
            }
            None => None,
        };
        let group = request.group.map(column).transpose()?;
        let mut n_missing = 0;
        let mut n_excluded = 0;
        let mut panels: Vec<PanelData> = Vec::new();
        for r in 0..table.n_rows() {
            let label = category.level_text(r);
            let weight = frequency.as_ref().map_or(1.0, |(_, v)| v[r]);
            let level = group.map(|g| g.level_text(r)).unwrap_or_default();
            if label.trim().is_empty() || weight.is_nan() || (group.is_some() && level.trim().is_empty()) {
                n_missing += 1;
                continue;
            }
            if states.is_excluded(r) {
                n_excluded += 1;
                continue;
            }
            if !weight.is_finite() || weight < 0.0 {
                return Err(invalid(format!("row {}: frequencies must be finite and not negative", r + 1)));
            }
            let k = match panels.iter().position(|p| p.0 == level) {
                Some(k) => k,
                None => {
                    panels.push((level, Vec::new(), Vec::new(), Vec::new()));
                    panels.len() - 1
                }
            };
            panels[k].1.push(label);
            panels[k].2.push(weight);
            panels[k].3.push(r);
        }
        if panels.is_empty() {
            return Err(invalid("no rows have a category (and frequency) to chart"));
        }
        panels.sort_by(|a, b| natural_order(&a.0, &b.0));
        // Ties between categories rank in natural order: present each
        // panel's observations sorted by category.
        for p in &mut panels {
            let mut order: Vec<usize> = (0..p.1.len()).collect();
            order.sort_by(|&a, &b| natural_order(&p.1[a], &p.1[b]));
            p.1 = order.iter().map(|&i| p.1[i].clone()).collect();
            p.2 = order.iter().map(|&i| p.2[i]).collect();
            p.3 = order.iter().map(|&i| p.3[i]).collect();
            if p.2.iter().sum::<f64>() <= 0.0 {
                let name = if p.0.is_empty() { "the data".to_owned() } else { p.0.clone() };
                return Err(invalid(format!("the frequencies of {name} add up to zero")));
            }
        }
        Ok(Self {
            request: *request,
            category: category.name.clone(),
            frequency: frequency.map(|(n, _)| n),
            group: group.map(|g| g.name.clone()),
            n_rows: panels.iter().map(|p| p.3.len()).sum(),
            data: panels,
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ParetoRequest {
        &self.request
    }

    /// Report title, e.g. `"Pareto Chart of Defect"`.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.group {
            Some(g) => format!("Pareto Chart of {} by {g}", self.category),
            None => format!("Pareto Chart of {}", self.category),
        }
    }

    /// The chart for `options`.
    ///
    /// # Errors
    /// Errors of [`pareto_table`] (an out-of-range threshold).
    pub fn chart(&self, options: &ParetoChartOptions) -> Result<ParetoChartSpec> {
        let pool = ParetoOptions { other_below: options.other_percent / 100.0 };
        let mut panels = Vec::new();
        for (level, labels, weights, rows) in &self.data {
            let t = pareto_table(labels, weights, pool)?;
            let bars = t
                .categories
                .into_iter()
                .map(|c| {
                    let mut members = c.members;
                    members.sort_unstable_by_key(|&i| rows[i]);
                    ParetoBar {
                        label: c.label,
                        value: c.total,
                        rows: members.iter().map(|&i| rows[i]).collect(),
                        weights: members.iter().map(|&i| weights[i]).collect(),
                        share: c.share,
                        cumulative: c.cumulative,
                        pooled: c.pooled,
                    }
                })
                .collect();
            let label = match &self.group {
                Some(g) => format!("{g}: {level}"),
                None => String::new(),
            };
            panels.push(ParetoPanel { label, total: t.total, bars });
        }
        let y_label = match (options.percent_scale, &self.frequency) {
            (true, _) => "Percent of total".to_owned(),
            (false, Some(f)) => f.clone(),
            (false, None) => "Count".to_owned(),
        };
        Ok(ParetoChartSpec { category: self.category.clone(), y_label, percent_scale: options.percent_scale, panels })
    }

    /// Report sections: the chart, a table of the bars and a summary.
    #[must_use]
    pub fn sections(&self, options: &ParetoChartOptions) -> Vec<ReportSection> {
        let spec = match self.chart(options) {
            Ok(spec) => spec,
            Err(e) => {
                return vec![section(SectionKind::ParetoChart, "Pareto Chart", SectionContent::Note(e.to_string()))];
            }
        };
        let summary = self.summary(&spec, options);
        let table = spec.table();
        vec![
            section(SectionKind::ParetoChart, "Pareto Chart", SectionContent::Pareto(spec)),
            section(SectionKind::ParetoTable, "Categories", SectionContent::Table(table)),
            summary,
        ]
    }

    fn summary(&self, spec: &ParetoChartSpec, options: &ParetoChartOptions) -> ReportSection {
        let what = if self.frequency.is_some() { "of the total frequency" } else { "of the rows" };
        let mut parts = Vec::new();
        for p in &spec.panels {
            let vital = p.bars.iter().position(|b| b.cumulative >= 0.8 - 1e-12).map_or(p.bars.len(), |i| i + 1);
            let named = p.bars.iter().filter(|b| b.pooled.is_empty()).count();
            let pooled: usize = p.bars.iter().map(|b| b.pooled.len()).sum();
            // The pooled bar stands for its k categories, in the count and the name.
            let lead: Vec<String> = p
                .bars
                .iter()
                .take(vital)
                .map(|b| match b.pooled.len() {
                    0 => b.label.clone(),
                    k => format!("{} ({k} categories)", b.label),
                })
                .collect();
            let vital: usize = p.bars.iter().take(vital).map(|b| b.pooled.len().max(1)).sum();
            let prefix = if p.label.is_empty() { String::new() } else { format!("{}: ", p.label) };
            let mut text = format!(
                "{prefix}the first {vital} of {} categories ({}) account for {}% {what}",
                named + pooled,
                lead.join(", "),
                format_number(100.0 * p.bars.get(lead.len().saturating_sub(1)).map_or(0.0, |b| b.cumulative), 3),
            );
            if pooled > 0 {
                text.push_str(&format!(
                    "; {pooled} categories below {}% each are pooled into \"Other\"",
                    format_number(options.other_percent, 3)
                ));
            }
            text.push('.');
            parts.push(text);
        }
        let mut note = parts.join("\n");
        let mut counts = format!("\n{} rows charted", self.n_rows);
        if self.n_excluded > 0 {
            counts.push_str(&format!(", {} excluded", self.n_excluded));
        }
        if self.n_missing > 0 {
            counts.push_str(&format!(", {} without a value", self.n_missing));
        }
        counts.push('.');
        note.push_str(&counts);
        section(SectionKind::ParetoSummary, "Summary", SectionContent::Note(note))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn defects() -> DataTable {
        let text = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        DataTable::new(
            "defects",
            vec![
                Column::text("Defect", text(&["Scratch", "Dent", "Stain", "Scratch", "Crack", "Burr", "Dent", ""])),
                Column::numeric("Count", vec![12.0, 9.0, 4.0, 10.0, 2.0, 1.0, 12.0, 3.0]),
                Column::text("Line", text(&["A", "A", "B", "B", "A", "B", "B", "A"])),
            ],
        )
        .unwrap()
    }

    #[test]
    fn counts_frequencies_and_other() {
        let t = defects();
        let rows = RowStateManager::new(t.n_rows());
        let counted = ParetoAnalysis::run(&t, &rows, &ParetoRequest { category: 0, frequency: None, group: None })
            .unwrap()
            .chart(&ParetoChartOptions::default())
            .unwrap();
        let bars: Vec<(&str, f64)> = counted.panels[0].bars.iter().map(|b| (b.label.as_str(), b.value)).collect();
        // Two each of Dent and Scratch (natural order breaks the tie), then
        // single rows in natural order.
        assert_eq!(bars, [("Dent", 2.0), ("Scratch", 2.0), ("Burr", 1.0), ("Crack", 1.0), ("Stain", 1.0)]);
        let a =
            ParetoAnalysis::run(&t, &rows, &ParetoRequest { category: 0, frequency: Some(1), group: None }).unwrap();
        let spec = a.chart(&ParetoChartOptions { other_percent: 5.0, percent_scale: false }).unwrap();
        let bars: Vec<(&str, f64)> = spec.panels[0].bars.iter().map(|b| (b.label.as_str(), b.value)).collect();
        assert_eq!(bars, [("Scratch", 22.0), ("Dent", 21.0), ("Stain", 4.0), ("Other", 3.0)]);
        assert_eq!(spec.panels[0].bars[0].rows, [0, 3]);
        assert_eq!(spec.panels[0].bars[3].rows, [4, 5]);
        assert!((spec.cumulative_height(&spec.panels[0], &spec.panels[0].bars[1]) - 43.0).abs() < 1e-9);
        let percent = a.chart(&ParetoChartOptions { other_percent: 0.0, percent_scale: true }).unwrap();
        assert!((percent.height(&percent.panels[0].bars[0]) - 44.0).abs() < 1e-9);
        let table = spec.table();
        assert_eq!(table.cell(3, "Cumulative %"), Some("100"));
        let sections = a.sections(&ParetoChartOptions::default());
        let SectionContent::Note(n) = &sections[2].content else { panic!() };
        assert!(n.contains("first 2 of 5 categories (Scratch, Dent) account for 86%"), "{n}");
        assert!(n.contains("1 without a value"), "{n}");
    }

    #[test]
    fn summary_counts_the_categories_pooled_into_other() {
        // Ten categories: A 50, B 20 and eight of 3.75 each pooled below 5 %.
        // The 80 % point falls inside "Other", so all ten are counted.
        let mut names = vec!["A".to_owned(), "B".to_owned()];
        let mut counts = vec![50.0, 20.0];
        for k in 0..8 {
            names.push(format!("C{k}"));
            counts.push(3.75);
        }
        let t = DataTable::new("ten", vec![Column::text("Defect", names), Column::numeric("Count", counts)]).unwrap();
        let rows = RowStateManager::new(t.n_rows());
        let a =
            ParetoAnalysis::run(&t, &rows, &ParetoRequest { category: 0, frequency: Some(1), group: None }).unwrap();
        let sections = a.sections(&ParetoChartOptions { other_percent: 5.0, percent_scale: false });
        let SectionContent::Note(n) = &sections[2].content else { panic!() };
        assert!(n.contains("the first 10 of 10 categories (A, B, Other (8 categories)) account for 100%"), "{n}");
        assert!(n.contains("8 categories below 5% each"), "{n}");
    }

    #[test]
    fn panels_by_group_and_brushing_helpers() {
        let t = defects();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([2], true);
        let a =
            ParetoAnalysis::run(&t, &rows, &ParetoRequest { category: 0, frequency: Some(1), group: Some(2) }).unwrap();
        assert_eq!(a.title(), "Pareto Chart of Defect by Line");
        let spec = a.chart(&ParetoChartOptions::default()).unwrap();
        assert_eq!(spec.panels.len(), 2);
        assert_eq!(spec.panels[0].label, "Line: A");
        let a_bars: Vec<(&str, f64)> = spec.panels[0].bars.iter().map(|b| (b.label.as_str(), b.value)).collect();
        assert_eq!(a_bars, [("Scratch", 12.0), ("Dent", 9.0), ("Crack", 2.0)]);
        let b_bars: Vec<(&str, f64)> = spec.panels[1].bars.iter().map(|b| (b.label.as_str(), b.value)).collect();
        assert_eq!(b_bars, [("Dent", 12.0), ("Scratch", 10.0), ("Burr", 1.0)], "the excluded Stain row is left out");
        // Panel B starts after panel A's three bars and a gap.
        assert_eq!(spec.bar_x(), [vec![1.0, 2.0, 3.0], vec![5.0, 6.0, 7.0]]);
        assert_eq!(spec.panes().len(), 2);
        assert_eq!(spec.bar_at(5.1, 3.0, 0.0).map(|b| b.rows.clone()), Some(vec![6]));
        assert!(spec.bar_at(4.0, 1.0, 0.0).is_none());
        assert_eq!(spec.rows_in_region((0.5, 2.2), (0.0, 1.0)), [0, 1]);
        rows.select([0], stat_data::SelectionMode::Replace);
        assert!((spec.selected_height(&spec.panels[0].bars[0], &rows) - 12.0).abs() < 1e-12);
        assert_eq!(spec.table().columns[0], "Group");
    }

    #[test]
    fn bad_requests_are_refused() {
        let t = defects();
        let rows = RowStateManager::new(t.n_rows());
        let run = |request| ParetoAnalysis::run(&t, &rows, &request);
        assert!(run(ParetoRequest { category: 9, frequency: None, group: None }).is_err());
        assert!(run(ParetoRequest { category: 1, frequency: Some(2), group: None }).is_err(), "text frequencies");
        let negative = DataTable::new(
            "n",
            vec![Column::text("Defect", vec!["a".to_owned()]), Column::numeric("Count", vec![-1.0])],
        )
        .unwrap();
        let rows = RowStateManager::new(1);
        assert!(
            ParetoAnalysis::run(&negative, &rows, &ParetoRequest { category: 0, frequency: Some(1), group: None })
                .is_err()
        );
    }

    /// The exported chart's key names the bars and the cumulative line, with
    /// no generic "Fit" or "Rows" entries.
    #[test]
    fn figure_key_names_bars_and_cumulative_line() {
        let t = defects();
        let rows = RowStateManager::new(t.n_rows());
        let a =
            ParetoAnalysis::run(&t, &rows, &ParetoRequest { category: 0, frequency: Some(1), group: None }).unwrap();
        let s = a.sections(&ParetoChartOptions::default());
        let svg = crate::report_figure::section_figure(&s[0], None, stat_viz::export::VectorFormat::Svg).unwrap();
        let svg = String::from_utf8(svg).unwrap();
        assert!(svg.contains("Cumulative %"));
        assert!(!svg.contains(">Fit<") && !svg.contains(">Rows<"));
    }
}
