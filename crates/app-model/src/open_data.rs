//! Choosing which block of a sheet becomes a table, for the Open Data dialog,
//! file opening, web tables, pasted text and the script `open_table` options.
//!
//! A file is first read as a [`RawSheet`]: every cell as text, rows and
//! columns numbered from 1 as in the dialog's preview, nothing taken as a
//! header yet. A [`Block`] then says where the table sits:
//!
//! * **Header row** — the row holding the column names, or none (the columns
//!   are then called `Column 1`, `Column 2`, …).
//! * **Header rows** — how many rows, from the header row down, hold names.
//!   Their texts are joined with a space (`Temperature` over `Min` gives
//!   `Temperature Min`). A blank cell in an upper header row takes the text to
//!   its left, as a merged heading cell spans the columns under it.
//! * **Data starts on row** / **Data ends on row** — the rows read as values;
//!   blank rows among them are skipped.
//! * **First column** / **Last column** — the columns read; columns blank in
//!   every header and data row are dropped.
//!
//! [`detect_block`] proposes a block: the largest run of consecutive non-blank
//! rows (runs above or below it of similar width join it; a narrow run such
//! as a title or a footnote does not), started at its first row that is at
//! least half as wide as the table. That row is the header unless it holds
//! numbers and no column has text in it above numbers below. The dialog shows
//! this as a starting point only.
//!
//! [`ValueOptions`] add a decimal comma (`1,5` read as 1.5, and `1.234,5` as
//! 1234.5, for a column whose every value reads that way) and extra missing
//! markers. Names are then made unique and non-blank
//! ([`stat_data::unique_names`]) and the values typed exactly as delimited
//! text is ([`stat_data::table_from_records_with_notes`]).

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, is_missing_marker, parse_records, table_from_records_with_notes};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// A sheet of cells as text, before any row is taken as the header: one
/// workbook sheet, a text file split at its delimiter, a web-page table or
/// pasted text. Rows may differ in length; missing cells are blank.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawSheet {
    /// The sheet's name (the file name for a text file).
    pub name: String,
    /// The cells, row by row.
    pub cells: Vec<Vec<String>>,
}

impl RawSheet {
    /// A sheet named `name` holding `cells`.
    #[must_use]
    pub fn new(name: impl Into<String>, cells: Vec<Vec<String>>) -> Self {
        Self { name: name.into(), cells }
    }

    /// Number of rows.
    #[must_use]
    pub fn n_rows(&self) -> usize {
        self.cells.len()
    }

    /// Number of columns: the length of the longest row.
    #[must_use]
    pub fn n_cols(&self) -> usize {
        self.cells.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// The text at 0-based `row` and `col`; blank outside the sheet.
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> &str {
        self.cells.get(row).and_then(|r| r.get(col)).map_or("", String::as_str)
    }

    /// Whether the sheet has no non-blank cell.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.cells.iter().all(|r| r.iter().all(|c| c.trim().is_empty()))
    }

    fn row_count(&self, row: usize) -> usize {
        self.cells.get(row).map_or(0, |r| r.iter().filter(|c| !c.trim().is_empty()).count())
    }
}

/// What a cell of the preview is, under a [`Block`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellRole {
    /// Part of a column name.
    Header,
    /// A value of the table.
    Data,
    /// Not read.
    Outside,
}

/// Where the table sits in a [`RawSheet`]. Rows and columns are numbered
/// from 1, as the preview shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// The row with the column names; `None` when the sheet has none.
    pub header_row: Option<usize>,
    /// How many rows, from `header_row` down, hold names (at least 1).
    pub header_rows: usize,
    /// The first row of values.
    pub data_start: usize,
    /// The last row of values; `None` reads to the end of the sheet.
    pub data_end: Option<usize>,
    /// The first column read.
    pub first_column: usize,
    /// The last column read; `None` reads to the longest row's end.
    pub last_column: Option<usize>,
}

impl Default for Block {
    /// Names in row 1, values from row 2, every column.
    fn default() -> Self {
        Self { header_row: Some(1), header_rows: 1, data_start: 2, data_end: None, first_column: 1, last_column: None }
    }
}

impl Block {
    /// The last header row, when there is a header.
    #[must_use]
    pub fn header_end(&self) -> Option<usize> {
        self.header_row.map(|h| h + self.header_rows.max(1) - 1)
    }

    /// Whether the table starts in the top-left cell with one row of names
    /// and runs to the sheet's last row and column, so that reading the
    /// sheet needs no choices.
    #[must_use]
    pub fn is_plain(&self, sheet: &RawSheet) -> bool {
        self.header_row == Some(1)
            && self.header_rows == 1
            && self.data_start == 2
            && self.data_end.is_none_or(|e| e >= sheet.n_rows())
            && self.first_column == 1
            && self.last_column.is_none_or(|l| l >= sheet.n_cols())
    }

