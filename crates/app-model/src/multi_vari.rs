//! Multi-vari chart (Seder 1950) bound to the row state
//! ([`stat_quality::multivari`]).
//!
//! The response is plotted against one to four factors, outermost first. Each
//! innermost cell (a combination of levels that occurs in the data) gets a
//! position along the horizontal axis; the outermost factor's levels become
//! side-by-side panes. Individual values are plotted at their cell, each
//! factor's level means are marked with their own colour and marker shape,
//! and the means are joined within each level of the factor above (the
//! outermost factor's means are joined across the panes).
//!
//! By default the values of each innermost cell are spread a little to the
//! left and right of the cell's position in their row order and joined by a
//! thin line, so each cell reads as a short run chart and a trend or a jump
//! within a cell shows (the chart as drawn by Seder 1950 and Snee 2001,
//! "Graphical analysis of process variation studies", *Journal of Quality
//! Technology* 33(4)). [`ReplicateLayout::Stacked`] draws them on one
//! vertical line at the cell instead. A ranges view
//! charts the range within each cell against 3-sigma limits, and a table
//! gives the spread of each factor's means (Seder's families of variation).
//!
//! Plotted values carry their rows and each cell's range carries its cell's
//! rows, so brushing either view selects rows everywhere.

use crate::format::format_number;
use crate::report::{
    Annotations, ChartPoint, ControlChartSpec, LimitStep, PlotPane, PlotSpec, ReportSection, RowPoint, SectionContent,
    SectionKind, TextTable,
};
use stat_core::{Result, StatError};
use stat_data::{DataTable, RowStateManager, natural_order};
use stat_quality::{MAX_MULTI_VARI_FACTORS, MultiVari, multi_vari};

/// Significant digits in report tables.
const DIGITS: usize = 5;

/// What to chart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiVariRequest {
    /// Numeric response column.
    pub response: usize,
    /// One to four factor columns, outermost first.
    pub factors: Vec<usize>,
}

/// How the individual values of a cell are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplicateLayout {
    /// Side by side in row order across ±[`REPLICATE_HALF_WIDTH`] of the
    /// cell's position, joined by a thin line (a short run chart per cell).
    #[default]
    InOrder,
    /// On one vertical line at the cell's position.
    Stacked,
}

impl ReplicateLayout {
    /// Both layouts, for menus.
    pub const ALL: [Self; 2] = [Self::InOrder, Self::Stacked];

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::InOrder => "in-order",
            Self::Stacked => "stacked",
        }
    }

    /// The layout with key `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.key() == key)
    }

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::InOrder => "In order (side by side)",
            Self::Stacked => "Stacked",
        }
    }
}

/// Half the width (in cell spacings) over which a cell's values are spread
/// in [`ReplicateLayout::InOrder`].
pub const REPLICATE_HALF_WIDTH: f64 = 0.3;

/// Display options of a multi-vari report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiVariOptions {
    /// Plot the individual values.
    pub show_values: bool,
    /// Show the ranges view.
    pub show_ranges: bool,
    /// Show the table of cell statistics.
    pub show_cells: bool,
    /// How the values within a cell are placed.
    pub replicates: ReplicateLayout,
}

impl Default for MultiVariOptions {
    fn default() -> Self {
        Self { show_values: true, show_ranges: true, show_cells: false, replicates: ReplicateLayout::InOrder }
    }
}

/// A computed multi-vari breakdown.
#[derive(Debug, Clone)]
pub struct MultiVariAnalysis {
    request: MultiVariRequest,
    response: String,
    factors: Vec<String>,
    /// Level names of each factor, in natural order (codes index them).
    levels: Vec<Vec<String>>,
    /// Dataset row of each observation.
    rows: Vec<usize>,
    values: Vec<f64>,
    result: MultiVari,
    /// Horizontal position of each cell, per depth.
    x: Vec<Vec<f64>>,
    n_excluded: usize,
    n_missing: usize,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn section(kind: SectionKind, title: impl Into<String>, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.into(), content }
}

