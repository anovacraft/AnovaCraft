//! The multi-way table report (Describe ▸ Multi-way table…): counts,
//! percentages and summary statistics of continuous columns, by several
//! nested row and column classifications ([`stat_discrete::tabulation`]).
//!
//! The report runs through [`crate::ComparisonReport`], so front ends edit
//! its settings in place.

use crate::format::format_number;
use crate::project::{invalid, obj, s};
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_discrete::tabulation::{
    Classification, GroupKey, Measure, MeasureSummary, MultiwayTable, TabulationInput, TabulationOptions, tabulate,
};
use stat_linear::natural_order;

/// A frequency statistic shown in every cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellFrequency {
    /// Number of rows (sum of the frequency column).
    Count,
    /// Percentage of the cell's row.
    RowPercent,
    /// Percentage of the cell's column.
    ColumnPercent,
    /// Percentage of all rows.
    TotalPercent,
}

impl CellFrequency {
    /// Every frequency statistic, in display order.
    pub const ALL: [Self; 4] = [Self::Count, Self::RowPercent, Self::ColumnPercent, Self::TotalPercent];

    /// Heading in the table.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Count => "Count",
            Self::RowPercent => "Row %",
            Self::ColumnPercent => "Column %",
            Self::TotalPercent => "Total %",
        }
    }

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::RowPercent => "row-percent",
            Self::ColumnPercent => "column-percent",
            Self::TotalPercent => "total-percent",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// A summary statistic of a continuous column shown in every cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellSummary {
    /// Number of non-missing values.
    N,
    /// Mean.
    Mean,
    /// Standard deviation.
    StdDev,
    /// Median.
    Median,
    /// Smallest value.
    Min,
    /// Largest value.
    Max,
    /// Sum.
    Sum,
}

impl CellSummary {
    /// Every summary statistic, in display order.
    pub const ALL: [Self; 7] = [Self::N, Self::Mean, Self::StdDev, Self::Median, Self::Min, Self::Max, Self::Sum];

    /// Name in headings.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::N => "N",
            Self::Mean => "Mean",
            Self::StdDev => "SD",
            Self::Median => "Median",
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Sum => "Sum",
        }
    }

    /// Key in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::N => "n",
            Self::Mean => "mean",
            Self::StdDev => "sd",
            Self::Median => "median",
            Self::Min => "min",
            Self::Max => "max",
            Self::Sum => "sum",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    fn of(self, m: &MeasureSummary) -> f64 {
        match self {
            Self::N => m.n,
            Self::Mean => m.mean,
            Self::StdDev => m.std_dev,
            Self::Median => m.median,
            Self::Min => m.min,
            Self::Max => m.max,
            Self::Sum => m.sum,
        }
    }
}

/// Settings of a multi-way table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiwayTableRequest {
    /// Row classification columns, outermost first.
    pub rows: Vec<usize>,
    /// Column classification columns, outermost first.
    pub columns: Vec<usize>,
    /// Frequency statistics shown in each cell, in order.
    pub frequencies: Vec<CellFrequency>,
    /// Continuous columns summarized in each cell.
    pub measures: Vec<usize>,
    /// Statistics of each continuous column, in order.
    pub statistics: Vec<CellSummary>,
    /// Frequency (count) column: each row stands for this many observations.
    pub frequency: Option<usize>,
    /// Add a grand-total row and column.
    pub margins: bool,
    /// Add a subtotal for each outer level over its inner levels.
    pub subtotals: bool,
}

impl MultiwayTableRequest {
    /// Counts by the given rows and columns, with margins.
    #[must_use]
    pub fn new(rows: Vec<usize>, columns: Vec<usize>) -> Self {
        Self {
            rows,
            columns,
            frequencies: vec![CellFrequency::Count],
            measures: Vec::new(),
            statistics: vec![CellSummary::Mean],
            frequency: None,
            margins: true,
            subtotals: false,
        }
    }