    /// Make the names start at `row` (or remove them), moving the first
    /// data row below the names when it would fall among them.
    pub fn set_header_row(&mut self, row: Option<usize>) {
        self.header_row = row.map(|r| r.max(1));
        if let Some(end) = self.header_end() {
            if self.data_start <= end {
                self.data_start = end + 1;
            }
        }
    }

    /// Make the values start at `row`, moving the names above it when they
    /// would overlap.
    pub fn set_data_start(&mut self, row: usize) {
        self.data_start = row.max(1);
        if let (Some(h), Some(end)) = (self.header_row, self.header_end()) {
            if end >= self.data_start {
                if h >= self.data_start {
                    self.header_row = None;
                } else {
                    self.header_rows = self.data_start - h;
                }
            }
        }
        if self.data_end.is_some_and(|e| e < self.data_start) {
            self.data_end = None;
        }
    }

    /// What the cell at 1-based `row`, `col` is under this block.
    #[must_use]
    pub fn role(&self, row: usize, col: usize) -> CellRole {
        let in_columns = col >= self.first_column && self.last_column.is_none_or(|l| col <= l);
        if !in_columns {
            return CellRole::Outside;
        }
        if let (Some(h), Some(end)) = (self.header_row, self.header_end()) {
            if (h..=end).contains(&row) {
                return CellRole::Header;
            }
        }
        if row >= self.data_start && self.data_end.is_none_or(|e| row <= e) {
            CellRole::Data
        } else {
            CellRole::Outside
        }
    }

    /// A one-line description, such as `names in row 4, values from row 5,
    /// columns 3 to 7`.
    #[must_use]
    pub fn describe(&self) -> String {
        let names = match (self.header_row, self.header_rows) {
            (None, _) => "no names (Column 1, Column 2, …)".to_owned(),
            (Some(h), 1) => format!("names in row {h}"),
            (Some(h), n) => format!("names in rows {h} to {}", h + n - 1),
        };
        let rows = match self.data_end {
            Some(e) => format!("values in rows {} to {e}", self.data_start),
            None => format!("values from row {}", self.data_start),
        };
        let columns = match self.last_column {
            Some(l) => format!("columns {} to {l}", self.first_column),
            None if self.first_column == 1 => "all columns".to_owned(),
            None => format!("columns from {}", self.first_column),
        };
        format!("{names}, {rows}, {columns}")
    }
}

/// How values are read, beyond the standard rules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValueOptions {
    /// Numbers are written with a decimal comma (`1,5`), and possibly
    /// points between thousands (`1.234,5`).
    pub decimal_comma: bool,
    /// Extra texts read as missing values (the standard markers such as `NA`
    /// always are, in number and date columns).
    pub missing: Vec<String>,
}

/// Everything needed to read a table from a sheet: the block, the value
/// options and, for text, the delimiter. Stored in projects (web tables) and
/// accepted by the script function `open_table` as a map.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenSettings {
    /// Where the table sits.
    pub block: Block,
    /// How values are read.
    pub values: ValueOptions,
    /// The delimiter of a text file; `None` to detect it.
    pub delimiter: Option<char>,
}