impl MultiVariAnalysis {
    /// Break the response down by the factors over the rows that are not
    /// excluded and have every value.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for a missing or non-numeric response,
    /// no factors or more than four, a factor repeated or equal to the
    /// response, or no complete rows.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &MultiVariRequest) -> Result<Self> {
        let column = |i: usize| table.column(i).ok_or_else(|| invalid(format!("no column {i}")));
        let response = column(request.response)?;
        let y = response.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", response.name)))?;
        if request.factors.is_empty() || request.factors.len() > MAX_MULTI_VARI_FACTORS {
            return Err(invalid(format!("a multi-vari chart takes one to {MAX_MULTI_VARI_FACTORS} factors")));
        }
        let mut distinct = request.factors.clone();
        distinct.push(request.response);
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() != request.factors.len() + 1 {
            return Err(invalid("the factors must differ from each other and from the response"));
        }
        let factor_columns = request.factors.iter().map(|&i| column(i)).collect::<Result<Vec<_>>>()?;
        let (mut rows, mut n_excluded, mut n_missing) = (Vec::new(), 0, 0);
        for r in 0..table.n_rows() {
            if !y[r].is_finite() || factor_columns.iter().any(|c| c.level_text(r).trim().is_empty()) {
                n_missing += 1;
            } else if states.is_excluded(r) {
                n_excluded += 1;
            } else {
                rows.push(r);
            }
        }
        if rows.is_empty() {
            return Err(invalid("no rows have a response and every factor"));
        }
        let mut levels = Vec::new();
        let mut codes = Vec::new();
        for c in &factor_columns {
            let labels: Vec<String> = rows.iter().map(|&r| c.level_text(r).trim().to_owned()).collect();
            let mut names = labels.clone();
            names.sort_by(|a, b| natural_order(a, b));
            names.dedup();
            codes.push(labels.iter().map(|l| names.iter().position(|n| n == l).expect("a level")).collect());
            levels.push(names);
        }
        let values: Vec<f64> = rows.iter().map(|&r| y[r]).collect();
        let result = multi_vari(&values, &codes)?;
        let x = positions(&result);
        Ok(Self {
            request: request.clone(),
            response: response.name.clone(),
            factors: factor_columns.iter().map(|c| c.name.clone()).collect(),
            levels,
            rows,
            values,
            result,
            x,
            n_excluded,
            n_missing,
        })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &MultiVariRequest {
        &self.request
    }

    /// The breakdown.
    #[must_use]
    pub fn result(&self) -> &MultiVari {
        &self.result
    }

