//! Multiple-response (check-all-that-apply) frequencies
//! ([`stat_discrete::multiple_response`]).
//!
//! The answers come in one of the two usual layouts:
//!
//! * **one text column of lists** — each cell holds the items a case picked,
//!   separated by a delimiter (`"Price; Quality"`); spaces around items are
//!   ignored and an item listed twice in one cell counts once;
//! * **one indicator column per item** — a cell counts as picked when it is
//!   a number other than 0, or text such as `yes`, `y`, `true`, `x`,
//!   `checked` or `1` (any case); the column name is the item.
//!
//! Excluded rows are left out; so is a case with no pick (counted as "no
//! response"). An optional group column adds a cross-tabulation and each
//! item's test of independence from the group, with a Bonferroni overall
//! test. The frequency chart's bars hold the rows of the cases picking each
//! item, so clicking a bar selects them everywhere.

use crate::compare::section;
use crate::format::{format_number, format_p_value};
use crate::project::{invalid, member, s};
use crate::report::{BarChartSpec, BarItem, ReportSection, SectionContent, SectionKind, TextTable};
use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_discrete::multiple_response::{
    MultipleResponseCrossTab, MultipleResponseTable, multiple_response_crosstab, multiple_response_frequencies,
};

const DIGITS: usize = 4;

/// Where the answers are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseSource {
    /// One text column of delimited lists.
    Lists {
        /// The column.
        column: usize,
        /// What separates the items (`";"` by default).
        delimiter: String,
    },
    /// One indicator column per item.
    Indicators {
        /// The columns, in item order.
        columns: Vec<usize>,
    },
}

/// The order of the items in the tables and chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemOrder {
    /// Most often picked first (ties in the order found).
    #[default]
    Frequency,
    /// As found: first appearance in a list column, column order for
    /// indicators.
    AsFound,
}

impl ItemOrder {
    /// Both orders.
    pub const ALL: [Self; 2] = [Self::Frequency, Self::AsFound];

    /// Label for menus.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Frequency => "Most picked first",
            Self::AsFound => "As found",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Frequency => "frequency",
            Self::AsFound => "as-found",
        }
    }
}

/// Settings of a multiple-response report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipleResponseRequest {
    /// The answers.
    pub source: ResponseSource,
    /// Optional group column for the cross-tabulation.
    pub group: Option<usize>,
    /// Item order.
    pub order: ItemOrder,
}

impl MultipleResponseRequest {
    /// Lists in `column` separated by `;`.
    #[must_use]
    pub fn lists(column: usize) -> Self {
        Self {
            source: ResponseSource::Lists { column, delimiter: ";".to_owned() },
            group: None,
            order: ItemOrder::default(),
        }
    }

    /// Every column the request uses.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = match &self.source {
            ResponseSource::Lists { column, .. } => vec![*column],
            ResponseSource::Indicators { columns } => columns.clone(),
        };
        out.extend(self.group);
        out
    }

    /// The request with its columns renumbered by `f`.
    #[must_use]
    pub fn map_columns(&self, f: &dyn Fn(usize) -> usize) -> Self {
        let source = match &self.source {
            ResponseSource::Lists { column, delimiter } => {
                ResponseSource::Lists { column: f(*column), delimiter: delimiter.clone() }
            }
            ResponseSource::Indicators { columns } => {
                ResponseSource::Indicators { columns: columns.iter().map(|&c| f(c)).collect() }
            }
        };
        Self { source, group: self.group.map(f), order: self.order }
    }

    /// Project-file form: `{"analysis": "multiple-response", "column": …,
    /// "delimiter": …}` or `{… "indicators": [names]}`, with `"group"` and
    /// `"order"`.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let col = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let mut members = vec![("analysis".to_owned(), s("multiple-response"))];
        match &self.source {
            ResponseSource::Lists { column, delimiter } => {
                members.push(("column".to_owned(), col(*column)));
                members.push(("delimiter".to_owned(), s(delimiter)));
            }
            ResponseSource::Indicators { columns } => {
                members.push(("indicators".to_owned(), JsonValue::Array(columns.iter().map(|&c| col(c)).collect())));
            }
        }
        members.push(("group".to_owned(), self.group.map_or(JsonValue::Null, col)));
        members.push(("order".to_owned(), s(self.order.key())));
        JsonValue::Object(members)
    }

    /// Read the project-file form.
    ///
    /// # Errors
    /// Missing or unknown columns, or an unknown order.
    pub fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let find = |value: &JsonValue, what: &str| -> Result<usize> {
            let name = value.as_str().ok_or_else(|| invalid(format!("{what}: expected a column name")))?;
            table.find(name).ok_or_else(|| invalid(format!("report refers to a missing column {name:?}")))
        };
        let source = match v.get("indicators") {
            Some(JsonValue::Array(list)) => ResponseSource::Indicators {
                columns: list.iter().map(|c| find(c, "indicators")).collect::<Result<_>>()?,
            },
            _ => ResponseSource::Lists {
                column: find(member(v, "column")?, "column")?,
                delimiter: v.get("delimiter").and_then(JsonValue::as_str).unwrap_or(";").to_owned(),
            },
        };
        let group = match v.get("group") {
            None | Some(JsonValue::Null) => None,
            Some(g) => Some(find(g, "group")?),
        };
        let order = match v.get("order").and_then(JsonValue::as_str) {
            None | Some("frequency") => ItemOrder::Frequency,
            Some("as-found") => ItemOrder::AsFound,
            Some(other) => return Err(invalid(format!("multiple response: unknown order {other:?}"))),
        };
        Ok(Self { source, group, order })
    }
}

