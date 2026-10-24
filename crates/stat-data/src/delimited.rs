//! Delimited text (CSV, tab-separated, semicolon- or pipe-separated): reading
//! with delimiter detection and column type inference, and writing.
//!
//! # Reading
//! * The first record is the header.
//! * Fields may be quoted with `"`; a doubled `""` inside quotes is a literal
//!   quote, and quoted fields may contain delimiters and line breaks (RFC 4180).
//! * Line endings may be `\n` or `\r\n`; a leading byte-order mark is ignored.
//! * A field is *blank* if it is empty or a missing marker such as `NA`,
//!   `N/A`, `NaN` or `null` ([`crate::table::MISSING_MARKERS`], the defaults
//!   of pandas' `read_csv`). A column is numeric if every non-blank field
//!   parses as a number; blank fields in a numeric column become `NaN`
//!   (missing). Otherwise it is a date-time column if every non-blank field
//!   reads as a date, a time of day or an ISO 8601 duration
//!   ([`crate::temporal::detect_column`]); otherwise it is text, and keeps
//!   every field (markers included) as written.
//! * [`detect_delimiter`] picks the candidate (comma, tab, semicolon, pipe) that
//!   splits the first lines into the same number of fields (more than one).
//!
//! # Writing
//! Fields that contain the delimiter, a quote or a line break are quoted; numbers
//! are written in Rust's shortest round-trip form, so reading the file back
//! reproduces every value exactly. Date-time values are written as ISO 8601
//! text ([`crate::temporal::TemporalDisplay::iso_text`]). Missing values are
//! written as empty fields.

use crate::table::{Column, ColumnData, DataTable, format_number, is_missing_marker};
use crate::temporal::detect_column;
use stat_core::{Result, StatError};

/// Delimiters tried by [`detect_delimiter`], in order of preference on ties.
pub const CANDIDATE_DELIMITERS: [char; 4] = [',', '\t', ';', '|'];

/// Parse CSV (comma-separated) text into a [`DataTable`] named `name`.
///
/// # Errors
/// As for [`read_delimited`].
pub fn read_csv(name: &str, text: &str) -> Result<DataTable> {
    read_delimited(name, text, Some(','))
}

/// Parse delimited text into a [`DataTable`]. With `delimiter = None` the
/// delimiter is detected with [`detect_delimiter`].
///
/// # Errors
/// [`StatError::InvalidArgument`] for an empty input, an unterminated quoted field,
/// or a record with the wrong number of fields. Blank and repeated header
/// names are renamed (see [`unique_names`]), with a note.
pub fn read_delimited(name: &str, text: &str, delimiter: Option<char>) -> Result<DataTable> {
    read_delimited_with_notes(name, text, delimiter).map(|(table, _)| table)
}

/// [`read_delimited`], also returning notes about how values were read (see
/// [`table_from_records_with_notes`]).
///
/// # Errors
/// As for [`read_delimited`].
pub fn read_delimited_with_notes(name: &str, text: &str, delimiter: Option<char>) -> Result<(DataTable, Vec<String>)> {
    let delimiter = delimiter.unwrap_or_else(|| detect_delimiter(text));
    let mut records = parse_records(text, delimiter)?.into_iter();
    let header = records.next().ok_or_else(|| invalid("the file is empty"))?;
    let body: Vec<Vec<String>> = records.filter(|r| !(r.len() == 1 && r[0].is_empty())).collect();
    table_from_records_with_notes(name, header, body)
}

/// Build a table from a header and rows of text fields, inferring each column's
/// type as described in the module documentation. Used by every text-based
/// importer (and by spreadsheet import, after cells are rendered as text).
///
/// Blank and repeated header names are renamed with [`unique_names`].
///
/// # Errors
/// [`StatError::InvalidArgument`] if a row's length differs from the header's.
pub fn table_from_records(name: &str, header: Vec<String>, rows: Vec<Vec<String>>) -> Result<DataTable> {
    table_from_records_with_notes(name, header, rows).map(|(table, _)| table)
}