    /// Report title, e.g. `"Multi-Vari Chart of Diameter by Batch / Shaft"`.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Multi-Vari Chart of {} by {}", self.response, self.factors.join(" / "))
    }

    /// Level names along a cell's path, from factor `from`.
    fn path_text(&self, path: &[usize], from: usize) -> String {
        path.iter().enumerate().skip(from).map(|(d, &c)| self.levels[d][c].as_str()).collect::<Vec<_>>().join(" / ")
    }

    /// Report sections for `options`.
    #[must_use]
    pub fn sections(&self, options: &MultiVariOptions) -> Vec<ReportSection> {
        let mut out = vec![self.chart(options), self.variation()];
        if options.show_ranges {
            out.push(self.ranges());
        }
        if options.show_cells {
            out.push(self.cells());
        }
        out
    }

    fn chart(&self, options: &MultiVariOptions) -> ReportSection {
        let depth = self.factors.len();
        let inner = &self.result.levels[depth - 1];
        let mut points = Vec::new();
        let mut point_runs = Vec::new();
        if options.show_values {
            for (k, cell) in inner.iter().enumerate() {
                let x = self.x[depth - 1][k];
                let n = cell.members.len();
                let mut run = Vec::with_capacity(n);
                for (j, &i) in cell.members.iter().enumerate() {
                    let at = match options.replicates {
                        ReplicateLayout::InOrder if n > 1 => {
                            x + REPLICATE_HALF_WIDTH * (2.0 * j as f64 / (n - 1) as f64 - 1.0)
                        }
                        _ => x,
                    };
                    points.push(RowPoint { row: self.rows[i], x: at, y: self.values[i] });
                    run.push([at, self.values[i]]);
                }
                if options.replicates == ReplicateLayout::InOrder && n > 1 {
                    point_runs.push(run);
                }
            }
        }
        // Means of each factor, innermost first so they draw beneath the
        // larger markers of the outer factors.
        let mut groups = Vec::new();
        let mut items = Vec::new();
        let mut segments = Vec::new();
        for d in (0..depth).rev() {
            let g = groups.len();
            groups.push(format!("{} means", self.factors[d]));
            for (k, cell) in self.result.levels[d].iter().enumerate() {
                items.push((self.x[d][k], cell.mean, String::new(), g));
            }
            // Join the means within each cell of the factor above.
            let siblings: Vec<Vec<usize>> = if d == 0 {
                vec![(0..self.result.levels[0].len()).collect()]
            } else {
                self.result.levels[d - 1].iter().map(|p| p.children.clone()).collect()
            };
            for s in siblings.into_iter().filter(|s| s.len() >= 2) {
                segments.push(s.iter().map(|&k| [self.x[d][k], self.result.levels[d][k].mean]).collect());
            }
        }
        let x_ticks = inner
            .iter()
            .enumerate()
            .map(|(k, c)| (self.x[depth - 1][k], self.path_text(&c.path, usize::from(depth > 1))))
            .collect();
        let panes = if depth > 1 {
            self.result.levels[0]
                .iter()
                .map(|c| {
                    let xs: Vec<f64> =
                        descendants(&self.result, 0, c).into_iter().map(|k| self.x[depth - 1][k]).collect();
                    PlotPane {
                        label: format!("{}: {}", self.factors[0], self.levels[0][c.path[0]]),
                        x_min: xs.iter().copied().fold(f64::INFINITY, f64::min) - 0.5,
                        x_max: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 0.5,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let x_label = if depth > 1 { self.factors[1..].join(" / ") } else { self.factors[0].clone() };
        let spec = PlotSpec {
            x_label,
            y_label: self.response.clone(),
            points,
            x_ticks: Some(x_ticks),
            panes,
            annotations: Some(Annotations { groups, items, labels: Vec::new() }),
            segments,
            point_runs,
            // The values take the first style; the means start after it.
            annotation_style_offset: usize::from(options.show_values),
            point_name: Some(match options.replicates {
                ReplicateLayout::InOrder => "Values (in row order within each cell)".to_owned(),
                ReplicateLayout::Stacked => "Values".to_owned(),
            }),
            ..PlotSpec::default()
        };
        section(SectionKind::MultiVariChart, "Multi-Vari Chart", SectionContent::Plot(Box::new(spec)))
    }

    fn variation(&self) -> ReportSection {
        let mut t = TextTable::new(["Source", "Comparisons", "Average range of means", "Largest range of means"]);
        for v in self.result.factor_variation() {
            let source = if v.factor == 0 {
                format!("{} (between levels)", self.factors[0])
            } else {
                format!("{} within {}", self.factors[v.factor], self.factors[v.factor - 1])
            };
            let cell = |x: f64| if x.is_nan() { "—".to_owned() } else { format_number(x, DIGITS) };
            t.push([source, v.groups.to_string(), cell(v.mean_range), cell(v.max_range)]);
        }
        let within: Vec<f64> =
            self.result.cells().iter().filter(|c| c.n() >= 2).map(stat_quality::MultiVariCell::range).collect();
        let cell = |x: f64| if x.is_nan() { "—".to_owned() } else { format_number(x, DIGITS) };
        t.push([
            "Within cells (range of values)".to_owned(),
            within.len().to_string(),
            cell(self.result.mean_within_range()),
            cell(within.iter().copied().fold(f64::NAN, f64::max)),
        ]);
        t.push([
            "Pooled within-cell standard deviation".to_owned(),
            within.len().to_string(),
            cell(self.result.pooled_within_sd()),
            "—".to_owned(),
        ]);
        let mut counts =
            format!("Grand mean {}; {} rows", format_number(self.result.grand_mean, DIGITS), self.rows.len());
        if self.n_excluded > 0 {
            counts.push_str(&format!(", {} excluded", self.n_excluded));
        }
        if self.n_missing > 0 {
            counts.push_str(&format!(", {} with a missing value", self.n_missing));
        }
        t.push([counts, String::new(), String::new(), String::new()]);
        section(SectionKind::MultiVariVariation, "Variation by Factor", SectionContent::Table(t))
    }

    fn ranges(&self) -> ReportSection {
        let limits = self.result.cell_range_limits();
        let mut points = Vec::new();
        let mut steps = Vec::new();
        for (cell, limit) in self.result.cells().iter().zip(&limits) {
            let Some(limit) = limit else { continue };
            let sample = points.len() + 1;
            let beyond = limit.upper.is_finite() && cell.range() > limit.upper + 1e-12 * limit.upper.abs();
            points.push(ChartPoint {
                sample,
                value: cell.range(),
                rows: cell.members.iter().map(|&i| self.rows[i]).collect(),
                rules: if beyond { vec![1] } else { Vec::new() },
                label: Some(self.path_text(&cell.path, 0)),
                note: None,
                signal: false,
            });
            let x = sample as f64;
            steps.push(LimitStep {
                from: x - 0.5,
                to: x + 0.5,
                center: limit.center,
                lower: limit.lower,
                upper: limit.upper,
                zone_sigma: 0.0,
            });
        }
        if points.is_empty() {
            return section(
                SectionKind::MultiVariRanges,
                "Ranges View",
                SectionContent::Note("No cell has two or more values, so there are no ranges to chart.".to_owned()),
            );
        }
        let first = steps[0];
        let constant = steps.iter().all(|s| {
            let same = |a: f64, b: f64| (a - b).abs() <= 1e-12 * a.abs().max(1.0) || (a.is_nan() && b.is_nan());
            same(s.center, first.center) && same(s.lower, first.lower) && same(s.upper, first.upper)
        });
        let spec = ControlChartSpec {
            x_label: "Cell".to_owned(),
            x_ticks: None,
            flag_label: None,
            limit_label: None,
            y_label: format!("Range of {}", self.response),
            points,
            center: first.center,
            lower: first.lower,
            upper: first.upper,
            zone_sigma: 0.0,
            steps: if constant { Vec::new() } else { steps },
            phases: Vec::new(),
        };
        section(SectionKind::MultiVariRanges, "Ranges View", SectionContent::ControlChart(spec))
    }

    fn cells(&self) -> ReportSection {
        let mut columns: Vec<String> = self.factors.clone();
        columns.extend(["N", "Mean", "Min", "Max", "Range", "Std Dev"].map(String::from));
        let mut t = TextTable::new(columns);
        let num = |x: f64| if x.is_nan() { "—".to_owned() } else { format_number(x, DIGITS) };
        for cell in self.result.cells() {
            let mut row: Vec<String> = cell.path.iter().enumerate().map(|(d, &c)| self.levels[d][c].clone()).collect();
            row.extend([
                cell.n().to_string(),
                num(cell.mean),
                num(cell.min),
                num(cell.max),
                num(cell.range()),
                num(cell.sd),
            ]);
            t.push(row);
        }
        section(SectionKind::MultiVariCells, "Cell Summary", SectionContent::Table(t))
    }
}

/// Indices of the innermost cells under `cell` (at `depth`).
fn descendants(m: &MultiVari, depth: usize, cell: &stat_quality::MultiVariCell) -> Vec<usize> {
    let mut out = Vec::new();
    let last = m.levels.len() - 1;
    if depth == last {
        if let Ok(k) = m.levels[last].binary_search_by(|c| c.path.cmp(&cell.path)) {
            out.push(k);
        }
        return out;
    }
    for &k in &cell.children {
        out.extend(descendants(m, depth + 1, &m.levels[depth + 1][k]));
    }
    out
}

/// Horizontal position of every cell at every depth: innermost cells one
/// unit apart, with an extra gap of 1 between levels of the outermost factor
/// and of 0.5 between levels of the factors in between; each outer cell sits
/// at the mean position of its innermost cells.
fn positions(m: &MultiVari) -> Vec<Vec<f64>> {
    let depth = m.levels.len();
    let inner = &m.levels[depth - 1];
    let mut xs = Vec::with_capacity(inner.len());
    let mut x = 0.0;
    for (k, cell) in inner.iter().enumerate() {
        let gap = match k.checked_sub(1).map(|p| &inner[p].path) {
            None => 0.0,
            Some(prev) => match prev.iter().zip(&cell.path).position(|(a, b)| a != b) {
                Some(0) if depth > 1 => 1.0,
                Some(d) if d + 1 < depth => 0.5,
                _ => 0.0,
            },
        };
        x += 1.0 + gap;
        xs.push(x);
    }
    let mut out = vec![Vec::new(); depth];
    out[depth - 1] = xs;
    for d in (0..depth - 1).rev() {
        out[d] = m.levels[d]
            .iter()
            .map(|c| c.children.iter().map(|&k| out[d + 1][k]).sum::<f64>() / c.children.len() as f64)
            .collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn shafts() -> DataTable {
        let text = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        DataTable::new(
            "shafts",
            vec![
                Column::text("Batch", text(&["B1", "B1", "B1", "B1", "B1", "B1", "B2", "B2", "B2", "B2", "B2", "B2"])),
                Column::numeric("Part", vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0]),
                Column::text("Position", text(&["L", "M", "R", "L", "M", "R", "L", "M", "R", "L", "M", "R"])),
                Column::numeric(
                    "Diameter",
                    vec![10.0, 11.0, 12.0, 13.0, 14.0, 18.0, 20.0, 21.0, 25.0, 24.0, 26.0, 28.0],
                ),
            ],
        )
        .unwrap()
    }

    #[test]
    fn chart_places_cells_panes_and_connected_means() {
        let t = shafts();
        let rows = RowStateManager::new(t.n_rows());
        let a = MultiVariAnalysis::run(&t, &rows, &MultiVariRequest { response: 3, factors: vec![0, 1] }).unwrap();
        assert!(a.title().starts_with("Multi-Vari Chart of Diameter by "));
        let s = a.sections(&MultiVariOptions::default());
        let SectionContent::Plot(p) = &s[0].content else { panic!() };
        assert_eq!(p.points.len(), 12);
        // Parts at 1, 2 in batch B1 and (after a gap) 4, 5 in B2.
        let ticks: Vec<(f64, &str)> = p.x_ticks.as_ref().unwrap().iter().map(|(x, l)| (*x, l.as_str())).collect();
        assert_eq!(ticks, [(1.0, "1"), (2.0, "2"), (4.0, "1"), (5.0, "2")]);
        assert_eq!(p.panes.len(), 2);
        assert_eq!((p.panes[1].label.as_str(), p.panes[1].x_min, p.panes[1].x_max), ("Batch: B2", 3.5, 5.5));
        let notes = p.annotations.as_ref().unwrap();
        assert_eq!(notes.groups, ["Part means", "Batch means"]);
        // Part means 11, 15 | 22, 26 joined within batch; batch means 13, 24 at 1.5 and 4.5.
        assert_eq!(p.segments.len(), 3);
        assert_eq!(p.segments[0], [[1.0, 11.0], [2.0, 15.0]]);
        assert_eq!(p.segments[2], [[1.5, 13.0], [4.5, 24.0]]);
        // Variation by factor: batches 11 apart, parts 4 apart within batch.
        let SectionContent::Table(v) = &s[1].content else { panic!() };
        assert_eq!(v.cell(0, "Average range of means"), Some("11"));
        assert_eq!(v.cell(1, "Average range of means"), Some("4"));
        assert_eq!(v.cell(2, "Average range of means"), Some("4"));
        // Ranges view: four cells of three, R̄ = 4.
        let SectionContent::ControlChart(c) = &s[2].content else { panic!() };
        assert_eq!(c.points.len(), 4);
        assert!((c.center - 4.0).abs() < 1e-12 && c.steps.is_empty());
        assert_eq!(c.points[1].rows, [3, 4, 5]);
        assert_eq!(c.points[1].label.as_deref(), Some("B1 / 2"));
    }

    #[test]
    fn three_factors_options_and_exclusion() {
        let t = shafts();
        let mut rows = RowStateManager::new(t.n_rows());
        rows.set_excluded([11], true);
        let a = MultiVariAnalysis::run(&t, &rows, &MultiVariRequest { response: 3, factors: vec![0, 1, 2] }).unwrap();
        let options =
            MultiVariOptions { show_values: false, show_ranges: true, show_cells: true, ..MultiVariOptions::default() };
        let s = a.sections(&options);
        let SectionContent::Plot(p) = &s[0].content else { panic!() };
        assert!(p.points.is_empty());
        let ticks = p.x_ticks.as_ref().unwrap();
        assert_eq!(ticks.len(), 11);
        assert_eq!(ticks[3], (4.5, "2 / L".to_owned()), "half a unit between parts");
        assert_eq!(ticks[6], (8.5, "1 / L".to_owned()), "a unit between batches");
        // Single readings have no range.
        let SectionContent::Note(_) = &s[2].content else { panic!("no ranges with one reading per cell") };
        let SectionContent::Table(cells) = &s[3].content else { panic!() };
        assert_eq!(cells.rows.len(), 11);
        assert_eq!(cells.columns[..3], ["Batch", "Part", "Position"]);
    }

    /// Three batches × five parts × four readings in row order; reading j of
    /// part p in batch b is 100 b + 10 p + j, so every cell rises in order.
    fn readings() -> DataTable {
        let (mut batch, mut part, mut y) = (Vec::new(), Vec::new(), Vec::new());
        for b in 1..=3 {
            for p in 1..=5 {
                for j in 0..4 {
                    batch.push(format!("B{b}"));
                    part.push(f64::from(p));
                    y.push(f64::from(100 * b + 10 * p + j));
                }
            }
        }
        DataTable::new(
            "readings",
            vec![Column::text("Batch", batch), Column::numeric("Part", part), Column::numeric("y", y)],
        )
        .unwrap()
    }

    #[test]
    fn replicates_run_in_row_order_within_each_cell() {
        let t = readings();
        let rows = RowStateManager::new(t.n_rows());
        let a = MultiVariAnalysis::run(&t, &rows, &MultiVariRequest { response: 2, factors: vec![0, 1] }).unwrap();
        let s = a.sections(&MultiVariOptions::default());
        let SectionContent::Plot(p) = &s[0].content else { panic!() };
        assert_eq!(p.points.len(), 60);
        let ticks = p.x_ticks.as_ref().unwrap();
        assert_eq!(p.point_runs.len(), 15, "one run per cell");
        for (cell, (run, (centre, _))) in p.point_runs.iter().zip(ticks).enumerate() {
            assert_eq!(run.len(), 4);
            // Spread across ±0.3 of the cell, left to right in row order.
            let offsets: Vec<f64> = run.iter().map(|q| q[0] - centre).collect();
            for (o, want) in offsets.iter().zip([-0.3, -0.1, 0.1, 0.3]) {
                assert!((o - want).abs() < 1e-12, "{offsets:?}");
            }
            // The run joins the cell's rows in order: values rise by one.
            for (j, q) in run.iter().enumerate() {
                let (b, part) = (cell / 5 + 1, cell % 5 + 1);
                assert!((q[1] - (100 * b + 10 * part + j) as f64).abs() < 1e-12);
            }
            // The points carry the same positions and their own rows.
            for (j, q) in run.iter().enumerate() {
                let pt = &p.points[4 * cell + j];
                assert_eq!(pt.row, 4 * cell + j);
                assert!((pt.x - q[0]).abs() < 1e-12 && (pt.y - q[1]).abs() < 1e-12);
            }
        }
        // Cell means at the cell's position (mean of 0..3 is 1.5); batch means
        // at the batch centre, joined across the panes.
        let notes = p.annotations.as_ref().unwrap();
        assert_eq!(notes.groups, ["Part means", "Batch means"]);
        let part_means: Vec<_> = notes.items.iter().filter(|i| i.3 == 0).collect();
        for (k, m) in part_means.iter().enumerate() {
            assert!((m.0 - ticks[k].0).abs() < 1e-12);
            assert!((m.1 - (100 * (k / 5 + 1) + 10 * (k % 5 + 1)) as f64 - 1.5).abs() < 1e-12);
        }
        let batch_means: Vec<_> = notes.items.iter().filter(|i| i.3 == 1).collect();
        for (b, m) in batch_means.iter().enumerate() {
            assert!((m.1 - (100 * (b + 1) + 30) as f64 - 1.5).abs() < 1e-12);
        }
        assert_eq!(p.segments.len(), 4, "part means within each batch, batch means across");
        assert_eq!(p.segments[3].len(), 3);
        assert_eq!(p.point_label(), "Values (in row order within each cell)");
        // Stacked keeps the values on the cell's vertical, with no runs.
        let stacked = MultiVariOptions { replicates: ReplicateLayout::Stacked, ..MultiVariOptions::default() };
        let SectionContent::Plot(q) = &a.sections(&stacked)[0].content else { panic!() };
        assert!(q.point_runs.is_empty());
        for (k, (x, _)) in ticks.iter().enumerate() {
            assert!(q.points[4 * k..4 * k + 4].iter().all(|pt| pt.x == *x));
        }
        assert_eq!(ReplicateLayout::from_key("stacked"), Some(ReplicateLayout::Stacked));
        assert_eq!(ReplicateLayout::from_key(ReplicateLayout::InOrder.key()), Some(ReplicateLayout::InOrder));
    }

    #[test]
    fn bad_requests_are_refused() {
        let t = shafts();
        let rows = RowStateManager::new(t.n_rows());
        let run =
            |response, factors: Vec<usize>| MultiVariAnalysis::run(&t, &rows, &MultiVariRequest { response, factors });
        assert!(run(0, vec![1]).is_err(), "text response");
        assert!(run(3, vec![]).is_err());
        assert!(run(3, vec![0, 0]).is_err());
        assert!(run(3, vec![3]).is_err());
        assert!(run(3, vec![0, 1, 2, 0, 1]).is_err());
    }
}