impl OpenSettings {
    /// The project-file and script form: `header_row` (0 for none),
    /// `header_rows`, `data_start`, `data_end` and `last_column` (0 for the
    /// end), `first_column`, `delimiter` (`""` to detect, `"tab"` for a tab),
    /// `decimal` (`"."` or `","`) and `missing` (a list of texts).
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let n = |x: usize| JsonValue::Number(x as f64);
        let s = |t: &str| JsonValue::String(t.to_owned());
        let b = &self.block;
        let delimiter = match self.delimiter {
            None => String::new(),
            Some('\t') => "tab".to_owned(),
            Some(c) => c.to_string(),
        };
        JsonValue::Object(vec![
            ("header_row".into(), n(b.header_row.unwrap_or(0))),
            ("header_rows".into(), n(b.header_rows)),
            ("data_start".into(), n(b.data_start)),
            ("data_end".into(), n(b.data_end.unwrap_or(0))),
            ("first_column".into(), n(b.first_column)),
            ("last_column".into(), n(b.last_column.unwrap_or(0))),
            ("delimiter".into(), s(&delimiter)),
            ("decimal".into(), s(if self.values.decimal_comma { "," } else { "." })),
            ("missing".into(), JsonValue::Array(self.values.missing.iter().map(|m| s(m)).collect())),
        ])
    }

    /// Read the form [`to_json`](Self::to_json) writes. Members left out
    /// take their values from `base` (for example the detected block), so a
    /// script can give only `header_row`.
    ///
    /// # Errors
    /// A member of the wrong type, a row or column number that is not a
    /// whole number, a delimiter longer than one character, or a decimal
    /// mark other than `.` and `,`.
    pub fn from_json(v: &JsonValue, base: &Self) -> Result<Self> {
        let JsonValue::Object(members) = v else { return Err(invalid("the open settings must be a map")) };
        let mut out = base.clone();
        let mut header_row_given = false;
        let mut data_start_given = false;
        for (key, value) in members {
            let whole = |what: &str| -> Result<usize> {
                match value {
                    JsonValue::Number(x) if *x >= 0.0 && x.fract() == 0.0 && *x < 1e9 => Ok(*x as usize),
                    _ => Err(invalid(format!("{what} must be a whole number of 0 or more"))),
                }
            };
            let text =
                |what: &str| -> Result<&str> { value.as_str().ok_or_else(|| invalid(format!("{what} must be text"))) };
            match key.as_str() {
                "header_row" => {
                    out.block.header_row = Some(whole(key)?).filter(|&r| r > 0);
                    header_row_given = true;
                }
                "header_rows" => out.block.header_rows = whole(key)?.max(1),
                "data_start" => {
                    out.block.data_start = whole(key)?.max(1);
                    data_start_given = true;
                }
                "data_end" => out.block.data_end = Some(whole(key)?).filter(|&r| r > 0),
                "first_column" => out.block.first_column = whole(key)?.max(1),
                "last_column" => out.block.last_column = Some(whole(key)?).filter(|&c| c > 0),
                "delimiter" => {
                    let d = text(key)?;
                    out.delimiter = match d {
                        "" => None,
                        "tab" | "\t" => Some('\t'),
                        _ => {
                            let mut chars = d.chars();
                            match (chars.next(), chars.next()) {
                                (Some(c), None) => Some(c),
                                _ => return Err(invalid("delimiter must be one character, \"tab\" or \"\"")),
                            }
                        }
                    };
                }
                "decimal" => {
                    out.values.decimal_comma = match text(key)? {
                        "." => false,
                        "," => true,
                        _ => return Err(invalid("decimal must be \".\" or \",\"")),
                    };
                }
                "missing" => {
                    let items = value.as_array().ok_or_else(|| invalid("missing must be a list of texts"))?;
                    out.values.missing = items
                        .iter()
                        .map(|m| {
                            m.as_str().map(str::to_owned).ok_or_else(|| invalid("missing must be a list of texts"))
                        })
                        .collect::<Result<_>>()?;
                }
                // Keys read by the caller (`sheet`, `sheets`, `stack`, …).
                _ => {}
            }
        }
        // Giving only the header row puts the values right under the names.
        if header_row_given && !data_start_given {
            out.block.data_start = out.block.header_end().map_or(1, |e| e + 1);
        }
        Ok(out)
    }
}

/// Split `text` into a sheet at `delimiter` (detected when `None`; see
/// [`stat_data::detect_delimiter`]), returning the delimiter used. Every
/// line is a row, blank lines included, so row numbers match the file's
/// line numbers (apart from quoted line breaks).
///
/// # Errors
/// An unterminated quoted field.
pub fn split_text(name: &str, text: &str, delimiter: Option<char>) -> Result<(RawSheet, char)> {
    let delimiter = delimiter.unwrap_or_else(|| stat_data::detect_delimiter(text));
    let mut cells = parse_records(text, delimiter)?;
    while cells.last().is_some_and(|r| r.iter().all(|c| c.trim().is_empty())) {
        cells.pop();
    }
    Ok((RawSheet::new(name, cells), delimiter))
}

/// Whether the sheet's numbers look written with a decimal comma: some cells
/// read as such (`1,5`, `-0,25`) and none has a decimal point. Never for a
/// comma-delimited file, whose fields cannot hold a bare comma.
#[must_use]
pub fn detect_decimal_comma(sheet: &RawSheet, delimiter: Option<char>) -> bool {
    if delimiter == Some(',') {
        return false;
    }
    let (mut comma, mut point) = (0usize, 0usize);
    for cell in sheet.cells.iter().take(200).flatten() {
        let t = cell.trim();
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let body = t.strip_prefix(['-', '+']).unwrap_or(t);
        if let Some((a, b)) = body.split_once(',') {
            comma += usize::from(digits(a) && digits(b));
        } else if let Some((a, b)) = body.split_once('.') {
            point += usize::from(digits(a) && digits(b));
        }
    }
    comma > 0 && point == 0
}

fn is_number(text: &str) -> bool {
    text.trim().parse::<f64>().is_ok_and(f64::is_finite)
}

fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

