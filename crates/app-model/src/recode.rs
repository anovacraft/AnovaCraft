//! Recoding a column: mapping each distinct value to a new one, for the data
//! grid's Recode dialog.
//!
//! The values are listed as the grid shows them ([`stat_data::Column::cell_text`]).
//! Giving several values the same new text merges them into one category;
//! blank new text makes the value missing. The result either replaces the
//! column's values in place (one undoable step) or becomes a new column whose
//! type is the original's when every new value fits it, else numbers when
//! every new value is a number, else text.

use std::collections::HashMap;

use stat_core::{Result, StatError};
use stat_data::{CellValue, Column, ColumnKind, DataTable, MeasurementLevel};

use crate::editing::Edit;

/// One distinct value of a column, with how many rows hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueCount {
    /// The value as the grid shows it (empty for missing).
    pub text: String,
    /// Rows holding it.
    pub count: usize,
}

/// The distinct values of column `column` with their counts: numbers (and
/// dates) in increasing order, text in alphabetical order ignoring case, the
/// missing value (if any) last.
///
/// # Errors
/// [`StatError::InvalidArgument`] if the column does not exist.
pub fn distinct_values(table: &DataTable, column: usize) -> Result<Vec<ValueCount>> {
    let col = table.column(column).ok_or_else(|| invalid(format!("there is no column {}", column + 1)))?;
    let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
    for r in 0..table.n_rows() {
        // The count and the first row holding the value (for its number).
        counts.entry(col.cell_text(r)).or_insert((0, r)).0 += 1;
    }
    let numeric = col.kind() != ColumnKind::Text;
    let sort_key = |text: &str, row: usize| -> (bool, f64, String) {
        let number = match col.value(row) {
            CellValue::Number(x) if numeric => x,
            _ => 0.0,
        };
        (text.is_empty(), number, text.to_lowercase())
    };
    let mut values: Vec<(String, usize, usize)> = counts.into_iter().map(|(t, (n, row))| (t, n, row)).collect();
    values.sort_by(|a, b| {
        let (ka, kb) = (sort_key(&a.0, a.2), sort_key(&b.0, b.2));
        ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1)).then(ka.2.cmp(&kb.2)).then(a.0.cmp(&b.0))
    });
    Ok(values.into_iter().map(|(text, count, _)| ValueCount { text, count }).collect())
}

/// Where the recoded values go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecodeTarget {
    /// Replace the column's values.
    InPlace,
    /// A new column with this name, inserted right after the original.
    NewColumn(String),
}

/// The edit that recodes column `column` by `mapping` (old text → new
/// text; values not in the mapping stay as they are), and how many cells
/// change.
///
/// # Errors
/// [`StatError::InvalidArgument`] if the column does not exist, is a formula
/// column being recoded in place, a new value is not valid for the column's
/// type when recoding in place (recode into a new column instead, which may
/// be text), the new name is empty or taken, or nothing would change in place.
pub fn recode_edit(
    table: &DataTable,
    column: usize,
    mapping: &HashMap<String, String>,
    target: &RecodeTarget,
) -> Result<(Edit, usize)> {
    let col = table.column(column).ok_or_else(|| invalid(format!("there is no column {}", column + 1)))?;
    let n = table.n_rows();
    let new_texts: Vec<String> = (0..n)
        .map(|r| {
            let old = col.cell_text(r);
            mapping.get(&old).cloned().unwrap_or(old)
        })
        .collect();
    let changed = (0..n).filter(|&r| new_texts[r] != col.cell_text(r)).count();
    match target {
        RecodeTarget::InPlace => {
            if col.formula.is_some() {
                return Err(invalid(format!(
                    "“{}” is a formula column; recode it into a new column, or convert the formula to values first",
                    col.name
                )));
            }
            let mut edits = Vec::new();
            for (r, text) in new_texts.iter().enumerate() {
                if *text == col.cell_text(r) {
                    continue;
                }
                let value = col.parse_value(text).map_err(|_| {
                    invalid(format!(
                        "“{text}” is not a valid value for “{}” ({}); recode into a new column instead",
                        col.name,
                        kind_name(col.kind())
                    ))
                })?;
                edits.push(Edit::SetCell { row: r, column, value });
            }
            if edits.is_empty() {
                return Err(invalid("no value would change".to_owned()));
            }
            let label = format!("recode {}", col.name);
            Ok((Edit::Batch { label, edits }, changed))
        }
        RecodeTarget::NewColumn(name) => {
            let name = name.trim();
            if name.is_empty() {
                return Err(invalid("give the new column a name".to_owned()));
            }
            if table.find(name).is_some() {
                return Err(invalid(format!("there is already a column named “{name}”")));
            }
            let new = new_column(col, name, &new_texts);
            Ok((Edit::InsertColumn { index: column + 1, column: new }, changed))
        }
    }
}

/// A column named `name` holding `texts`: of `like`'s type when every text
/// fits it, else numbers when every text is a number, else text. Rows whose
/// text is unchanged keep `like`'s value exactly (display text can round it,
/// such as the time of day of a date shown without one). It keeps `like`'s
/// measurement level unless a text column would be continuous.
fn new_column(like: &Column, name: &str, texts: &[String]) -> Column {
    let parsed: Option<Vec<CellValue>> = texts
        .iter()
        .enumerate()
        .map(|(r, t)| if *t == like.cell_text(r) { Some(like.value(r)) } else { like.parse_value(t).ok() })
        .collect();
    let numbers = |values: &[CellValue]| -> Vec<f64> {
        values.iter().map(|v| if let CellValue::Number(x) = v { *x } else { f64::NAN }).collect()
    };
    let mut column = match (parsed, like.kind()) {
        (Some(values), ColumnKind::DateTime(display)) => Column::date_time(name, numbers(&values), display),
        (Some(values), ColumnKind::Numeric) => Column::numeric(name, numbers(&values)),
        _ => {
            let as_numbers = Column::numeric(name, Vec::new());
            let parsed: Option<Vec<CellValue>> = texts.iter().map(|t| as_numbers.parse_value(t).ok()).collect();
            match parsed {
                Some(values) => Column::numeric(name, numbers(&values)),
                None => Column::text(name, texts.to_vec()),
            }
        }
    };
    column.level = if column.kind() == ColumnKind::Text && like.level == MeasurementLevel::Continuous {
        MeasurementLevel::Nominal
    } else {
        like.level
    };
    column
}