/// The items in one list cell: split at `delimiter`, trimmed, empty pieces
/// and repeats dropped (first occurrence kept).
#[must_use]
pub fn split_items(text: &str, delimiter: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let pieces: Vec<&str> = if delimiter.is_empty() { vec![text] } else { text.split(delimiter).collect() };
    for piece in pieces {
        let item = piece.trim();
        if !item.is_empty() && !out.iter().any(|o| o == item) {
            out.push(item.to_owned());
        }
    }
    out
}

/// Whether an indicator cell's text counts as picked.
fn picked_text(text: &str) -> bool {
    matches!(text.trim().to_lowercase().as_str(), "yes" | "y" | "true" | "x" | "checked" | "1" | "✓")
}

/// A computed multiple-response report.
#[derive(Debug, Clone)]
pub struct MultipleResponseAnalysis {
    request: MultipleResponseRequest,
    /// What the answers are called (the list column, or "Items").
    name: String,
    group_name: Option<String>,
    /// Items in report order.
    items: Vec<String>,
    /// Rows of the cases picking each item (report order).
    item_rows: Vec<Vec<usize>>,
    /// The frequencies.
    pub table: MultipleResponseTable,
    /// The cross-tabulation, with a group.
    pub crosstab: Option<MultipleResponseCrossTab>,
}