/// Propose where the table sits in `sheet` (see the module documentation).
/// An empty sheet gives [`Block::default`].
#[must_use]
pub fn detect_block(sheet: &RawSheet) -> Block {
    let counts: Vec<usize> = (0..sheet.n_rows()).map(|r| sheet.row_count(r)).collect();
    // Runs of consecutive non-blank rows, 0-based and inclusive.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (r, &c) in counts.iter().enumerate() {
        match runs.last_mut() {
            Some((_, end)) if c > 0 && *end + 1 == r => *end = r,
            _ if c > 0 => runs.push((r, r)),
            _ => {}
        }
    }
    let total = |&(a, b): &(usize, usize)| counts[a..=b].iter().sum::<usize>();
    let widest = |&(a, b): &(usize, usize)| counts[a..=b].iter().copied().max().unwrap_or(0);
    let Some(main) = (0..runs.len()).max_by(|&i, &j| total(&runs[i]).cmp(&total(&runs[j])).then(j.cmp(&i))) else {
        return Block::default();
    };
    let width = widest(&runs[main]);
    let threshold = if width >= 2 { width.div_ceil(2).max(2) } else { 1 };
    // The columns a run of rows uses, first to last non-blank cell.
    let span = |&(a, b): &(usize, usize)| {
        let first = (a..=b).filter_map(|r| sheet.cells[r].iter().position(|c| !is_blank(c))).min().unwrap_or(0);
        let last = (a..=b).filter_map(|r| sheet.cells[r].iter().rposition(|c| !is_blank(c))).max().unwrap_or(0);
        (first, last)
    };
    let main_span = span(&runs[main]);
    // A run joins the table when it is about as wide and lies over its
    // columns (a heading block off to the side of the table does not).
    let joins = |run: &(usize, usize)| {
        let (a, b) = span(run);
        let overlap = (b.min(main_span.1) + 1).saturating_sub(a.max(main_span.0));
        widest(run) >= threshold && 2 * overlap > b - a
    };
    let (mut first_run, mut last_run) = (main, main);
    while first_run > 0 && joins(&runs[first_run - 1]) {
        first_run -= 1;
    }
    while last_run + 1 < runs.len() && joins(&runs[last_run + 1]) {
        last_run += 1;
    }
    let end = runs[last_run].1;
    let mut start = runs[first_run].0;
    // Skip title lines directly above the names.
    while counts[start] < threshold && start < end {
        start += 1;
    }
    let rows = start..=end;
    let first_col = rows.clone().filter_map(|r| sheet.cells[r].iter().position(|c| !is_blank(c))).min().unwrap_or(0);
    let last_col = rows.clone().filter_map(|r| sheet.cells[r].iter().rposition(|c| !is_blank(c))).max().unwrap_or(0);
    // The first row names the columns unless it holds numbers and no
    // column has text in it above numbers further down.
    let top: Vec<&str> = (first_col..=last_col).map(|c| sheet.cell(start, c)).collect();
    let holds_numbers = top.iter().any(|t| is_number(t));
    let text_over_numbers = (first_col..=last_col).any(|c| {
        let t = sheet.cell(start, c);
        let below =
            (start + 1..=end.min(start + 20)).map(|r| sheet.cell(r, c)).find(|v| !is_blank(v) && !is_missing_marker(v));
        !is_blank(t) && !is_number(t) && below.is_some_and(is_number)
    });
    let header = !holds_numbers || text_over_numbers;
    Block {
        header_row: header.then_some(start + 1),
        header_rows: 1,
        data_start: start + 1 + usize::from(header),
        data_end: (last_run + 1 < runs.len()).then_some(end + 1),
        first_column: first_col + 1,
        last_column: (last_col + 1 < sheet.n_cols()).then_some(last_col + 1),
    }
}

/// The settings [`detect_block`] and [`detect_decimal_comma`] propose for a
/// sheet split at `delimiter` (`None` for a sheet that is not text).
#[must_use]
pub fn detect_settings(sheet: &RawSheet, delimiter: Option<char>) -> OpenSettings {
    OpenSettings {
        block: detect_block(sheet),
        values: ValueOptions { decimal_comma: detect_decimal_comma(sheet, delimiter), missing: Vec::new() },
        delimiter,
    }
}

/// `text` read with a decimal comma, as Rust number text: `1,5` → `1.5`,
/// `-1.234.567,25` → `-1234567.25`; `None` if it is not such a number
/// (points must separate groups of three digits).
fn decimal_comma_number(text: &str) -> Option<String> {
    let t = text.trim();
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", t.strip_prefix('+').unwrap_or(t)),
    };
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let (whole, fraction) = mantissa.split_once(',').map_or((mantissa, None), |(w, f)| (w, Some(f)));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let groups: Vec<&str> = whole.split('.').collect();
    let grouped = groups.len() == 1 && digits(groups[0])
        || groups.len() > 1
            && digits(groups[0])
            && groups[0].len() <= 3
            && groups[1..].iter().all(|g| g.len() == 3 && digits(g));
    if !grouped || fraction.is_some_and(|f| !digits(f)) {
        return None;
    }
    let mut out = format!("{sign}{}", groups.concat());
    if let Some(f) = fraction {
        out.push('.');
        out.push_str(f);
    }
    if let Some(e) = exponent {
        let e_digits = e.strip_prefix(['-', '+']).unwrap_or(e);
        if !digits(e_digits) {
            return None;
        }
        out.push('e');
        out.push_str(e);
    }
    out.parse::<f64>().ok()?;
    Some(out)
}

/// What [`read_block`] returns: the column names, the rows of values (as
/// text) and notes on how they were read.
pub type BlockText = (Vec<String>, Vec<Vec<String>>, Vec<String>);

