//! Finding text in a data table's cells, for the data grid's Find bar.
//!
//! Cells are matched on the text the grid shows for them
//! ([`stat_data::Column::cell_text`]), so a number matches as displayed and a
//! missing value never matches. Matching is a substring test, optionally
//! ignoring letter case. Replacing rewrites the matched text inside each cell
//! and reads the result back as a value of the column's type, as typing it
//! would.

use stat_data::DataTable;

use crate::editing::Edit;

/// What to look for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// The text to find. An empty query matches nothing.
    pub text: String,
    /// Upper and lower case letters must match exactly.
    pub match_case: bool,
}

impl Query {
    /// The query's text for comparison, lower-cased unless case matters.
    fn needle(&self) -> String {
        if self.match_case { self.text.clone() } else { self.text.to_lowercase() }
    }

    fn matches(&self, needle: &str, cell: &str) -> bool {
        if self.match_case { cell.contains(needle) } else { cell.to_lowercase().contains(needle) }
    }
}

/// The rows (ascending) with at least one cell in `columns` (every column
/// when empty) containing the query's text.
#[must_use]
pub fn matching_rows(table: &DataTable, query: &Query, columns: &[usize]) -> Vec<usize> {
    if query.text.is_empty() {
        return Vec::new();
    }
    let needle = query.needle();
    let all: Vec<usize> = (0..table.n_cols()).collect();
    let columns = if columns.is_empty() { &all[..] } else { columns };
    let cols = table.columns();
    (0..table.n_rows()).filter(|&r| columns.iter().any(|&c| query.matches(&needle, &cols[c].cell_text(r)))).collect()
}

/// The next cell `(row, column)` after `from` containing the query's text,
/// reading across each row and then down, wrapping round to the first cell.
/// `from` itself is checked last, so repeated calls step through every
/// match. Without `from` the search starts at the first cell. `None` when no
/// cell matches.
#[must_use]
pub fn next_match(table: &DataTable, query: &Query, from: Option<(usize, usize)>) -> Option<(usize, usize)> {
    let (n_rows, n_cols) = (table.n_rows(), table.n_cols());
    let total = n_rows * n_cols;
    if query.text.is_empty() || total == 0 {
        return None;
    }
    let needle = query.needle();
    // Linear position of the first cell to test.
    let start = from.map_or(0, |(r, c)| (r.min(n_rows - 1) * n_cols + c.min(n_cols - 1) + 1) % total);
    let cols = table.columns();
    (0..total)
        .map(|k| (start + k) % total)
        .map(|i| (i / n_cols, i % n_cols))
        .find(|&(r, c)| query.matches(&needle, &cols[c].cell_text(r)))
}