    /// Every column used.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        self.rows.iter().chain(&self.columns).chain(&self.measures).copied().chain(self.frequency).collect()
    }

    /// The same request with columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(mut self, f: impl Fn(usize) -> usize) -> Self {
        for c in self.rows.iter_mut().chain(self.columns.iter_mut()).chain(self.measures.iter_mut()) {
            *c = f(*c);
        }
        self.frequency = self.frequency.map(&f);
        self
    }

    /// The project-file form (`"analysis": "multiway-table"`).
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let names = |cols: &[usize]| {
            JsonValue::Array(cols.iter().map(|&i| s(table.column(i).map_or("", |c| c.name.as_str()))).collect())
        };
        obj([
            ("analysis", s("multiway-table")),
            ("rows", names(&self.rows)),
            ("columns", names(&self.columns)),
            ("frequencies", JsonValue::Array(self.frequencies.iter().map(|f| s(f.key())).collect())),
            ("measures", names(&self.measures)),
            ("statistics", JsonValue::Array(self.statistics.iter().map(|f| s(f.key())).collect())),
            (
                "frequency",
                self.frequency.map_or(JsonValue::Null, |i| s(table.column(i).map_or("", |c| c.name.as_str()))),
            ),
            ("margins", JsonValue::Bool(self.margins)),
            ("subtotals", JsonValue::Bool(self.subtotals)),
        ])
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns, or unknown statistic keys.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let cols = |key: &str| -> Result<Vec<usize>> {
            let Some(list) = v.get(key) else { return Ok(Vec::new()) };
            list.as_array()
                .ok_or_else(|| invalid(format!("{key}: expected a list of column names")))?
                .iter()
                .map(|n| {
                    let name = n.as_str().ok_or_else(|| invalid(format!("{key}: expected column names")))?;
                    table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
                })
                .collect()
        };
        let keys = |key: &str| -> Vec<String> {
            v.get(key)
                .and_then(JsonValue::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default()
        };
        let frequencies = match v.get("frequencies") {
            Some(_) => keys("frequencies")
                .iter()
                .map(|k| {
                    CellFrequency::from_key(k).ok_or_else(|| invalid(format!("unknown frequency statistic {k:?}")))
                })
                .collect::<Result<Vec<_>>>()?,
            None => vec![CellFrequency::Count],
        };
        let statistics = match v.get("statistics") {
            Some(_) => keys("statistics")
                .iter()
                .map(|k| CellSummary::from_key(k).ok_or_else(|| invalid(format!("unknown summary statistic {k:?}"))))
                .collect::<Result<Vec<_>>>()?,
            None => vec![CellSummary::Mean],
        };
        let frequency = match v.get("frequency") {
            None | Some(JsonValue::Null) => None,
            Some(n) => {
                let name = n.as_str().ok_or_else(|| invalid("frequency: expected a column name"))?;
                Some(table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))?)
            }
        };
        Ok(Self {
            rows: cols("rows")?,
            columns: cols("columns")?,
            frequencies,
            measures: cols("measures")?,
            statistics,
            frequency,
            margins: v.get("margins").and_then(JsonValue::as_bool).unwrap_or(true),
            subtotals: v.get("subtotals").and_then(JsonValue::as_bool).unwrap_or(false),
        })
    }
}

/// One statistic shown in each cell.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Item {
    Frequency(CellFrequency),
    Summary(usize, CellSummary),
}

/// A computed multi-way table.
#[derive(Debug, Clone)]
pub struct MultiwayTableAnalysis {
    request: MultiwayTableRequest,
    row_names: Vec<String>,
    column_names: Vec<String>,
    row_levels: Vec<Vec<String>>,
    column_levels: Vec<Vec<String>>,
    measure_names: Vec<String>,
    frequency_name: Option<String>,
    table: MultiwayTable,
    excluded: usize,
}