/// The column names and rows of values the settings pick from `sheet`
/// (names not yet made unique), with notes.
///
/// # Errors
/// A block that holds no values and no names.
pub fn read_block(sheet: &RawSheet, settings: &OpenSettings) -> Result<BlockText> {
    let b = &settings.block;
    let first = b.first_column.max(1) - 1;
    let header_rows: Vec<usize> = match (b.header_row, b.header_end()) {
        (Some(h), Some(e)) => (h - 1..e).filter(|&r| r < sheet.n_rows()).collect(),
        _ => Vec::new(),
    };
    let data_end = b.data_end.map_or(sheet.n_rows(), |e| e.min(sheet.n_rows()));
    let data_rows: Vec<usize> = (b.data_start.max(1) - 1..data_end)
        .filter(|&r| {
            sheet.cells[r]
                .iter()
                .skip(first)
                .take(b.last_column.map_or(usize::MAX, |l| l.saturating_sub(first)))
                .any(|c| !is_blank(c))
        })
        .collect();
    let last = match b.last_column {
        Some(l) => l,
        None => header_rows.iter().chain(&data_rows).map(|&r| sheet.cells[r].len()).max().unwrap_or(0),
    };
    if last <= first {
        return Err(invalid("the chosen columns hold nothing: check First column and Last column"));
    }
    let columns: Vec<usize> =
        (first..last).filter(|&c| header_rows.iter().chain(&data_rows).any(|&r| !is_blank(sheet.cell(r, c)))).collect();
    if columns.is_empty() {
        return Err(invalid("the chosen rows and columns hold nothing: check where the data starts"));
    }
    // Names: header rows joined with a space; an upper row's blank cell takes
    // the text to its left (a merged heading spanning several columns).
    let mut names = vec![String::new(); columns.len()];
    for (k, &r) in header_rows.iter().enumerate() {
        let upper = k + 1 < header_rows.len();
        let mut carried = "";
        for (name, &c) in names.iter_mut().zip(&columns) {
            let mut part = sheet.cell(r, c).trim();
            if upper {
                if part.is_empty() {
                    part = carried;
                } else {
                    carried = part;
                }
            }
            if !part.is_empty() {
                if !name.is_empty() {
                    name.push(' ');
                }
                name.push_str(part);
            }
        }
    }
    let missing: Vec<&str> = settings.values.missing.iter().map(|m| m.trim()).filter(|m| !m.is_empty()).collect();
    let mut short = 0usize;
    let mut rows: Vec<Vec<String>> = data_rows
        .iter()
        .map(|&r| {
            if sheet.cells[r].len() < last && b.last_column.is_none() {
                short += 1;
            }
            columns
                .iter()
                .map(|&c| {
                    let v = sheet.cell(r, c);
                    if missing.contains(&v.trim()) { String::new() } else { v.to_owned() }
                })
                .collect()
        })
        .collect();
    let mut notes = Vec::new();
    if settings.values.decimal_comma {
        for j in 0..columns.len() {
            let converted: Option<Vec<String>> = rows
                .iter()
                .map(|row| {
                    let v = &row[j];
                    if is_blank(v) || is_missing_marker(v) { Some(v.clone()) } else { decimal_comma_number(v) }
                })
                .collect();
            if let Some(values) = converted {
                for (row, v) in rows.iter_mut().zip(values) {
                    row[j] = v;
                }
            }
        }
    }
    if short > 0 && sheet.cells.iter().map(Vec::len).collect::<std::collections::BTreeSet<_>>().len() > 1 {
        let rows_word = if short == 1 { "row has" } else { "rows have" };
        notes
            .push(format!("{short} {rows_word} fewer fields than the widest row; the missing fields were left blank."));
    }
    Ok((names, rows, notes))
}

/// The table `settings` pick from `sheet`, named `name`, with notes on how
/// it was read.
///
/// # Errors
/// As for [`read_block`].
pub fn table_from_sheet(name: &str, sheet: &RawSheet, settings: &OpenSettings) -> Result<(DataTable, Vec<String>)> {
    let (names, rows, mut notes) = read_block(sheet, settings)?;
    let (table, more) = table_from_records_with_notes(name, names, rows)?;
    notes.extend(more);
    Ok((table, notes))
}