/// [`table_from_records`], also returning one note per column whose slash
/// dates (such as `03/04/2026`) could be read either way round and were read
/// month first, one per number or date column in which missing markers
/// such as `NA` were read as missing, and one listing renamed header names.
///
/// # Errors
/// As for [`table_from_records`].
pub fn table_from_records_with_notes(
    name: &str,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
) -> Result<(DataTable, Vec<String>)> {
    let width = header.len();
    let mut fields: Vec<Vec<String>> = vec![Vec::with_capacity(rows.len()); width];
    for (i, rec) in rows.into_iter().enumerate() {
        if rec.len() != width {
            return Err(invalid(format!("row {} has {} fields, header has {width}", i + 2, rec.len())));
        }
        for (col, value) in fields.iter_mut().zip(rec) {
            col.push(value);
        }
    }
    let mut notes = Vec::new();
    let named = unique_names(header.clone());
    if let Some(note) = renamed_note(&header, &named) {
        notes.push(note);
    }
    let columns = named
        .into_iter()
        .zip(fields)
        .map(|(name, values)| {
            let example = values.iter().find(|v| is_missing_marker(v)).map(|v| v.trim().to_owned());
            let (column, ambiguous, markers) = infer_column(name.trim(), values);
            if ambiguous {
                notes.push(month_first_note(&column.name));
            }
            if let Some(example) = example.filter(|_| markers > 0) {
                notes.push(missing_markers_note(&column.name, markers, &example));
            }
            column
        })
        .collect();
    Ok((DataTable::new(name, columns)?, notes))
}

/// Column names made usable: surrounding spaces are trimmed, a blank name
/// becomes `Column N` (its 1-based position) and a repeated name gets a
/// ` (2)`, ` (3)`, … suffix, so every name is non-empty and unique. Every
/// importer applies this to the header it reads.
#[must_use]
pub fn unique_names(raw: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for (j, name) in raw.into_iter().enumerate() {
        let base = if name.trim().is_empty() { format!("Column {}", j + 1) } else { name.trim().to_owned() };
        let mut candidate = base.clone();
        let mut k = 2;
        while out.contains(&candidate) {
            candidate = format!("{base} ({k})");
            k += 1;
        }
        out.push(candidate);
    }
    out
}

/// The note shown when [`unique_names`] had to rename blank or repeated
/// names (`None` when every name was kept, apart from trimming).
#[must_use]
pub fn renamed_note(raw: &[String], named: &[String]) -> Option<String> {
    let changes: Vec<String> = raw
        .iter()
        .zip(named)
        .enumerate()
        .filter(|(_, (r, n))| r.trim() != n.as_str())
        .map(|(j, (r, n))| {
            if r.trim().is_empty() {
                format!("column {} had no name and is called {n:?}", j + 1)
            } else {
                format!("column {} repeated the name {:?} and is called {n:?}", j + 1, r.trim())
            }
        })
        .collect();
    (!changes.is_empty()).then(|| format!("Renamed so every column has its own name: {}.", changes.join("; ")))
}

/// The note shown when a column's slash dates were read month first because
/// the values allowed either order.
#[must_use]
pub fn month_first_note(column: &str) -> String {
    format!(
        "Column {column:?}: dates such as 03/04/2026 could be month or day first and were read as \
         month/day/year. If they are day first, choose Swap day and month in the column's type menu."
    )
}

/// A numeric column if every value that is neither blank nor a
/// [missing marker](is_missing_marker) is a number, else a date-time column if
/// every such value is a date, time or duration, else a text column (which
/// keeps markers as text). Returns the column, whether slash dates were read
/// month first by default, and how many markers were read as missing.
fn infer_column(name: &str, values: Vec<String>) -> (Column, bool, usize) {
    let blank = |v: &String| v.trim().is_empty() || is_missing_marker(v);
    let markers = values.iter().filter(|v| !v.trim().is_empty() && is_missing_marker(v)).count();
    if !values.iter().any(|v| !blank(v)) {
        return (Column::text(name, values), false, 0);
    }
    let numeric: Option<Vec<f64>> = values
        .iter()
        .map(|v| if blank(v) { Some(f64::NAN) } else { v.trim().parse::<f64>().ok().filter(|x| x.is_finite()) })
        .collect();
    if let Some(v) = numeric {
        return (Column::numeric(name, v), false, markers);
    }
    let cleaned: Vec<&str> = values.iter().map(|v| if blank(v) { "" } else { v.as_str() }).collect();
    match detect_column(&cleaned) {
        Some(d) => (Column::date_time(name, d.values, d.display), d.ambiguous_order, markers),
        None => (Column::text(name, values), false, 0),
    }
}