/// `text` with every occurrence of the query's text replaced by
/// `replacement`, matching letter case only if the query asks to; `None`
/// when the query's text does not occur. Matching goes character by
/// character, so it is safe for any Unicode text.
#[must_use]
pub fn replace_text(text: &str, query: &Query, replacement: &str) -> Option<String> {
    if query.text.is_empty() {
        return None;
    }
    let same = |a: char, b: char| if query.match_case { a == b } else { a.to_lowercase().eq(b.to_lowercase()) };
    let hay: Vec<char> = text.chars().collect();
    let needle: Vec<char> = query.text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut found) = (0, false);
    while i < hay.len() {
        if i + needle.len() <= hay.len() && hay[i..i + needle.len()].iter().zip(&needle).all(|(&a, &b)| same(a, b)) {
            out.push_str(replacement);
            i += needle.len();
            found = true;
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    found.then_some(out)
}

/// What a replacement did: the edit to apply (one undoable step, `None` when
/// nothing changed), how many cells it changes, and how many matching cells
/// were left alone because the new text is not a value of their column's type
/// (or the column is a formula column).
#[derive(Debug)]
pub struct Replacement {
    /// The edit, labelled "replace N cells".
    pub edit: Option<Edit>,
    /// Cells the edit changes.
    pub changed: usize,
    /// Matching cells that could not take the new text.
    pub skipped: usize,
}

/// Replace the query's text with `replacement` in the given `(row, column)`
/// cells. Formula cells, and cells whose new text is not a valid value for
/// their column (such as letters in a numeric column), are skipped and
/// counted.
#[must_use]
pub fn replace_in_cells(
    table: &DataTable,
    query: &Query,
    replacement: &str,
    cells: impl IntoIterator<Item = (usize, usize)>,
) -> Replacement {
    let cols = table.columns();
    let (mut edits, mut skipped) = (Vec::new(), 0);
    for (row, column) in cells {
        let Some(col) = cols.get(column).filter(|_| row < table.n_rows()) else { continue };
        let Some(text) = replace_text(&col.cell_text(row), query, replacement) else { continue };
        match col.parse_value(&text) {
            Ok(value) if col.formula.is_none() => edits.push(Edit::SetCell { row, column, value }),
            _ => skipped += 1,
        }
    }
    let changed = edits.len();
    let edit = (changed > 0).then(|| Edit::Batch {
        label: format!("replace {changed} cell{}", if changed == 1 { "" } else { "s" }),
        edits,
    });
    Replacement { edit, changed, skipped }
}

/// Replace the query's text in every cell of `columns` (every column when
/// empty), as [`replace_in_cells`].
#[must_use]
pub fn replace_all(table: &DataTable, query: &Query, replacement: &str, columns: &[usize]) -> Replacement {
    let all: Vec<usize> = (0..table.n_cols()).collect();
    let columns = if columns.is_empty() { &all[..] } else { columns };
    let cells = (0..table.n_rows()).flat_map(|r| columns.iter().map(move |&c| (r, c)));
    replace_in_cells(table, query, replacement, cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::text("name", vec!["Apple".into(), "banana".into(), "Cherry".into(), String::new()]),
                Column::numeric("weight", vec![12.5, 3.0, f64::NAN, 125.0]),
            ],
        )
        .unwrap()
    }

    fn query(text: &str, match_case: bool) -> Query {
        Query { text: text.into(), match_case }
    }

    #[test]
    fn matching_rows_ignores_case_unless_asked() {
        let t = table();
        assert_eq!(matching_rows(&t, &query("an", false), &[]), vec![1]);
        assert_eq!(matching_rows(&t, &query("apple", false), &[]), vec![0]);
        assert!(matching_rows(&t, &query("apple", true), &[]).is_empty());
        assert!(matching_rows(&t, &query("", false), &[]).is_empty());
    }

    #[test]
    fn numbers_match_as_shown_and_columns_restrict() {
        let t = table();
        assert_eq!(matching_rows(&t, &query("12", false), &[]), vec![0, 3]);
        assert_eq!(matching_rows(&t, &query("12", false), &[0]), Vec::<usize>::new());
        assert_eq!(matching_rows(&t, &query("e", false), &[0]), vec![0, 2]);
    }

    #[test]
    fn next_match_steps_through_and_wraps() {
        let t = table();
        let q = query("12", false);
        let first = next_match(&t, &q, None);
        assert_eq!(first, Some((0, 1)));
        let second = next_match(&t, &q, first);
        assert_eq!(second, Some((3, 1)));
        assert_eq!(next_match(&t, &q, second), first);
        assert_eq!(next_match(&t, &query("zzz", false), None), None);
        // A lone match is found again from itself.
        assert_eq!(next_match(&t, &query("cherry", false), Some((2, 0))), Some((2, 0)));
    }

    #[test]
    fn replace_text_respects_case_and_unicode() {
        assert_eq!(replace_text("Apple pie", &query("p", false), "b").as_deref(), Some("Abble bie"));
        assert_eq!(replace_text("Apple", &query("a", true), "x"), None);
        assert_eq!(replace_text("Straße ÄÖ", &query("äö", false), "AE").as_deref(), Some("Straße AE"));
        assert_eq!(replace_text("abc", &query("", false), "x"), None);
    }

    #[test]
    fn replace_all_parses_values_and_skips_bad_ones() {
        let mut t = table();
        let r = replace_all(&t, &query("12", false), "13", &[]);
        assert_eq!((r.changed, r.skipped), (2, 0));
        let mut rows = stat_data::RowStateManager::new(t.n_rows());
        r.edit.unwrap().apply(&mut t, &mut rows).unwrap();
        assert_eq!(t.columns()[1].cell_text(0), "13.5");
        assert_eq!(t.columns()[1].cell_text(3), "135");
        // Letters cannot go into a numeric column.
        let r = replace_all(&t, &query("3", false), "x", &[]);
        assert_eq!((r.changed, r.skipped), (0, 3));
        assert!(r.edit.is_none());
        // Restricted to a column.
        let r = replace_all(&t, &query("a", false), "o", &[0]);
        assert_eq!(r.changed, 2);
    }
}
