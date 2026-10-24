//! Copying cells out as text and pasting text in.
//!
//! **Copy** writes tab-separated text, one line per row, which spreadsheets
//! and text editors read as a block of cells. Values appear as the grid shows
//! them (numbers in shortest round-trip form, dates in their display format,
//! missing values empty); a field holding a tab, line break or quote is quoted
//! with doubled inner quotes (RFC 4180 style).
//!
//! **Paste** reads tab- or comma-separated text ([`parse`]): tab-separated if
//! the text has a tab, else comma-separated if every line splits into the same
//! number of fields (more than one) and not every comma is followed by a
//! space (`Smith, John` is one value), else one value per line. The paste
//! summary says when the text was split at commas.
//!
//! * [`paste_edit`] stores the block in the grid with its top-left corner at
//!   the cell cursor, as one undoable [`Edit::Batch`]. Rows and columns are
//!   added at the end as needed (a new column's type is inferred from its
//!   pasted values, as when reading a file). Each value is checked against
//!   its column's type; if any is wrong, nothing is pasted. A first line that
//!   repeats the target columns' names (as copied rows carry) is skipped.
//! * [`table_from_text`] makes a new table from the text. The first line is
//!   taken as column names when every field in it is filled in, distinct and
//!   not a number, and more lines follow; otherwise columns are named
//!   `Column 1`, `Column 2`, ….

use stat_core::{Result, StatError};
use stat_data::delimited::parse_records;
use stat_data::{CellValue, Column, ColumnKind, DataTable, RowState, table_from_records_with_notes};

use crate::editing::Edit;

/// Split clipboard text into rows of fields (see the module documentation).
/// Short rows are padded with empty fields; a trailing line break adds no row.
///
/// # Errors
/// [`StatError::InvalidArgument`] for empty text or an unterminated quoted field.
pub fn parse(text: &str) -> Result<Vec<Vec<String>>> {
    let text = text.strip_suffix('\n').map_or(text, |t| t.strip_suffix('\r').unwrap_or(t));
    if text.trim().is_empty() {
        return Err(invalid("the clipboard holds no text"));
    }
    let mut rows = if text.contains('\t') {
        parse_records(text, '\t')?
    } else {
        let comma = split_at_commas(text).then(|| parse_records(text, ',').ok()).flatten().filter(|r| {
            let width = r.first().map_or(0, Vec::len);
            width > 1 && r.iter().all(|f| f.len() == width)
        });
        match comma {
            Some(rows) => rows,
            None => text.lines().map(|l| vec![l.to_owned()]).collect(),
        }
    };
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        row.resize(width, String::new());
    }
    Ok(rows)
}

/// Whether text without tabs may be split at its commas: not when every
/// comma is followed by a space, as in a column of names written
/// `Last, First` or of prose.
fn split_at_commas(text: &str) -> bool {
    let mut commas = text.match_indices(',').peekable();
    commas.peek().is_some() && commas.any(|(i, _)| !text[i + 1..].starts_with([' ', '\u{a0}']))
}

/// Whether [`parse`] splits `text` at commas (it has no tab and splits into
/// columns there).
#[must_use]
pub fn splits_on_commas(text: &str) -> bool {
    !text.contains('\t') && parse(text).is_ok_and(|rows| rows.first().is_some_and(|r| r.len() > 1))
}

/// [`paste_edit`] of `text` as [`parse`] splits it. Text without tabs is
/// not split at commas into a table of one column, and when the pieces do
/// not fit the columns it is pasted one value per line instead; the summary
/// says when it was split.
///
/// # Errors
/// As for [`parse`] and [`paste_edit`].
pub fn paste_text(table: &DataTable, row: usize, column: usize, text: &str) -> Result<(Edit, PasteSummary)> {
    let cells = parse(text)?;
    if !splits_on_commas(text) {
        return paste_edit(table, row, column, &cells);
    }
    let trimmed = text.strip_suffix('\n').map_or(text, |t| t.strip_suffix('\r').unwrap_or(t));
    let lines: Vec<Vec<String>> = trimmed.lines().map(|l| vec![l.to_owned()]).collect();
    if table.n_cols() == 1 {
        return paste_edit(table, row, column, &lines);
    }
    match paste_edit(table, row, column, &cells) {
        Ok((edit, mut summary)) => {
            summary.split_on_commas = true;
            Ok((edit, summary))
        }
        Err(split_error) => paste_edit(table, row, column, &lines).map_err(|_| split_error),
    }
}

/// What a paste will do, for the status message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteSummary {
    /// Rows × columns of values pasted.
    pub cells: (usize, usize),
    /// Rows added at the end.
    pub added_rows: usize,
    /// Columns added at the end.
    pub added_columns: usize,
    /// The first line was skipped because it repeated the column names.
    pub skipped_header: bool,
    /// The text had no tabs and was split at commas.
    pub split_on_commas: bool,
}