/// The note shown when `count` missing markers in `column` were read as
/// missing values.
#[must_use]
pub fn missing_markers_note(column: &str, count: usize, example: &str) -> String {
    let values = if count == 1 { "value" } else { "values" };
    format!("Column {column:?}: {count} {values} such as {example:?} were read as missing.")
}

/// The delimiter that splits the first lines of `text` most consistently: every
/// record has the same number of fields, more than one, preferring more fields
/// and then the order of [`CANDIDATE_DELIMITERS`]. Falls back to a comma.
#[must_use]
pub fn detect_delimiter(text: &str) -> char {
    // Look at the first ~20 lines (enough to see the header and some data),
    // extended to the end of any quoted field still open there: an even number
    // of quotes so far means no quoted field is open (a doubled `""` adds two).
    let mut lines = Vec::new();
    let mut quotes = 0usize;
    for line in text.lines() {
        quotes += line.matches('"').count();
        lines.push(line);
        if lines.len() >= 20 && quotes % 2 == 0 {
            break;
        }
    }
    let sample = lines.join("\n");
    let mut best: Option<(char, usize)> = None;
    for &d in &CANDIDATE_DELIMITERS {
        let Ok(records) = parse_records(&sample, d) else { continue };
        let records: Vec<_> = records.into_iter().filter(|r| !(r.len() == 1 && r[0].is_empty())).collect();
        let Some(width) = records.first().map(Vec::len) else { continue };
        if width > 1 && records.iter().all(|r| r.len() == width) && best.is_none_or(|(_, w)| width > w) {
            best = Some((d, width));
        }
    }
    best.map_or(',', |(d, _)| d)
}

/// Render `table` as delimited text with a header row, `\n` line endings, and
/// a trailing newline.
#[must_use]
pub fn write_delimited(table: &DataTable, delimiter: char) -> String {
    let mut out = String::new();
    let header: Vec<String> = table.columns().iter().map(|c| quote(&c.name, delimiter)).collect();
    out.push_str(&header.join(&delimiter.to_string()));
    out.push('\n');
    for r in 0..table.n_rows() {
        let row: Vec<String> = table
            .columns()
            .iter()
            .map(|c| match &c.data {
                ColumnData::Numeric(v) => {
                    if v[r].is_nan() {
                        String::new()
                    } else {
                        format_number(v[r])
                    }
                }
                ColumnData::Text(v) => quote(&v[r], delimiter),
                ColumnData::DateTime { values, display } => display.iso_text(values[r]),
            })
            .collect();
        out.push_str(&row.join(&delimiter.to_string()));
        out.push('\n');
    }
    out
}