fn kind_name(kind: ColumnKind) -> &'static str {
    match kind {
        ColumnKind::Numeric => "a number column",
        ColumnKind::Text => "a text column",
        ColumnKind::DateTime(_) => "a date column",
    }
}

fn invalid(message: String) -> StatError {
    StatError::InvalidArgument { message }
}

/// Text helpers offered by the Recode dialog for the new values.
pub mod helpers {
    /// Leading and trailing spaces removed, and inner runs of spaces made single.
    #[must_use]
    pub fn trim_spaces(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Each word with an upper-case first letter and the rest lower case.
    #[must_use]
    pub fn title_case(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut start = true;
        for c in text.chars() {
            if c.is_alphanumeric() {
                if start {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
                start = false;
            } else {
                out.push(c);
                start = true;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::RowStateManager;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::text(
                    "site",
                    vec!["north".into(), " North".into(), "south".into(), String::new(), "north".into()],
                ),
                Column::numeric("dose", vec![10.0, 2.0, 10.0, f64::NAN, 2.0]),
            ],
        )
        .unwrap()
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
    }

    #[test]
    fn distinct_values_are_counted_and_ordered() {
        let t = table();
        let site = distinct_values(&t, 0).unwrap();
        let texts: Vec<&str> = site.iter().map(|v| v.text.as_str()).collect();
        assert_eq!(texts, vec![" North", "north", "south", ""]);
        assert_eq!(site[1].count, 2);
        let dose = distinct_values(&t, 1).unwrap();
        let texts: Vec<&str> = dose.iter().map(|v| v.text.as_str()).collect();
        assert_eq!(texts, vec!["2", "10", ""]);
        assert!(distinct_values(&t, 5).is_err());
    }

    /// F-25: recoding a date column (shown without its time of day) into a
    /// new column keeps the original instants of the rows not recoded.
    #[test]
    fn recode_into_a_new_column_keeps_unchanged_date_times() {
        let display = stat_data::TemporalDisplay::new(stat_data::TemporalFormat::Date);
        let t = DataTable::new("t", vec![Column::date_time("when", vec![1_700_000_000.0, 86_400.0], display)]).unwrap();
        let (edit, changed) =
            recode_edit(&t, 0, &map(&[("1999-01-01", "2000-01-01")]), &RecodeTarget::NewColumn("when 2".into()))
                .unwrap();
        assert_eq!(changed, 0);
        let Edit::InsertColumn { column, .. } = edit else { panic!("expected a new column") };
        assert_eq!(column.as_numeric().unwrap(), &[1_700_000_000.0, 86_400.0]);
        assert_eq!(column.kind(), ColumnKind::DateTime(display));
    }

    #[test]
    fn recode_in_place_merges_and_undoes() {
        let mut t = table();
        let mut rows = RowStateManager::new(t.n_rows());
        let (edit, changed) =
            recode_edit(&t, 0, &map(&[(" North", "North"), ("north", "North")]), &RecodeTarget::InPlace).unwrap();
        assert_eq!(changed, 3);
        edit.apply(&mut t, &mut rows).unwrap();
        let col = &t.columns()[0];
        assert_eq!((0..5).map(|r| col.cell_text(r)).collect::<Vec<_>>(), ["North", "North", "south", "", "North"]);
        // Text cannot go into a numeric column in place.
        assert!(recode_edit(&t, 1, &map(&[("2", "low")]), &RecodeTarget::InPlace).is_err());
        assert!(recode_edit(&t, 0, &map(&[]), &RecodeTarget::InPlace).is_err());
    }

    #[test]
    fn recode_into_a_new_column_picks_its_type() {
        let mut t = table();
        let mut rows = RowStateManager::new(t.n_rows());
        let target = RecodeTarget::NewColumn("dose group".into());
        let (edit, _) = recode_edit(&t, 1, &map(&[("2", "low"), ("10", "high")]), &target).unwrap();
        edit.apply(&mut t, &mut rows).unwrap();
        let col = &t.columns()[2];
        assert_eq!(col.name, "dose group");
        assert_eq!(col.kind(), ColumnKind::Text);
        assert_eq!(col.level, MeasurementLevel::Nominal);
        assert_eq!(col.cell_text(0), "high");
        // Numbers stay numbers.
        let target = RecodeTarget::NewColumn("dose mg".into());
        let (edit, _) = recode_edit(&t, 1, &map(&[("2", "2000"), ("10", "10000")]), &target).unwrap();
        edit.apply(&mut t, &mut rows).unwrap();
        assert_eq!(t.columns()[2].kind(), ColumnKind::Numeric);
        // A taken or empty name is refused.
        assert!(recode_edit(&t, 1, &map(&[]), &RecodeTarget::NewColumn("dose".into())).is_err());
        assert!(recode_edit(&t, 1, &map(&[]), &RecodeTarget::NewColumn("  ".into())).is_err());
    }

    #[test]
    fn helpers_tidy_text() {
        assert_eq!(helpers::trim_spaces("  New   York "), "New York");
        assert_eq!(helpers::title_case("nEW york-city"), "New York-City");
    }
}