impl MultiwayTableAnalysis {
    /// Tabulate the non-excluded rows.
    ///
    /// # Errors
    /// No classification column, a column in two roles, a continuous or
    /// frequency column that is not numeric, nothing to show in the cells,
    /// or no row with every classification present.
    pub fn run(table: &DataTable, states: &RowStateManager, request: &MultiwayTableRequest) -> Result<Self> {
        let used = request.columns();
        if request.rows.is_empty() && request.columns.is_empty() {
            return Err(invalid("choose at least one row or column classification"));
        }
        if (1..used.len()).any(|i| used[..i].contains(&used[i])) {
            return Err(invalid("a column can have only one role in the table"));
        }
        let shows_measures = !request.measures.is_empty() && !request.statistics.is_empty();
        if request.frequencies.is_empty() && !shows_measures {
            return Err(invalid("choose a count or percentage, or a continuous column and a statistic, to show"));
        }
        let name = |i: usize| -> Result<String> {
            table.column(i).map(|c| c.name.clone()).ok_or_else(|| invalid(format!("no column {i}")))
        };
        let numeric = |i: usize| -> Result<&[f64]> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            c.as_numeric().ok_or_else(|| invalid(format!("{} must be a numeric column", c.name)))
        };
        let rows: Vec<usize> = (0..table.n_rows()).filter(|&r| !states.is_excluded(r)).collect();
        let excluded = table.n_rows() - rows.len();
        let classify = |i: usize| -> Result<(Classification, Vec<String>)> {
            let col = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            // Levels of the counted rows only: an excluded row's level is not listed.
            let mut levels: Vec<String> = rows.iter().map(|&r| col.level_text(r)).filter(|t| !t.is_empty()).collect();
            levels.sort_by(|a, b| natural_order(a, b));
            levels.dedup();
            let codes = rows
                .iter()
                .map(|&r| {
                    let t = col.level_text(r);
                    if t.is_empty() { None } else { levels.iter().position(|l| *l == t) }
                })
                .collect();
            Ok((Classification { name: name(i)?, levels: levels.clone(), codes }, levels))
        };
        let (mut row_cls, mut row_levels) = (Vec::new(), Vec::new());
        for &i in &request.rows {
            let (c, l) = classify(i)?;
            row_cls.push(c);
            row_levels.push(l);
        }
        let (mut col_cls, mut column_levels) = (Vec::new(), Vec::new());
        for &i in &request.columns {
            let (c, l) = classify(i)?;
            col_cls.push(c);
            column_levels.push(l);
        }
        let mut measures = Vec::new();
        for &i in &request.measures {
            let v = numeric(i)?;
            measures.push(Measure { name: name(i)?, values: rows.iter().map(|&r| v[r]).collect() });
        }
        let weights = match request.frequency {
            Some(i) => {
                let v = numeric(i)?;
                Some(rows.iter().map(|&r| v[r]).collect())
            }
            None => None,
        };
        let input = TabulationInput { rows: row_cls, columns: col_cls, measures, weights };
        let result = tabulate(&input, TabulationOptions { margins: request.margins, subtotals: request.subtotals })?;
        Ok(Self {
            request: request.clone(),
            row_names: input.rows.iter().map(|c| c.name.clone()).collect(),
            column_names: input.columns.iter().map(|c| c.name.clone()).collect(),
            row_levels,
            column_levels,
            measure_names: input.measures.iter().map(|m| m.name.clone()).collect(),
            frequency_name: request.frequency.map(name).transpose()?,
            table: result,
            excluded,
        })
    }

    /// The computed table.
    #[must_use]
    pub fn table(&self) -> &MultiwayTable {
        &self.table
    }

    /// The request this analysis answers.
    #[must_use]
    pub fn request(&self) -> &MultiwayTableRequest {
        &self.request
    }

    /// Report title, e.g. "Multi-way Table: Centre, Treatment by Outcome".
    #[must_use]
    pub fn title(&self) -> String {
        let rows = self.row_names.join(", ");
        let cols = self.column_names.join(", ");
        match (rows.is_empty(), cols.is_empty()) {
            (false, false) => format!("Multi-way Table: {rows} by {cols}"),
            (false, true) => format!("Multi-way Table: {rows}"),
            _ => format!("Multi-way Table: {cols}"),
        }
    }

    fn items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self.request.frequencies.iter().map(|&f| Item::Frequency(f)).collect();
        for m in 0..self.measure_names.len() {
            items.extend(self.request.statistics.iter().map(|&s| Item::Summary(m, s)));
        }
        items
    }

    fn item_label(&self, item: Item) -> String {
        match item {
            Item::Frequency(f) => f.label().to_owned(),
            Item::Summary(m, s) => format!("{} {}", s.label(), self.measure_names[m]),
        }
    }

    /// Labels of a group: each level, "All" where the group spans every
    /// level (only the first such position is labelled).
    fn group_labels(key: &GroupKey, levels: &[Vec<String>]) -> Vec<String> {
        let mut seen_all = false;
        key.0
            .iter()
            .enumerate()
            .map(|(d, k)| match k {
                Some(c) => levels[d][*c].clone(),
                None if !seen_all => {
                    seen_all = true;
                    "All".to_owned()
                }
                None => String::new(),
            })
            .collect()
    }

    fn cell(&self, i: usize, j: usize, item: Item) -> String {
        let t = &self.table;
        let percent = |v: f64| if v.is_finite() { format!("{v:.1}") } else { "—".to_owned() };
        match item {
            Item::Frequency(CellFrequency::Count) => count_text(t.counts[i][j]),
            Item::Frequency(CellFrequency::RowPercent) => percent(t.row_percent(i, j)),
            Item::Frequency(CellFrequency::ColumnPercent) => percent(t.column_percent(i, j)),
            Item::Frequency(CellFrequency::TotalPercent) => percent(t.total_percent(i, j)),
            Item::Summary(m, CellSummary::N) => count_text(t.summaries[i][j][m].n),
            Item::Summary(m, s) => {
                let v = s.of(&t.summaries[i][j][m]);
                if v.is_finite() { format_number(v, 5) } else { "—".to_owned() }
            }
        }
    }

    /// The table as text: one column per row classification, then one per
    /// column group and statistic.
    #[must_use]
    pub fn text_table(&self) -> TextTable {
        let items = self.items();
        let mut headings: Vec<String> =
            if self.row_names.is_empty() { vec![String::new()] } else { self.row_names.clone() };
        for g in &self.table.column_groups {
            let group = Self::group_labels(g, &self.column_levels)
                .into_iter()
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" / ");
            for &item in &items {
                headings.push(match (group.is_empty(), items.len()) {
                    (true, _) => self.item_label(item),
                    (false, 1) => group.clone(),
                    (false, _) => format!("{group} · {}", self.item_label(item)),
                });
            }
        }
        let mut out = TextTable::new(headings);
        for (i, g) in self.table.row_groups.iter().enumerate() {
            let mut row = if self.row_names.is_empty() {
                vec!["All".to_owned()]
            } else {
                Self::group_labels(g, &self.row_levels)
            };
            for j in 0..self.table.column_groups.len() {
                for &item in &items {
                    row.push(self.cell(i, j, item));
                }
            }
            out.push(row);
        }
        out
    }

    /// Report sections: the table and a note on what it holds.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let t = &self.table;
        let mut note = format!("{} rows tabulated (total count {}).", t.observations, count_text(t.grand_total));
        if t.skipped > 0 {
            note.push_str(&format!(
                " {} rows left out for an empty classification{}.",
                t.skipped,
                if self.frequency_name.is_some() { " or a missing or negative frequency" } else { "" }
            ));
        }
        if self.excluded > 0 {
            note.push_str(&format!(" {} excluded rows not used.", self.excluded));
        }
        if let Some(f) = &self.frequency_name {
            note.push_str(&format!(" Each row counts as many times as its {f} value."));
        }
        if self.request.frequencies.iter().any(|f| *f != CellFrequency::Count) {
            note.push_str(
                " Row % divides by the row's count over all columns, Column % by the column's count over all \
                 rows, Total % by the total count.",
            );
        }
        if !self.measure_names.is_empty() {
            note.push_str(" Summary statistics leave out missing values; SD uses the n − 1 divisor.");
        }
        if self.request.margins || self.request.subtotals {
            note.push_str(" \"All\" rows and columns span every level of a classification.");
        }
        vec![
            ReportSection {
                kind: SectionKind::MultiwayTable,
                title: "Multi-way Table".into(),
                content: SectionContent::Table(self.text_table()),
            },
            ReportSection {
                kind: SectionKind::MultiwayNote,
                title: "About This Table".into(),
                content: SectionContent::Note(note),
            },
        ]
    }
}