impl PasteSummary {
    /// A sentence such as "Pasted 3 × 2 values; added 1 row and 1 column."
    #[must_use]
    pub fn describe(&self) -> String {
        let plural = |n: usize, what: &str| if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") };
        let mut text = format!("Pasted {} × {} values", self.cells.0, self.cells.1);
        let added: Vec<String> = [(self.added_rows, "row"), (self.added_columns, "column")]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, what)| plural(*n, what))
            .collect();
        if !added.is_empty() {
            text.push_str(&format!("; added {}", added.join(" and ")));
        }
        if self.skipped_header {
            text.push_str("; the column names line was skipped");
        }
        if self.split_on_commas {
            text.push_str(&format!("; the text was split on commas into {}", plural(self.cells.1, "column")));
        }
        text.push('.');
        text
    }
}

/// The edit that pastes `cells` (rows of text fields) into `table` with the
/// top-left value at `(row, column)`, adding rows and columns at the end as
/// needed (see the module documentation).
///
/// # Errors
/// [`StatError::InvalidArgument`] for an empty block, a cursor outside the
/// table, or a value that does not fit its column's type (naming the cell);
/// nothing is pasted then.
pub fn paste_edit(table: &DataTable, row: usize, column: usize, cells: &[Vec<String>]) -> Result<(Edit, PasteSummary)> {
    let (n_rows, n_cols) = (table.n_rows(), table.n_cols());
    if (n_rows > 0 && row >= n_rows) || (n_cols > 0 && column >= n_cols) {
        return Err(invalid("the cell cursor is outside the table"));
    }
    let width = cells.iter().map(Vec::len).max().unwrap_or(0);
    // Skip a first line that names the target columns (copied rows carry one).
    let skipped_header = cells.len() > 1
        && column + width <= n_cols
        && cells[0].iter().enumerate().all(|(j, f)| table.column(column + j).is_some_and(|c| c.name == f.trim()));
    let cells = if skipped_header { &cells[1..] } else { cells };
    if cells.is_empty() || width == 0 {
        return Err(invalid("there is nothing to paste"));
    }
    let height = cells.len();
    let field = |i: usize, j: usize| cells[i].get(j).map_or("", String::as_str);
    let rows_after = n_rows.max(row + height);
    let added_rows = rows_after - n_rows;
    let added_columns = (column + width).saturating_sub(n_cols);

    let mut edits = Vec::new();
    if added_rows > 0 {
        let empty = table.empty_row();
        let new = (n_rows..rows_after).map(|i| (i, empty.clone(), RowState::default())).collect();
        edits.push(Edit::InsertRows { rows: new });
    }
    // Existing columns: check every value first, then set the cells.
    for j in 0..width.min(n_cols.saturating_sub(column)) {
        let col = table.column(column + j).ok_or_else(|| invalid("no such column"))?;
        for i in 0..height {
            let r = row + i;
            let value = col
                .parse_value(field(i, j))
                .map_err(|e| invalid(format!("Row {}: {}. Nothing was pasted", r + 1, error_text(&e))))?;
            let unchanged = r < n_rows && same(&col.value(r), &value);
            if !unchanged {
                edits.push(Edit::SetCell { row: r, column: column + j, value });
            }
        }
    }
    // New columns: typed from their pasted values, missing above and below the block.
    let mut names: Vec<String> = table.columns().iter().map(|c| c.name.clone()).collect();
    for j in n_cols.saturating_sub(column)..width {
        let mut values = vec![String::new(); rows_after];
        for i in 0..height {
            values[row + i] = field(i, j).to_owned();
        }
        let name = unused_name(&names);
        let (inferred, _) =
            table_from_records_with_notes("", vec![name.clone()], values.into_iter().map(|v| vec![v]).collect())?;
        let new =
            inferred.column(0).cloned().unwrap_or_else(|| Column::empty(name.clone(), ColumnKind::Text, rows_after));
        edits.push(Edit::InsertColumn { index: names.len(), column: new });
        names.push(name);
    }
    let label = if height * width == 1 { "paste".to_owned() } else { format!("paste {height} × {width} values") };
    let summary =
        PasteSummary { cells: (height, width), added_rows, added_columns, skipped_header, split_on_commas: false };
    Ok((Edit::Batch { label, edits }, summary))
}