/// One table from several sheets read with their own settings, stacked:
/// a first column named `sheet_column` holds each row's sheet name, then
/// every column any sheet has, matched by name, in the order first met
/// (blank where a sheet lacks one). Values are typed after stacking, so a
/// column is numeric only if it is numeric in every sheet.
///
/// # Errors
/// No sheets, or a sheet whose block holds nothing.
pub fn stack_sheets(
    name: &str,
    sheets: &[(&RawSheet, &OpenSettings)],
    sheet_column: &str,
) -> Result<(DataTable, Vec<String>)> {
    if sheets.is_empty() {
        return Err(invalid("choose at least one sheet to stack"));
    }
    let mut all_names: Vec<String> = Vec::new();
    let mut parts = Vec::new();
    let mut notes = Vec::new();
    for (sheet, settings) in sheets {
        let (raw, rows, more) =
            read_block(sheet, settings).map_err(|e| invalid(format!("sheet {:?}: {e}", sheet.name)))?;
        let names = stat_data::unique_names(raw);
        for n in &names {
            if !all_names.contains(n) {
                all_names.push(n.clone());
            }
        }
        notes.extend(more.into_iter().map(|n| format!("{}: {n}", sheet.name)));
        parts.push((sheet.name.clone(), names, rows));
    }
    let mut column = sheet_column.trim().to_owned();
    if column.is_empty() {
        column = "Sheet".to_owned();
    }
    while all_names.contains(&column) {
        column.push('_');
    }
    let mut header = vec![column];
    header.extend(all_names.iter().cloned());
    let mut records = Vec::new();
    for (sheet, names, rows) in parts {
        let at: Vec<Option<usize>> = all_names.iter().map(|n| names.iter().position(|m| m == n)).collect();
        for row in rows {
            let mut record = vec![sheet.clone()];
            record.extend(at.iter().map(|i| i.map_or_else(String::new, |i| row[i].clone())));
            records.push(record);
        }
    }
    let (table, more) = table_from_records_with_notes(name, header, records)?;
    notes.extend(more);
    Ok((table, notes))
}