/// Quote a field if it contains the delimiter, a quote, a line break, or
/// leading/trailing spaces (which a reader might otherwise trim).
fn quote(field: &str, delimiter: char) -> String {
    let needs = field.contains(delimiter)
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r')
        || field.trim() != field;
    if needs { format!("\"{}\"", field.replace('"', "\"\"")) } else { field.to_owned() }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Split delimited text into records of fields, following the quoting rules
/// in the module documentation. Every line is a record (blank lines included);
/// no header is assumed.
///
/// # Errors
/// [`StatError::InvalidArgument`] for an unterminated quoted field.
pub fn parse_records(text: &str, delimiter: char) -> Result<Vec<Vec<String>>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => in_quotes = true,
            c if c == delimiter => record.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }
    if in_quotes {
        return Err(invalid("unterminated quoted field"));
    }
    if any && (!field.is_empty() || !record.is_empty()) {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::MeasurementLevel;

    #[test]
    fn infers_types_and_missing_values() {
        let t = read_csv("d", "x,group,y\r\n1,a,2.5\r\n2,b,\r\n3,\"c, d\",4e1\r\n").unwrap();
        assert_eq!(t.n_rows(), 3);
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap(), &[1.0, 2.0, 3.0]);
        let y = t.column(2).unwrap().as_numeric().unwrap();
        assert!(y[1].is_nan());
        assert_eq!(y[2], 40.0);
        let g = t.column(1).unwrap();
        assert_eq!(g.level, MeasurementLevel::Nominal);
        assert_eq!(g.data, ColumnData::Text(vec!["a".into(), "b".into(), "c, d".into()]));
    }

    #[test]
    fn missing_markers_are_missing_in_number_and_date_columns_only() {
        let text = "x,when,region\n1,2026-03-04,NA\nNA,N/A,US\n nan ,null,NA\n4,2026-03-05,EU\n";
        let (t, notes) = read_delimited_with_notes("d", text, None).unwrap();
        let x = t.column(0).unwrap().as_numeric().unwrap();
        assert_eq!(x[0], 1.0);
        assert!(x[1].is_nan() && x[2].is_nan());
        assert_eq!(x[3], 4.0);
        let when = t.column(1).unwrap();
        assert!(matches!(when.data, ColumnData::DateTime { .. }));
        assert_eq!(when.cell_text(1), "");
        // A text column keeps the markers as text.
        assert_eq!(t.column(2).unwrap().cell_text(0), "NA");
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("\"x\": 2 values such as \"NA\""), "{notes:?}");
        // A column of markers only is text; other spellings are not markers.
        let t = read_csv("d", "a,b\nNA,na\nnull,3\n").unwrap();
        assert!(t.column(0).unwrap().as_numeric().is_none());
        assert!(t.column(1).unwrap().as_numeric().is_none());
    }

    #[test]
    fn quotes_newlines_and_bom() {
        let t = read_csv("d", "\u{feff}name,note\n\"A\",\"say \"\"hi\"\"\nthere\"\n").unwrap();
        assert_eq!(t.column(1).unwrap().cell_text(0), "say \"hi\"\nthere");
        assert_eq!(t.column(0).unwrap().name, "name");
    }

    #[test]
    fn no_trailing_newline_and_blank_lines() {
        let t = read_csv("d", "a,b\n1,2\n\n3,4").unwrap();
        assert_eq!(t.n_rows(), 2);
    }

    #[test]
    fn errors() {
        assert!(read_csv("d", "").is_err());
        assert!(read_csv("d", "a,b\n1\n").is_err());
        assert!(read_csv("d", "a\n\"open\n").is_err());
    }

    #[test]
    fn detects_delimiters() {
        assert_eq!(detect_delimiter("a\tb\tc\n1\t2\t3\n"), '\t');
        assert_eq!(detect_delimiter("a;b\n1,5;2,5\n"), ';', "European decimal commas");
        assert_eq!(detect_delimiter("a|b\n1|2\n"), '|');
        assert_eq!(detect_delimiter("a,b\n1,2\n"), ',');
        assert_eq!(detect_delimiter("just one column\n1\n"), ',', "fallback");
        // A comma inside quotes does not fool the tab detection.
        assert_eq!(detect_delimiter("name\tnote\nx\t\"a, b\"\n"), '\t');
        let t = read_delimited("d", "a\tb\n1\t2\n", None).unwrap();
        assert_eq!(t.column(1).unwrap().as_numeric().unwrap(), &[2.0]);
    }

    /// A quoted field that spans line 20 of the file used to cut the detection
    /// sample inside the quotes, so no delimiter parsed and the comma fallback
    /// broke the import of a valid tab-separated file.
    #[test]
    fn detection_sample_ends_outside_quotes() {
        let mut text = String::from("name\tnote\n");
        for i in 0..18 {
            text.push_str(&format!("r{i}\tplain\n"));
        }
        text.push_str("r18\t\"line one\nline two, with comma\nline three\"\n");
        for i in 19..25 {
            text.push_str(&format!("r{i}\tplain\n"));
        }
        assert_eq!(detect_delimiter(&text), '\t');
        let t = read_delimited("t", &text, None).unwrap();
        assert_eq!((t.n_rows(), t.n_cols()), (25, 2));
        assert_eq!(t.column(1).unwrap().cell_text(18), "line one\nline two, with comma\nline three");
    }

    #[test]
    fn write_then_read_round_trips_exactly() {
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("x, with comma", vec![0.1, -2.5e-300, f64::NAN, 1e21]),
                Column::text("note", vec!["plain".into(), "has \"quotes\"".into(), "a\tb".into(), " padded ".into()]),
                Column::text("code", vec!["007".into(), "x".into(), String::new(), "y".into()]),
            ],
        )
        .unwrap();
        for d in [',', '\t', ';'] {
            let text = write_delimited(&t, d);
            let back = read_delimited("t", &text, Some(d)).unwrap();
            assert_eq!(back.column(0).unwrap().name, "x, with comma");
            let (a, b) = (t.column(0).unwrap().as_numeric().unwrap(), back.column(0).unwrap().as_numeric().unwrap());
            for (x, y) in a.iter().zip(b) {
                assert!(x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()), "{x} vs {y} ({d:?})");
            }
            assert_eq!(back.column(1).unwrap().data, t.column(1).unwrap().data, "delimiter {d:?}");
            // Detection finds the delimiter that was written.
            assert_eq!(detect_delimiter(&text), d);
        }
    }

    /// A repeated or blank header name used to refuse to open ("duplicate
    /// column name") although a workbook with the same header opened.
    #[test]
    fn repeated_and_blank_names_are_renamed_with_a_note() {
        let (t, notes) = read_delimited_with_notes("d", "a,a,\n1,2,3\n", Some(',')).unwrap();
        let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "a (2)", "Column 3"]);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("\"a (2)\"") && notes[0].contains("Column 3"), "{}", notes[0]);
        let (t, notes) = read_delimited_with_notes("d", " x ,y\n1,2\n", None).unwrap();
        assert_eq!(t.column(0).unwrap().name, "x");
        assert!(notes.is_empty(), "trimming alone is not worth a note");
        assert_eq!(
            unique_names(vec![String::new(), "a".into(), "a".into(), "a (2)".into()]),
            ["Column 1", "a", "a (2)", "a (2) (2)"]
        );
    }

    #[test]
    fn table_from_records_checks_widths() {
        let ok = table_from_records("t", vec!["a".into(), "b".into()], vec![vec!["1".into(), "x".into()]]).unwrap();
        assert_eq!(ok.n_cols(), 2);
        assert!(table_from_records("t", vec!["a".into()], vec![vec!["1".into(), "2".into()]]).is_err());
    }

    #[test]
    fn date_columns_are_detected_and_round_trip() {
        let text = "when,at,took,id\n2026-10-13,2026-10-13T15:57:58.25+01:00,PT1H30M,10/13/26\n\
                    ,2026-10-14 08:00,PT0S,\n2026-10-15,,-PT10M,03/04/26\n";
        let (t, notes) = read_delimited_with_notes("t", text, None).unwrap();
        assert!(notes.is_empty(), "13 settles the order: {notes:?}");
        let kinds: Vec<_> = t.columns().iter().map(|c| c.temporal().map(|d| d.format)).collect();
        use crate::temporal::TemporalFormat as F;
        assert_eq!(kinds, [Some(F::Date), Some(F::DateTime), Some(F::Duration), Some(F::Date)]);
        assert_eq!(t.column(1).unwrap().temporal().unwrap().offset_minutes, 60);
        assert_eq!(t.column(1).unwrap().cell_text(1), "2026-10-14 08:00:00.00");
        assert_eq!(t.column(2).unwrap().cell_text(2), "-00:10:00");
        assert_eq!(t.column(3).unwrap().cell_text(2), "2026-03-04");
        assert!(t.column(0).unwrap().as_numeric().unwrap()[1].is_nan());
        let written = write_delimited(&t, ',');
        assert!(written.starts_with("when,at,took,id\n2026-10-13,2026-10-13T15:57:58.25+01:00,PT1H30M,2026-10-13\n"));
        assert_eq!(read_csv("t", &written).unwrap(), t);
        // Undecidable slash dates are read month first, with a note.
        let (t, notes) = read_delimited_with_notes("t", "d\n03/04/2026\n05/06/2026\n", None).unwrap();
        assert_eq!(t.column(0).unwrap().cell_text(0), "2026-03-04");
        assert_eq!(notes, [month_first_note("d")]);
        // Numbers stay numbers, and mixed columns stay text.
        let t = read_csv("t", "a,b\n2026,2026-10-13\n2027,soon\n").unwrap();
        assert!(t.column(0).unwrap().temporal().is_none() && t.column(0).unwrap().as_numeric().is_some());
        assert!(t.column(1).unwrap().as_numeric().is_none());
    }
}