/// A new table named `name` from clipboard text (see the module documentation),
/// with notes about how values were read.
///
/// # Errors
/// As for [`parse`], or duplicate column names.
pub fn table_from_text(name: &str, text: &str) -> Result<(DataTable, Vec<String>)> {
    let mut rows = parse(text)?;
    let first = &rows[0];
    let distinct = first.iter().enumerate().all(|(j, f)| !first[..j].iter().any(|g| g.trim() == f.trim()));
    let header =
        rows.len() > 1 && distinct && first.iter().all(|f| !f.trim().is_empty() && f.trim().parse::<f64>().is_err());
    let names = if header {
        rows.remove(0).into_iter().map(|f| f.trim().to_owned()).collect()
    } else {
        (1..=first.len()).map(|j| format!("Column {j}")).collect()
    };
    table_from_records_with_notes(name, names, rows)
}

/// Tab-separated text of the given rows and columns (with a line of column
/// names first when `names` is set), ending in a line break.
#[must_use]
pub fn copy_text(table: &DataTable, rows: &[usize], columns: &[usize], names: bool) -> String {
    let cols: Vec<&Column> = columns.iter().filter_map(|&j| table.column(j)).collect();
    let mut out = String::new();
    let mut line = |fields: Vec<String>| {
        out.push_str(&fields.iter().map(|f| quote(f)).collect::<Vec<_>>().join("\t"));
        out.push('\n');
    };
    if names {
        line(cols.iter().map(|c| c.name.clone()).collect());
    }
    for &r in rows.iter().filter(|&&r| r < table.n_rows()) {
        line(cols.iter().map(|c| c.cell_text(r)).collect());
    }
    out
}