impl MultipleResponseAnalysis {
    /// Read the answers of the included rows and tabulate them.
    ///
    /// # Errors
    /// Invalid columns, a group column among the answer columns, fewer than
    /// one item, or no case picking anything.
    pub fn run(table: &DataTable, rows: &RowStateManager, request: &MultipleResponseRequest) -> Result<Self> {
        let used: Vec<usize> = (0..table.n_rows()).filter(|&r| !rows.is_excluded(r)).collect();
        let (name, mut items, mut picks) = match &request.source {
            ResponseSource::Lists { column, delimiter } => {
                let c = table.column(*column).ok_or_else(|| invalid("no such column"))?;
                let mut items: Vec<String> = Vec::new();
                let cells: Vec<Vec<String>> = used.iter().map(|&r| split_items(&c.level_text(r), delimiter)).collect();
                for cell in &cells {
                    for item in cell {
                        if !items.contains(item) {
                            items.push(item.clone());
                        }
                    }
                }
                let picks: Vec<Vec<bool>> =
                    cells.iter().map(|cell| items.iter().map(|i| cell.contains(i)).collect()).collect();
                (c.name.clone(), items, picks)
            }
            ResponseSource::Indicators { columns } => {
                if columns.len() < 2 {
                    return Err(invalid("choose at least two indicator columns, one per item"));
                }
                let cols = columns
                    .iter()
                    .map(|&i| table.column(i).ok_or_else(|| invalid("no such column")))
                    .collect::<Result<Vec<_>>>()?;
                let items = cols.iter().map(|c| c.name.clone()).collect();
                let picks = used
                    .iter()
                    .map(|&r| {
                        cols.iter()
                            .map(|c| match c.as_numeric() {
                                Some(v) => !v[r].is_nan() && v[r] != 0.0,
                                None => picked_text(&c.level_text(r)),
                            })
                            .collect()
                    })
                    .collect();
                ("Items".to_owned(), items, picks)
            }
        };
        if request.group.is_some_and(|g| request.columns()[..request.columns().len() - 1].contains(&g)) {
            return Err(invalid("the group column cannot also hold the answers"));
        }
        if items.is_empty() {
            return Err(invalid(format!("{name} lists no items")));
        }
        if request.order == ItemOrder::Frequency {
            let counts: Vec<usize> = (0..items.len()).map(|i| picks.iter().filter(|p| p[i]).count()).collect();
            let mut order: Vec<usize> = (0..items.len()).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(counts[i]));
            items = order.iter().map(|&i| items[i].clone()).collect();
            picks = picks.iter().map(|p| order.iter().map(|&i| p[i]).collect()).collect();
        }
        let frequencies = multiple_response_frequencies(&picks, &items)?;
        let item_rows = (0..items.len())
            .map(|i| used.iter().zip(&picks).filter(|(_, p)| p[i]).map(|(&r, _)| r).collect())
            .collect();
        let (crosstab, group_name) = match request.group {
            None => (None, None),
            Some(g) => {
                let c = table.column(g).ok_or_else(|| invalid("no such group column"))?;
                let labels: Vec<Option<String>> = used
                    .iter()
                    .map(|&r| {
                        let t = c.level_text(r);
                        (!t.is_empty()).then_some(t)
                    })
                    .collect();
                (Some(multiple_response_crosstab(&picks, &items, &labels)?), Some(c.name.clone()))
            }
        };
        Ok(Self { request: request.clone(), name, group_name, items, item_rows, table: frequencies, crosstab })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &MultipleResponseRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.group_name {
            Some(g) => format!("Multiple Response: {} by {g}", self.name),
            None => format!("Multiple Response: {}", self.name),
        }
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        let t = &self.table;
        let mut freq = TextTable::new(["Item", "Cases", "% of cases", "% of responses"]);
        for f in &t.items {
            freq.push([
                f.item.clone(),
                f.count.to_string(),
                format_number(f.percent_of_cases, DIGITS),
                format_number(f.percent_of_responses, DIGITS),
            ]);
        }
        let case_total: f64 = t.items.iter().map(|f| f.percent_of_cases).sum();
        freq.push(["Total".to_owned(), t.responses.to_string(), format_number(case_total, DIGITS), "100".to_owned()]);
        let mut out = vec![section(
            SectionKind::MultipleResponseFrequencies,
            format!(
                "Response Frequencies: {} respondents (% of cases adds to more than 100 when cases pick several \
                 items)",
                t.respondents
            ),
            SectionContent::Table(freq),
        )];
        let mut summary = TextTable::new(["Statistic", "Value"]);
        summary.push(["Cases used".to_owned(), t.cases.to_string()]);
        summary.push(["Respondents (at least one item)".to_owned(), t.respondents.to_string()]);
        summary.push(["No response".to_owned(), (t.cases - t.respondents).to_string()]);
        summary.push(["Responses".to_owned(), t.responses.to_string()]);
        summary.push(["Items per respondent".to_owned(), format_number(t.mean_per_respondent(), DIGITS)]);
        summary.push(["Different items".to_owned(), t.items.len().to_string()]);
        out.push(section(SectionKind::MultipleResponseSummary, "Respondents", SectionContent::Table(summary)));
        let bars = self
            .item_rows
            .iter()
            .enumerate()
            .map(|(i, rows)| BarItem { center: (i + 1) as f64, width: 0.7, rows: rows.clone(), unlinked: 0 })
            .collect();
        let ticks = self.items.iter().enumerate().map(|(i, l)| ((i + 1) as f64, l.clone())).collect();
        out.push(section(
            SectionKind::MultipleResponseChart,
            "Frequency Chart (click a bar to select the cases that picked it)",
            SectionContent::Bars(BarChartSpec {
                x_time: None,
                x_label: self.name.clone(),
                y_label: "Cases".to_owned(),
                bars,
                x_ticks: Some(ticks),
                curve: None,
                references: Vec::new(),
            }),
        ));
        if let (Some(c), Some(g)) = (&self.crosstab, &self.group_name) {
            let mut headings = vec!["Item".to_owned()];
            for grp in &c.groups {
                headings.push(format!("{} (n = {})", grp.group, grp.respondents));
                headings.push(format!("{} %", grp.group));
            }
            let mut cross = TextTable::new(headings);
            for (i, item) in c.items.iter().enumerate() {
                let mut row = vec![item.clone()];
                for grp in &c.groups {
                    row.push(grp.counts[i].to_string());
                    row.push(format_number(grp.percents[i], DIGITS));
                }
                cross.push(row);
            }
            out.push(section(
                SectionKind::MultipleResponseCrossTab,
                format!("Cross-tabulation by {g} (cases picking each item, and % of the group's respondents)"),
                SectionContent::Table(cross),
            ));
            let mut tests = TextTable::new(["Item", "χ²", "DF", "p-value", "Bonferroni p", "Note"]);
            for (item, test) in c.items.iter().zip(&c.tests) {
                match test {
                    Some(t) => tests.push([
                        item.clone(),
                        format_number(t.chi_square, 5),
                        t.df.to_string(),
                        format_p_value(t.p_value),
                        format_p_value(t.adjusted_p_value),
                        if t.small_expected { "small expected counts".to_owned() } else { String::new() },
                    ]),
                    None => tests.push([
                        item.clone(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        "not testable (picked by all or none, or one group)".to_owned(),
                    ]),
                }
            }
            let overall = c.overall_p_value.map_or_else(
                || "no item could be tested".to_owned(),
                |p| format!("overall Bonferroni p = {}", format_p_value(p)),
            );
            out.push(section(
                SectionKind::MultipleResponseTests,
                format!("Tests of Marginal Independence from {g}: each item a 2 × G Pearson χ²; {overall}"),
                SectionContent::Table(tests),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn survey() -> DataTable {
        let reasons = ["Price; Quality", "Price", "", "Price;Service", "Quality ; Service", "Price; Quality; Service"];
        DataTable::new(
            "Survey",
            vec![
                Column::text("Reasons", reasons.iter().map(|s| (*s).to_owned()).collect()),
                Column::text("Region", ["x", "x", "x", "y", "y", "y"].iter().map(|s| (*s).to_owned()).collect()),
                Column::numeric("Price", vec![1.0, 1.0, 0.0, 1.0, 0.0, 1.0]),
                Column::numeric("Quality", vec![1.0, 0.0, 0.0, 0.0, 1.0, 1.0]),
                Column::text("Service", ["no", "", "", "Yes", "x", "TRUE"].iter().map(|s| (*s).to_owned()).collect()),
            ],
        )
        .unwrap()
    }

    #[test]
    fn items_split_and_trim() {
        assert_eq!(split_items(" a ;b;; a ;c ", ";"), ["a", "b", "c"]);
        assert!(split_items("   ", ";").is_empty());
        assert_eq!(split_items("a, b", ","), ["a", "b"]);
    }

    #[test]
    fn lists_and_indicators_give_the_same_hand_counts() {
        let t = survey();
        let rows = RowStateManager::new(6);
        let lists = MultipleResponseAnalysis::run(&t, &rows, &MultipleResponseRequest::lists(0)).unwrap();
        assert_eq!(lists.items, ["Price", "Quality", "Service"]);
        let counts: Vec<usize> = lists.table.items.iter().map(|i| i.count).collect();
        assert_eq!(counts, [4, 3, 3]);
        assert_eq!((lists.table.respondents, lists.table.responses), (5, 10));
        assert_eq!(lists.item_rows[0], [0, 1, 3, 5]);
        let ind = MultipleResponseRequest {
            source: ResponseSource::Indicators { columns: vec![2, 3, 4] },
            group: None,
            order: ItemOrder::AsFound,
        };
        let indicators = MultipleResponseAnalysis::run(&t, &rows, &ind).unwrap();
        assert_eq!(indicators.table, lists.table);
        // Excluded rows are left out.
        let mut rows = RowStateManager::new(6);
        rows.set_excluded([5], true);
        let fewer = MultipleResponseAnalysis::run(&t, &rows, &MultipleResponseRequest::lists(0)).unwrap();
        assert_eq!(fewer.table.responses, 7);
    }

    #[test]
    fn a_group_adds_the_crosstab_and_tests() {
        let t = survey();
        let request = MultipleResponseRequest { group: Some(1), ..MultipleResponseRequest::lists(0) };
        let a = MultipleResponseAnalysis::run(&t, &RowStateManager::new(6), &request).unwrap();
        let c = a.crosstab.as_ref().unwrap();
        assert_eq!(c.groups[0].counts, [2, 1, 0]);
        // Service: 0 of 2 in x and 3 of 3 in y, X² = 5 (see stat-discrete).
        assert!((c.tests[2].as_ref().unwrap().chi_square - 5.0).abs() < 1e-12);
        let sections = a.sections();
        assert_eq!(sections.len(), 5);
        assert_eq!(a.title(), "Multiple Response: Reasons by Region");
        let SectionContent::Bars(chart) = &sections[2].content else { panic!() };
        assert_eq!(chart.bars[0].rows, [0, 1, 3, 5]);
    }

    #[test]
    fn requests_round_trip_and_follow_columns() {
        let t = survey();
        for request in [
            MultipleResponseRequest {
                group: Some(1),
                order: ItemOrder::AsFound,
                source: ResponseSource::Lists { column: 0, delimiter: ",".into() },
            },
            MultipleResponseRequest {
                source: ResponseSource::Indicators { columns: vec![2, 4] },
                group: None,
                order: ItemOrder::Frequency,
            },
        ] {
            let json = request.to_json(&t);
            assert_eq!(MultipleResponseRequest::from_json(&json, &t).unwrap(), request);
        }
        let moved = MultipleResponseRequest::lists(0).map_columns(&|c| c + 2);
        assert_eq!(moved.columns(), [2]);
        let bad = MultipleResponseRequest { group: Some(0), ..MultipleResponseRequest::lists(0) };
        assert!(MultipleResponseAnalysis::run(&t, &RowStateManager::new(6), &bad).is_err());
    }
}