/// A count: an integer without decimals, otherwise six significant digits.
fn count_text(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 { format!("{}", v.round() as i64) } else { format_number(v, 6) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn data() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::text("A", ["a1", "a1", "a1", "a1", "a2", "a2", "a2", "a2"].map(String::from).to_vec()),
                Column::text("B", ["b1", "b1", "b2", "b2", "b1", "b1", "b1", "b2"].map(String::from).to_vec()),
                Column::text("Y", ["no", "yes", "yes", "yes", "no", "no", "yes", "yes"].map(String::from).to_vec()),
                Column::numeric("x", vec![1.0, 2.0, 3.0, 5.0, 4.0, 6.0, f64::NAN, 10.0]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn table_layout_and_cells() {
        let t = data();
        let mut req = MultiwayTableRequest::new(vec![0, 1], vec![2]);
        req.frequencies = vec![CellFrequency::Count, CellFrequency::RowPercent];
        req.measures = vec![3];
        req.statistics = vec![CellSummary::Mean];
        req.subtotals = true;
        let a = MultiwayTableAnalysis::run(&t, &RowStateManager::new(8), &req).unwrap();
        assert_eq!(a.title(), "Multi-way Table: A, B by Y");
        let tt = a.text_table();
        assert_eq!(tt.columns[..5], ["A", "B", "no · Count", "no · Row %", "no · Mean x"]);
        assert_eq!(tt.columns.len(), 2 + 3 * 3);
        assert_eq!(tt.rows.len(), 7);
        assert_eq!(tt.rows[0][..5], ["a1", "b1", "1", "50.0", "1"]);
        assert_eq!(tt.rows[2][..2], ["a1", "All"]);
        assert_eq!(tt.rows[6][..2], ["All", ""]);
        // Grand total: 8 rows; mean of x over the seven present values.
        assert_eq!(tt.rows[6][8], "8");
        assert_eq!(tt.rows[6][10], format_number(31.0 / 7.0, 5));
        // a2·b1·yes holds only the missing x.
        assert_eq!(tt.rows[3][7], "—");
        assert_eq!(a.sections().len(), 2);

        let back = MultiwayTableRequest::from_json(&req.to_json(&t), &t).unwrap();
        assert_eq!(back, req);
        let mut states = RowStateManager::new(8);
        states.set_excluded(0..4, true);
        let b = MultiwayTableAnalysis::run(&t, &states, &req).unwrap();
        assert_eq!(b.table().observations, 4);
    }

    #[test]
    fn invalid_requests() {
        let t = data();
        let states = RowStateManager::new(8);
        assert!(MultiwayTableAnalysis::run(&t, &states, &MultiwayTableRequest::new(vec![], vec![])).is_err());
        assert!(MultiwayTableAnalysis::run(&t, &states, &MultiwayTableRequest::new(vec![0], vec![0])).is_err());
        let mut req = MultiwayTableRequest::new(vec![0], vec![]);
        req.frequencies.clear();
        assert!(MultiwayTableAnalysis::run(&t, &states, &req).is_err(), "nothing to show");
        req.measures = vec![1];
        assert!(MultiwayTableAnalysis::run(&t, &states, &req).is_err(), "text measure");
        // A single classification, no column groups: the stat names head the columns.
        let a = MultiwayTableAnalysis::run(&t, &states, &MultiwayTableRequest::new(vec![2], vec![])).unwrap();
        assert_eq!(a.text_table().columns, ["Y", "Count"]);
        assert_eq!(a.text_table().rows, vec![vec!["no", "3"], vec!["yes", "5"], vec!["All", "8"]]);
    }

    #[test]
    fn excluded_rows_add_no_levels() {
        let t = data();
        let mut states = RowStateManager::new(8);
        states.set_excluded(4..8, true);
        let a = MultiwayTableAnalysis::run(&t, &states, &MultiwayTableRequest::new(vec![0], vec![])).unwrap();
        assert_eq!(a.text_table().rows, vec![vec!["a1", "4"], vec!["All", "4"]]);
        // Margins over the other classification are unchanged.
        let b = MultiwayTableAnalysis::run(&t, &states, &MultiwayTableRequest::new(vec![1], vec![])).unwrap();
        assert_eq!(b.text_table().rows, vec![vec!["b1", "2"], vec!["b2", "2"], vec!["All", "4"]]);
    }
}