/// The edit behind Data ▸ Use row as column names: the cells of `row`
/// (0-based) become the column names (blank or repeated ones made unique as
/// on import), the row is removed, and text columns whose remaining values
/// are all numbers or dates are typed again, as one undoable step.
/// Formula columns keep their formulas and types.
///
/// # Errors
/// A row outside the table.
pub fn use_row_as_names_edit(table: &DataTable, row: usize) -> Result<crate::editing::Edit> {
    use crate::editing::Edit;
    if row >= table.n_rows() {
        return Err(invalid("choose a row of the table"));
    }
    let raw: Vec<String> = table.columns().iter().map(|c| c.cell_text(row)).collect();
    let names = stat_data::unique_names(raw);
    let mut edits = Vec::new();
    // Through temporary names, so that no rename meets a name still in use.
    for j in 0..table.n_cols() {
        edits.push(Edit::RenameColumn { index: j, name: format!("\u{1}{j}") });
    }
    for (j, name) in names.iter().enumerate() {
        edits.push(Edit::RenameColumn { index: j, name: name.clone() });
    }
    edits.push(Edit::RemoveRows { rows: vec![row] });
    for (j, column) in table.columns().iter().enumerate() {
        let stat_data::ColumnData::Text(values) = &column.data else { continue };
        if column.formula.is_some() {
            continue;
        }
        let rest: Vec<Vec<String>> =
            values.iter().enumerate().filter(|&(i, _)| i != row).map(|(_, v)| vec![v.clone()]).collect();
        let (typed, _) = table_from_records_with_notes("", vec![names[j].clone()], rest)?;
        if let Some(new) = typed.column(0).filter(|c| !matches!(c.data, stat_data::ColumnData::Text(_))) {
            edits.push(Edit::ReplaceColumn { index: j, column: new.clone() });
        }
    }
    Ok(Edit::Batch { label: format!("use row {} as column names", row + 1), edits })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::ColumnData;

    fn sheet(rows: &[&[&str]]) -> RawSheet {
        RawSheet::new("s", rows.iter().map(|r| r.iter().map(|c| (*c).to_owned()).collect()).collect())
    }

    fn names(t: &DataTable) -> Vec<&str> {
        t.columns().iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn plain_table_starts_top_left() {
        let s = sheet(&[&["x", "y"], &["1", "a"], &["2", "b"]]);
        let b = detect_block(&s);
        assert_eq!(b, Block::default());
        assert!(b.is_plain(&s));
        let (t, notes) = table_from_sheet("t", &s, &detect_settings(&s, None)).unwrap();
        assert_eq!(names(&t), ["x", "y"]);
        assert_eq!(t.n_rows(), 2);
        assert!(notes.is_empty());
    }

    /// A report-style sheet: a title, a two-cell heading line off to the
    /// side, a blank row, then the table at C4.
    #[test]
    fn heading_block_beside_the_table_is_left_out() {
        let s = sheet(&[
            &["Fill weights"],
            &["Plant", "North"],
            &[],
            &["", "", "Shift", "Operator", "Weight"],
            &["", "", "A", "Ana", "500.1"],
            &["", "", "B", "Ben", "499.5"],
        ]);
        let b = detect_block(&s);
        assert_eq!((b.header_row, b.first_column, b.data_start), (Some(4), 3, 5));
    }

    /// Title lines and a blank row above a table that starts at C4.
    #[test]
    fn title_rows_and_offset_block() {
        let s = sheet(&[
            &["Plant trial 2026"],
            &["", "", ""],
            &[],
            &["", "", "Plot", "Yield", "Variety"],
            &["", "", "1", "4.5", "A"],
            &["", "", "2", "5.25", "B"],
            &[],
            &["Source: field records"],
        ]);
        let b = detect_block(&s);
        assert_eq!(b.header_row, Some(4));
        assert_eq!((b.data_start, b.data_end), (5, Some(6)), "the footnote is left out");
        assert_eq!((b.first_column, b.last_column), (3, None));
        assert!(!b.is_plain(&s));
        let (t, _) = table_from_sheet("t", &s, &detect_settings(&s, None)).unwrap();
        assert_eq!(names(&t), ["Plot", "Yield", "Variety"]);
        assert_eq!(t.column(1).unwrap().data, ColumnData::Numeric(vec![4.5, 5.25]));
        assert_eq!(b.role(4, 3), CellRole::Header);
        assert_eq!(b.role(5, 4), CellRole::Data);
        assert_eq!(b.role(5, 2), CellRole::Outside);
        assert_eq!(b.role(8, 3), CellRole::Outside);
        assert_eq!(b.describe(), "names in row 4, values in rows 5 to 6, columns from 3");
    }

    #[test]
    fn numbers_in_the_first_row_mean_no_names() {
        let s = sheet(&[&["1", "2"], &["3", "4"]]);
        let b = detect_block(&s);
        assert_eq!((b.header_row, b.data_start), (None, 1));
        let (t, _) = table_from_sheet("t", &s, &detect_settings(&s, None)).unwrap();
        assert_eq!(names(&t), ["Column 1", "Column 2"]);
        assert_eq!(t.n_rows(), 2);
        // Labels in the first column over more labels: still data.
        let s = sheet(&[&["A", "1"], &["B", "2"]]);
        assert_eq!(detect_block(&s).header_row, None);
        // Text over numbers in some column: names (years are names here).
        let s = sheet(&[&["name", "2024", "total"], &["Ann", "3", "4"]]);
        assert_eq!(detect_block(&s).header_row, Some(1));
        // All text: the first row names the columns.
        let s = sheet(&[&["name", "city"], &["Ann", "Oslo"]]);
        assert_eq!(detect_block(&s).header_row, Some(1));
    }

    /// Two header rows, the upper one a merged heading over two columns.
    #[test]
    fn multi_row_header_is_joined() {
        let s = sheet(&[&["Site", "Temperature", ""], &["", "Min", "Max"], &["A", "1", "9"], &["B", "2", "8"]]);
        let mut settings = detect_settings(&s, None);
        settings.block.header_rows = 2;
        settings.block.set_header_row(Some(1));
        assert_eq!(settings.block.data_start, 3);
        let (t, _) = table_from_sheet("t", &s, &settings).unwrap();
        assert_eq!(names(&t), ["Site", "Temperature Min", "Temperature Max"]);
        assert_eq!(t.n_rows(), 2);
        assert_eq!(settings.block.describe(), "names in rows 1 to 2, values from row 3, all columns");
    }

    #[test]
    fn block_edits_keep_names_above_values() {
        let mut b = Block::default();
        b.set_header_row(Some(4));
        assert_eq!(b.data_start, 5);
        b.set_data_start(3);
        assert_eq!(b.header_row, None, "names below the values are dropped");
        let mut b = Block { header_rows: 3, ..Block::default() };
        b.set_data_start(3);
        assert_eq!((b.header_row, b.header_rows), (Some(1), 2));
    }

    #[test]
    fn decimal_comma_and_missing_markers() {
        let (s, d) = split_text("t", "a;b;c\n1,5;x;1.234,5\n2,25;-;7\n", None).unwrap();
        assert_eq!(d, ';');
        assert!(detect_decimal_comma(&s, Some(d)));
        let mut settings = detect_settings(&s, Some(d));
        settings.values.missing = vec!["-".into()];
        let (t, _) = table_from_sheet("t", &s, &settings).unwrap();
        assert_eq!(t.column(0).unwrap().data, ColumnData::Numeric(vec![1.5, 2.25]));
        assert_eq!(t.column(2).unwrap().data, ColumnData::Numeric(vec![1234.5, 7.0]));
        assert_eq!(t.column(1).unwrap().data, ColumnData::Text(vec!["x".into(), String::new()]));
        assert!(!detect_decimal_comma(&sheet(&[&["a"], &["1.5"], &["2,5"]]), None), "a point anywhere: no");
        assert_eq!(decimal_comma_number("03.04.2026"), None, "dates are not numbers");
        assert_eq!(decimal_comma_number("12"), Some("12".into()));
        assert_eq!(decimal_comma_number("-1,5e3"), Some("-1.5e3".into()));
        // A column with one value that does not read that way is left alone.
        let s = sheet(&[&["a"], &["1,5"], &["n.a."]]);
        let settings =
            OpenSettings { values: ValueOptions { decimal_comma: true, missing: vec![] }, ..Default::default() };
        let (t, _) = table_from_sheet("t", &s, &settings).unwrap();
        assert_eq!(t.column(0).unwrap().data, ColumnData::Text(vec!["1,5".into(), "n.a.".into()]));
    }

    #[test]
    fn stacked_sheets_match_columns_by_name() {
        let a = RawSheet::new("Jan", vec![vec!["x".into(), "y".into()], vec!["1".into(), "2".into()]]);
        let b = RawSheet::new(
            "Feb",
            vec![
                vec![String::new()],
                vec!["y".into(), "z".into()],
                vec!["3".into(), "w".into()],
                vec!["4".into(), "v".into()],
            ],
        );
        let sa = detect_settings(&a, None);
        let sb = detect_settings(&b, None);
        assert_eq!(sb.block.header_row, Some(2));
        let (t, _) = stack_sheets("both", &[(&a, &sa), (&b, &sb)], "Sheet").unwrap();
        assert_eq!(names(&t), ["Sheet", "x", "y", "z"]);
        assert_eq!(t.n_rows(), 3);
        assert_eq!(t.column(0).unwrap().data, ColumnData::Text(vec!["Jan".into(), "Feb".into(), "Feb".into()]));
        let y = t.column(2).unwrap().as_numeric().unwrap();
        assert_eq!(y, &[2.0, 3.0, 4.0]);
        assert!(t.column(1).unwrap().as_numeric().unwrap()[1].is_nan());
        assert!(stack_sheets("none", &[], "Sheet").is_err());
    }

    #[test]
    fn short_rows_are_padded_with_a_note() {
        let (s, _) = split_text("t", "a,b,c\n1,2,3\n4,5\n", Some(',')).unwrap();
        let (t, notes) = table_from_sheet("t", &s, &detect_settings(&s, Some(','))).unwrap();
        assert_eq!(t.n_rows(), 2);
        assert!(t.column(2).unwrap().as_numeric().unwrap()[1].is_nan());
        assert_eq!(notes.len(), 1, "{notes:?}");
    }

    #[test]
    fn settings_round_trip_and_partial_maps() {
        let s = OpenSettings {
            block: Block {
                header_row: Some(4),
                header_rows: 2,
                data_start: 6,
                data_end: Some(40),
                first_column: 3,
                last_column: Some(9),
            },
            values: ValueOptions { decimal_comma: true, missing: vec!["-".into(), "n.a.".into()] },
            delimiter: Some('\t'),
        };
        assert_eq!(OpenSettings::from_json(&s.to_json(), &OpenSettings::default()).unwrap(), s);
        let only = stat_data::parse_json(r#"{"header_row": 4, "first_column": 3}"#).unwrap();
        let read = OpenSettings::from_json(&only, &OpenSettings::default()).unwrap();
        assert_eq!(read.block, Block { header_row: Some(4), data_start: 5, first_column: 3, ..Block::default() });
        let none = stat_data::parse_json(r#"{"header_row": 0}"#).unwrap();
        let read = OpenSettings::from_json(&none, &OpenSettings::default()).unwrap();
        assert_eq!((read.block.header_row, read.block.data_start), (None, 1));
        for bad in [r#"{"header_row": -1}"#, r#"{"delimiter": ";;"}"#, r#"{"decimal": "x"}"#, "[1]"] {
            assert!(
                OpenSettings::from_json(&stat_data::parse_json(bad).unwrap(), &OpenSettings::default()).is_err(),
                "{bad}"
            );
        }
    }

    /// A table opened with its names as the first row of values gets them
    /// back in one undoable step, and the columns become numeric again.
    #[test]
    fn use_row_as_names() {
        use crate::editing::EditHistory;
        use stat_data::{Column, RowStateManager};
        let mut t = DataTable::new(
            "t",
            vec![
                Column::text("Column 1", vec!["Title".into(), "Plot".into(), "1".into(), "2".into()]),
                Column::text("Column 2", vec![String::new(), "Plot".into(), "4.5".into(), "x".into()]),
            ],
        )
        .unwrap();
        let original = t.clone();
        let mut rows = RowStateManager::new(4);
        let mut history = EditHistory::default();
        history.apply(use_row_as_names_edit(&t, 1).unwrap(), &mut t, &mut rows).unwrap();
        assert_eq!(names(&t), ["Plot", "Plot (2)"]);
        assert_eq!(t.n_rows(), 3);
        // "Title" keeps the first column text; the second stays text because of "x".
        assert!(matches!(t.column(0).unwrap().data, ColumnData::Text(_)));
        history.apply(use_row_as_names_edit(&t, 0).unwrap(), &mut t, &mut rows).unwrap();
        assert_eq!(names(&t), ["Title", "Column 2"]);
        assert_eq!(t.column(0).unwrap().data, ColumnData::Numeric(vec![1.0, 2.0]));
        history.undo(&mut t, &mut rows).unwrap().unwrap();
        history.undo(&mut t, &mut rows).unwrap().unwrap();
        assert_eq!(t.columns(), original.columns());
        assert!(use_row_as_names_edit(&t, 9).is_err());
    }
}