fn quote(field: &str) -> String {
    if field.contains(['\t', '\n', '\r', '"']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

fn same(a: &CellValue, b: &CellValue) -> bool {
    match (a, b) {
        (CellValue::Number(x), CellValue::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        (CellValue::Text(x), CellValue::Text(y)) => x == y,
        _ => false,
    }
}

fn unused_name(names: &[String]) -> String {
    (1..).map(|k| format!("Column {k}")).find(|n| !names.contains(n)).unwrap_or_default()
}

/// An error's message without its category prefix, for the status line.
#[must_use]
pub fn error_text(e: &StatError) -> String {
    match e {
        StatError::InvalidArgument { message } => message.clone(),
        other => other.to_string(),
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::EditHistory;
    use stat_data::{ColumnData, RowStateManager, SelectionMode};

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, f64::NAN]),
                Column::text("g", vec!["a".into(), "b\tc".into(), "".into()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn parses_tabs_commas_and_single_columns() {
        assert_eq!(parse("1\t2\n3\t4\n").unwrap(), [["1", "2"], ["3", "4"]]);
        assert_eq!(parse("a,b\r\n\"c, d\",e").unwrap(), [["a", "b"], ["c, d", "e"]]);
        // Commas that do not split every line the same way are text.
        assert_eq!(parse("Smith, J\nLee").unwrap(), [["Smith, J"], ["Lee"]]);
        // Every line splits in two, but every comma is followed by a space.
        assert_eq!(parse("Smith, John\nDoe, Jane\n").unwrap(), [["Smith, John"], ["Doe, Jane"]]);
        assert!(!splits_on_commas("Smith, John\nDoe, Jane\n"));
        assert!(splits_on_commas("1,2\n3,4"));
        assert_eq!(parse("1\t2\t3\n4").unwrap(), [["1", "2", "3"], ["4", "", ""]], "ragged rows are padded");
        assert!(parse(" \n").is_err());
    }

    #[test]
    fn copy_round_trips_through_parse() {
        let t = table();
        let text = copy_text(&t, &[0, 1, 2], &[0, 1], true);
        assert_eq!(text, "x\tg\n1\ta\n2\t\"b\tc\"\n\t\n");
        let back = parse(&text).unwrap();
        assert_eq!(back[2], ["2", "b\tc"]);
        assert_eq!(back[3], ["", ""]);
        assert_eq!(copy_text(&t, &[1], &[1], false), "\"b\tc\"\n");
    }

    #[test]
    fn paste_sets_cells_adds_rows_and_columns_and_undoes_as_one_step() {
        let mut t = table();
        let mut rows = RowStateManager::new(3);
        rows.select([0], SelectionMode::Replace);
        let original = t.clone();
        let cells = parse("5\tz\t7\n6\ty\t8\n9\tx\t\n").unwrap();
        let (edit, summary) = paste_edit(&t, 1, 0, &cells).unwrap();
        assert_eq!(
            summary,
            PasteSummary {
                cells: (3, 3),
                added_rows: 1,
                added_columns: 1,
                skipped_header: false,
                split_on_commas: false
            }
        );
        assert_eq!(summary.describe(), "Pasted 3 × 3 values; added 1 row and 1 column.");
        assert_eq!(edit.removed_columns(t.n_cols()), Vec::<usize>::new());
        let mut h = EditHistory::default();
        h.apply(edit, &mut t, &mut rows).unwrap();
        assert_eq!(t.n_rows(), 4);
        assert_eq!(rows.len(), 4);
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap()[..2], [1.0, 5.0]);
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap()[2..], [6.0, 9.0]);
        assert_eq!(t.column(1).unwrap().cell_text(3), "x");
        let new = t.column(2).unwrap();
        assert_eq!(new.name, "Column 1");
        assert!(matches!(new.data, ColumnData::Numeric(_)), "inferred numeric");
        assert_eq!(new.cell_text(0), "");
        assert_eq!(new.cell_text(2), "8");
        assert_eq!(h.next_undo().unwrap().0, "paste 3 × 3 values");
        // Undoing removes the added column, so it is reported as removed.
        assert_eq!(h.next_undo().unwrap().1.removed_columns(t.n_cols()), [2]);
        h.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t, original);
        assert_eq!(rows.selected_rows(), [0]);
        h.redo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t.n_cols(), 3);
    }

    #[test]
    fn paste_is_type_checked_and_all_or_nothing() {
        let t = table();
        let cells = parse("3\tq\nabc\tr").unwrap();
        let err = paste_edit(&t, 0, 0, &cells).unwrap_err().to_string();
        assert!(err.contains("Row 2") && err.contains("\"x\"") && err.contains("abc"), "{err}");
        assert!(paste_edit(&t, 5, 0, &cells).is_err(), "cursor outside the table");
    }

    #[test]
    fn paste_skips_a_header_line_matching_the_columns() {
        let t = table();
        let cells = parse(&copy_text(&t, &[0], &[0, 1], true)).unwrap();
        let (edit, summary) = paste_edit(&t, 2, 0, &cells).unwrap();
        assert!(summary.skipped_header);
        assert_eq!(summary.cells, (1, 2));
        let Edit::Batch { edits, .. } = edit else { panic!("a batch") };
        assert_eq!(edits.len(), 2, "two cells change; nothing added");
    }

    #[test]
    fn unchanged_cells_make_no_edits() {
        let t = table();
        let (edit, _) = paste_edit(&t, 0, 0, &parse("1\ta").unwrap()).unwrap();
        assert_eq!(edit, Edit::Batch { label: "paste 1 × 2 values".into(), edits: vec![] });
    }

    #[test]
    fn new_tables_detect_a_header_line() {
        let (t, _) = table_from_text("c", "Name\tScore\nAnn\t3\nBo\t4.5\n").unwrap();
        assert_eq!(t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Name", "Score"]);
        assert_eq!(t.column(1).unwrap().as_numeric().unwrap(), [3.0, 4.5]);
        let (t, _) = table_from_text("c", "1,2\n3,4").unwrap();
        assert_eq!(t.column(0).unwrap().name, "Column 1");
        assert_eq!(t.n_rows(), 2);
        let (t, _) = table_from_text("c", "a\ta\n1\t2").unwrap();
        assert_eq!(t.n_rows(), 2, "repeated names are data, not a header");
        let (t, _) = table_from_text("c", "only").unwrap();
        assert_eq!((t.n_rows(), t.column(0).unwrap().cell_text(0)), (1, "only".to_owned()));
    }

    /// A copied column of `Last, First` names used to be pasted as two
    /// columns; text split at commas now says so.
    #[test]
    fn comma_text_pastes_as_one_column_when_it_should() {
        let t = DataTable::new("t", vec![Column::text("Name", vec![String::new()])]).unwrap();
        let (edit, summary) = paste_text(&t, 0, 0, "Smith, John\nDoe, Jane\n").unwrap();
        assert_eq!((summary.cells, summary.split_on_commas), ((2, 1), false));
        let mut t2 = t.clone();
        let mut rows = RowStateManager::new(1);
        EditHistory::default().apply(edit, &mut t2, &mut rows).unwrap();
        assert_eq!(t2.column(0).unwrap().cell_text(0), "Smith, John");
        // Into a one-column table even without spaces.
        let (_, summary) = paste_text(&t, 0, 0, "1,234\n5,678").unwrap();
        assert_eq!((summary.cells, summary.added_columns), ((2, 1), 0));
        // Two columns: split, and the summary says so.
        let t = DataTable::new("t", vec![Column::numeric("a", vec![0.0]), Column::numeric("b", vec![0.0])]).unwrap();
        let (_, summary) = paste_text(&t, 0, 0, "1,2\n3,4").unwrap();
        assert!(summary.split_on_commas);
        assert!(summary.describe().contains("split on commas into 2 columns"), "{}", summary.describe());
    }
}
