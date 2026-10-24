//! In-memory columnar data table.
//!
//! Each column is stored contiguously and carries a measurement level (Stevens
//! 1946, "On the theory of scales of measurement", *Science* 103). Missing
//! numeric and date-time values are `NaN`; missing text values are empty
//! strings. Date-time columns hold seconds since 1970-01-01T00:00:00 UTC (see
//! [`crate::temporal`]), so they read as numbers wherever an analysis needs
//! one and show as dates wherever a value is displayed.
//!
//! This is a deliberately small, dependency-free store. `CLAUDE.md` names Polars /
//! Arrow as the long-term backing; the public API here (named columns, typed
//! column slices, row count) is shaped so that a Polars-backed implementation can
//! replace it without changing callers.

use std::collections::BTreeMap;

use stat_core::{Result, StatError};

use crate::temporal::{TemporalDisplay, TemporalFormat};

/// Level of measurement of a column, which determines the analyses it can enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeasurementLevel {
    /// Numeric values on an interval or ratio scale.
    Continuous,
    /// Unordered categories.
    Nominal,
    /// Ordered categories.
    Ordinal,
}

impl MeasurementLevel {
    /// Lower-case display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Nominal => "nominal",
            Self::Ordinal => "ordinal",
        }
    }
}

/// Column storage.
///
/// Equality treats two missing (`NaN`) numeric values as equal, so tables that
/// hold the same data compare equal.
#[derive(Debug, Clone)]
pub enum ColumnData {
    /// Floating-point values; `NaN` marks a missing value.
    Numeric(Vec<f64>),
    /// Text values; an empty string marks a missing value.
    Text(Vec<String>),
    /// Dates, times of day or durations as seconds (since
    /// 1970-01-01T00:00:00 UTC, since midnight, or a length); `NaN` marks a
    /// missing value.
    DateTime {
        /// The values in seconds.
        values: Vec<f64>,
        /// How the values are shown.
        display: TemporalDisplay,
    },
}

fn same_numbers(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y || (x.is_nan() && y.is_nan()))
}

impl PartialEq for ColumnData {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Numeric(a), Self::Numeric(b)) => same_numbers(a, b),
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::DateTime { values: a, display: da }, Self::DateTime { values: b, display: db }) => {
                da == db && same_numbers(a, b)
            }
            _ => false,
        }
    }
}

impl ColumnData {
    /// Number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Numeric(v) | Self::DateTime { values: v, .. } => v.len(),
            Self::Text(v) => v.len(),
        }
    }

    /// `true` if the column has no values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Text that marks a missing value in number and date columns: the default
/// missing-value markers of pandas' `read_csv` (`na_values`), matched exactly
/// after trimming surrounding spaces. Text columns keep these as ordinary text,
/// so a country code `NA` in a text column is not lost.
pub const MISSING_MARKERS: [&str; 18] = [
    "#N/A", "#N/A N/A", "#NA", "-1.#IND", "-1.#QNAN", "-NaN", "-nan", "1.#IND", "1.#QNAN", "<NA>", "N/A", "NA", "NULL",
    "NaN", "None", "n/a", "nan", "null",
];

/// `true` if `text` (trimmed) is one of [`MISSING_MARKERS`].
#[must_use]
pub fn is_missing_marker(text: &str) -> bool {
    MISSING_MARKERS.contains(&text.trim())
}

/// Shortest text that reads back as exactly `x`: plain decimal notation for
/// magnitudes in `[1e-5, 1e16)` (and zero), scientific notation otherwise, so
/// that neither `1e300` nor `2.5e-300` expands to hundreds of digits.
/// Non-finite values render as `NaN`, `inf` and `-inf`.
#[must_use]
pub fn format_number(x: f64) -> String {
    let a = x.abs();
    if a == 0.0 || (1e-5..1e16).contains(&a) || !a.is_finite() { format!("{x}") } else { format!("{x:e}") }
}

/// One cell's value, as stored: a number (`NaN` for missing) or text (empty for
/// missing). Date-time columns hold numbers (seconds).
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    /// A value of a numeric or date-time column.
    Number(f64),
    /// A value of a text column.
    Text(String),
}

/// The storage type of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    /// Floating-point values.
    Numeric,
    /// Text values.
    Text,
    /// Dates, times of day or durations, with how they are shown.
    DateTime(TemporalDisplay),
}

impl ColumnKind {
    /// A date-time kind of `format`, shown at UTC to whole seconds.
    #[must_use]
    pub fn date_time(format: TemporalFormat) -> Self {
        Self::DateTime(TemporalDisplay::new(format))
    }

    /// `true` for kinds stored as numbers (numeric and date-time).
    #[must_use]
    pub fn is_numeric(self) -> bool {
        !matches!(self, Self::Text)
    }
}

/// A named column with its measurement level.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    /// Column name (unique within a table).
    pub name: String,
    /// Values.
    pub data: ColumnData,
    /// Measurement level.
    pub level: MeasurementLevel,
    /// The formula computing the values, for a formula column (a Rhai
    /// expression evaluated per row; see `app_model::formula`). `None` for a
    /// column of entered values.
    pub formula: Option<String>,
    /// Value labels: the label shown for each stored value, keyed by the
    /// value's cell text (for example `"1"` → `"Treatment"`). Analyses,
    /// plots and keys name a level by its label ([`Column::level_text`]);
    /// the data keep the stored values. Empty for no labels.
    pub value_labels: BTreeMap<String, String>,
    /// Whether plots list this column's value in the tooltip of a point
    /// ("show on hover").
    pub hover: bool,
}

impl Column {
    /// A continuous numeric column.
    #[must_use]
    pub fn numeric(name: impl Into<String>, values: Vec<f64>) -> Self {
        Self {
            name: name.into(),
            data: ColumnData::Numeric(values),
            level: MeasurementLevel::Continuous,
            formula: None,
            value_labels: BTreeMap::new(),
            hover: false,
        }
    }

    /// A nominal text column.
    #[must_use]
    pub fn text(name: impl Into<String>, values: Vec<String>) -> Self {
        Self {
            name: name.into(),
            data: ColumnData::Text(values),
            level: MeasurementLevel::Nominal,
            formula: None,
            value_labels: BTreeMap::new(),
            hover: false,
        }
    }

    /// A continuous date-time column of seconds shown with `display`.
    #[must_use]
    pub fn date_time(name: impl Into<String>, values: Vec<f64>, display: TemporalDisplay) -> Self {
        Self {
            name: name.into(),
            data: ColumnData::DateTime { values, display },
            level: MeasurementLevel::Continuous,
            formula: None,
            value_labels: BTreeMap::new(),
            hover: false,
        }
    }

    /// Numeric values, or `None` for a text column. A date-time column gives
    /// its values in seconds, so every analysis of numbers accepts it.
    #[must_use]
    pub fn as_numeric(&self) -> Option<&[f64]> {
        match &self.data {
            ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => Some(v),
            ColumnData::Text(_) => None,
        }
    }

    /// How a date-time column shows its values, or `None` for other columns.
    #[must_use]
    pub fn temporal(&self) -> Option<TemporalDisplay> {
        match &self.data {
            ColumnData::DateTime { display, .. } => Some(*display),
            _ => None,
        }
    }

    /// Text for a value `x` measured on this column (a data value, or a
    /// location statistic such as a mean or quantile): a date or time for a
    /// date-time column, otherwise the shortest round-trip number. Empty for a
    /// missing value of a date-time column.
    #[must_use]
    pub fn value_text(&self, x: f64) -> String {
        match self.temporal() {
            Some(d) => d.text(x),
            None => format_number(x),
        }
    }

    /// Text for a spread `x` on this column's scale (a standard deviation,
    /// range or interquartile range): a length of time for a date-time
    /// column, otherwise the shortest round-trip number.
    #[must_use]
    pub fn span_text(&self, x: f64) -> String {
        match self.temporal() {
            Some(d) => d.span_text(x),
            None => format_number(x),
        }
    }

    /// Storage type.
    #[must_use]
    pub fn kind(&self) -> ColumnKind {
        match &self.data {
            ColumnData::Numeric(_) => ColumnKind::Numeric,
            ColumnData::Text(_) => ColumnKind::Text,
            ColumnData::DateTime { display, .. } => ColumnKind::DateTime(*display),
        }
    }

    /// A column of `len` missing values.
    #[must_use]
    pub fn empty(name: impl Into<String>, kind: ColumnKind, len: usize) -> Self {
        match kind {
            ColumnKind::Numeric => Self::numeric(name, vec![f64::NAN; len]),
            ColumnKind::Text => Self::text(name, vec![String::new(); len]),
            ColumnKind::DateTime(display) => Self::date_time(name, vec![f64::NAN; len], display),
        }
    }

    /// The value in `row`.
    ///
    /// # Panics
    /// If `row` is out of range.
    #[must_use]
    pub fn value(&self, row: usize) -> CellValue {
        match &self.data {
            ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => CellValue::Number(v[row]),
            ColumnData::Text(v) => CellValue::Text(v[row].clone()),
        }
    }

    /// The missing value for this column's type.
    #[must_use]
    pub fn missing(&self) -> CellValue {
        match self.kind() {
            ColumnKind::Numeric | ColumnKind::DateTime(_) => CellValue::Number(f64::NAN),
            ColumnKind::Text => CellValue::Text(String::new()),
        }
    }

    /// Interpret typed text as a value for this column. Blank text is a missing
    /// value, and so is a [missing marker](MISSING_MARKERS) such as `NA` in a
    /// numeric or date-time column; a numeric column accepts any finite number in Rust's float syntax
    /// (e.g. `12`, `-0.5`, `1e-3`); a date-time column accepts the forms of
    /// [`TemporalDisplay::parse`].
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the column is numeric and the text is
    /// not a finite number, or the column holds dates and the text is not one.
    pub fn parse_value(&self, text: &str) -> Result<CellValue> {
        match self.kind() {
            ColumnKind::Text => Ok(CellValue::Text(text.to_owned())),
            ColumnKind::Numeric | ColumnKind::DateTime(_) if is_missing_marker(text) => Ok(CellValue::Number(f64::NAN)),
            ColumnKind::DateTime(display) => display
                .parse(text)
                .map(CellValue::Number)
                .map_err(|e| StatError::InvalidArgument { message: format!("{e} (column {:?})", self.name) }),
            ColumnKind::Numeric => {
                let t = text.trim();
                if t.is_empty() {
                    return Ok(CellValue::Number(f64::NAN));
                }
                t.parse::<f64>().ok().filter(|x| x.is_finite()).map(CellValue::Number).ok_or_else(|| {
                    StatError::InvalidArgument {
                        message: format!("“{t}” is not a number (column {:?} is numeric)", self.name),
                    }
                })
            }
        }
    }

    /// This column converted to `kind`, with the number of non-missing values
    /// that could not be converted (text that is not a number or a date
    /// becomes missing). Numbers become their shortest round-trip text and
    /// dates their displayed text. Numbers and date-times convert into each
    /// other unchanged, as seconds since 1970-01-01T00:00:00 UTC; converting
    /// between date-time displays keeps the values and changes only how they
    /// are shown. Blank text and [missing markers](MISSING_MARKERS) such as
    /// `NA` become missing without counting as lost. The measurement level
    /// becomes the default for the new kind unless it is still valid: an
    /// Ordinal column stays Ordinal (text is only Ordinal by the user's choice,
    /// and ordered numbers such as ratings are common), while Nominal text
    /// becomes Continuous numbers.
    #[must_use]
    pub fn converted(&self, kind: ColumnKind) -> (Self, usize) {
        let to_text_level =
            if self.level == MeasurementLevel::Continuous { MeasurementLevel::Nominal } else { self.level };
        match (&self.data, kind) {
            (ColumnData::Numeric(_), ColumnKind::Numeric) | (ColumnData::Text(_), ColumnKind::Text) => {
                (self.clone(), 0)
            }
            (ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. }, ColumnKind::DateTime(display)) => {
                let data = ColumnData::DateTime { values: v.clone(), display };
                (Self { name: self.name.clone(), data, level: self.level, formula: self.formula.clone(), value_labels: self.value_labels.clone(), hover: self.hover }, 0)
            }
            (ColumnData::DateTime { values, .. }, ColumnKind::Numeric) => (
                Self {
                    name: self.name.clone(),
                    data: ColumnData::Numeric(values.clone()),
                    level: self.level,
                    formula: self.formula.clone(),
                    value_labels: self.value_labels.clone(),
                    hover: self.hover,
                },
                0,
            ),
            (ColumnData::Numeric(_) | ColumnData::DateTime { .. }, ColumnKind::Text) => {
                let text = (0..self.data.len()).map(|r| self.cell_text(r)).collect();
                (
                    Self {
                        name: self.name.clone(),
                        data: ColumnData::Text(text),
                        level: to_text_level,
                        formula: self.formula.clone(),
                        value_labels: self.value_labels.clone(),
                        hover: self.hover,
                    },
                    0,
                )
            }
            (ColumnData::Text(v), ColumnKind::DateTime(display)) => {
                let mut lost = 0;
                let values = v
                    .iter()
                    .map(|s| {
                        if is_missing_marker(s) {
                            return f64::NAN;
                        }
                        display.parse(s).unwrap_or_else(|_| {
                            lost += 1;
                            f64::NAN
                        })
                    })
                    .collect();
                let column = Self::date_time(self.name.clone(), values, display);
                (
                    Self {
                        formula: self.formula.clone(),
                        level: self.ordinal_or(column.level),
                        value_labels: self.value_labels.clone(),
                        hover: self.hover,
                        ..column
                    },
                    lost,
                )
            }
            (ColumnData::Text(v), ColumnKind::Numeric) => {
                let mut lost = 0;
                let values = v
                    .iter()
                    .map(|s| {
                        let t = s.trim();
                        if t.is_empty() || is_missing_marker(t) {
                            return f64::NAN;
                        }
                        t.parse::<f64>().ok().filter(|x| x.is_finite()).unwrap_or_else(|| {
                            lost += 1;
                            f64::NAN
                        })
                    })
                    .collect();
                let level = self.ordinal_or(MeasurementLevel::Continuous);
                (Self { formula: self.formula.clone(), level, ..Self::numeric(self.name.clone(), values) }, lost)
            }
        }
    }

    /// Ordinal if this column is Ordinal, else `default`.
    fn ordinal_or(&self, default: MeasurementLevel) -> MeasurementLevel {
        if self.level == MeasurementLevel::Ordinal { MeasurementLevel::Ordinal } else { default }
    }

    /// A date-time column with each value's month and day exchanged (see
    /// [`crate::temporal::swap_day_month`]), with the number of non-missing
    /// values that could not be swapped (day above 12), which become missing.
    /// `None` for other columns.
    #[must_use]
    pub fn day_month_swapped(&self) -> Option<(Self, usize)> {
        let ColumnData::DateTime { values, display } = &self.data else { return None };
        let mut lost = 0;
        let swapped = values
            .iter()
            .map(|&x| {
                if x.is_nan() {
                    return x;
                }
                crate::temporal::swap_day_month(x, display.offset_minutes).unwrap_or_else(|| {
                    lost += 1;
                    f64::NAN
                })
            })
            .collect();
        let data = ColumnData::DateTime { values: swapped, display: *display };
        Some((Self { name: self.name.clone(), data, level: self.level, formula: None, value_labels: self.value_labels.clone(), hover: self.hover }, lost))
    }

    /// Insert `items` (strictly ascending final positions, values already
    /// checked against the column's type) in one pass.
    fn insert_values(&mut self, items: Vec<(usize, CellValue)>) {
        match &mut self.data {
            ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => insert_sorted(
                v,
                items.into_iter().map(|(at, x)| (at, if let CellValue::Number(x) = x { x } else { f64::NAN })),
            ),
            ColumnData::Text(v) => insert_sorted(
                v,
                items.into_iter().map(|(at, x)| (at, if let CellValue::Text(t) = x { t } else { String::new() })),
            ),
        }
    }

    /// Remove `rows` (strictly ascending, in range) in one pass and return
    /// their values in order.
    fn remove_values(&mut self, rows: &[usize]) -> Vec<CellValue> {
        match &mut self.data {
            ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => {
                remove_sorted(v, rows).into_iter().map(CellValue::Number).collect()
            }
            ColumnData::Text(v) => remove_sorted(v, rows).into_iter().map(CellValue::Text).collect(),
        }
    }

    /// Display text for one cell: empty for a missing value.
    ///
    /// # Panics
    /// If `row` is out of range.
    #[must_use]
    pub fn cell_text(&self, row: usize) -> String {
        match &self.data {
            ColumnData::Numeric(v) => {
                let x = v[row];
                if x.is_nan() { String::new() } else { format_number(x) }
            }
            ColumnData::Text(v) => v[row].clone(),
            ColumnData::DateTime { values, display } => display.text(values[row]),
        }
    }

    /// The name of the level in `row`, as analyses, plots and keys show it:
    /// the value's label when the column has [value labels](Self::value_labels)
    /// for it, otherwise its [cell text](Self::cell_text). Empty for a
    /// missing value. Every place that turns a column into category names
    /// goes through this, so labels reach them all.
    ///
    /// # Panics
    /// If `row` is out of range.
    #[must_use]
    pub fn level_text(&self, row: usize) -> String {
        let text = self.cell_text(row);
        if self.value_labels.is_empty() || text.is_empty() {
            return text;
        }
        self.value_labels.get(&text).cloned().unwrap_or(text)
    }

    /// The label of the stored value whose cell text is `code`, if it has one.
    #[must_use]
    pub fn label_of(&self, code: &str) -> Option<&str> {
        self.value_labels.get(code).map(String::as_str)
    }

    /// The stored value's cell text for a level name: the code whose label
    /// is `name`, or `name` itself when no label matches (so a level named
    /// by its code is found either way).
    #[must_use]
    pub fn code_of<'a>(&'a self, name: &'a str) -> &'a str {
        self.value_labels.iter().find(|(_, l)| l.as_str() == name).map_or(name, |(c, _)| c.as_str())
    }
}

/// A rectangular table of named columns.
///
/// Every change made through the editing methods advances
/// [`revision`](Self::revision), so views and analyses can tell when the data
/// changed. Revisions are unique across the process (see
/// [`crate::next_revision`]): two tables share one only when one is a clone
/// of the other with no edit since, so an equal revision means equal data
/// and nothing has to be compared value by value. Equality compares name,
/// columns and values, not the revision.
#[derive(Debug, Clone)]
pub struct DataTable {
    name: String,
    columns: Vec<Column>,
    n_rows: usize,
    revision: u64,
}

impl PartialEq for DataTable {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.n_rows == other.n_rows && self.columns == other.columns
    }
}

/// Remove the elements at `rows` (strictly ascending, each below `v.len()`)
/// from `v` in a single pass and return them in order. Linear in `v.len()`,
/// unlike repeated [`Vec::remove`].
pub(crate) fn remove_sorted<T>(v: &mut Vec<T>, rows: &[usize]) -> Vec<T> {
    let mut removed = Vec::with_capacity(rows.len());
    let mut kept = Vec::with_capacity(v.len().saturating_sub(rows.len()));
    let mut next = rows.iter().copied().peekable();
    for (i, x) in std::mem::take(v).into_iter().enumerate() {
        if next.next_if_eq(&i).is_some() {
            removed.push(x);
        } else {
            kept.push(x);
        }
    }
    *v = kept;
    removed
}

/// Merge `items` into `v` in a single pass so that each `(at, value)` ends up
/// at index `at` of the result. The positions must be strictly ascending and
/// each at most the length before that item goes in (as for
/// [`DataTable::insert_rows`]). Linear in the result's length, unlike repeated
/// [`Vec::insert`].
pub(crate) fn insert_sorted<T>(v: &mut Vec<T>, items: impl IntoIterator<Item = (usize, T)>) {
    let mut items = items.into_iter().peekable();
    let mut old = std::mem::take(v).into_iter();
    let mut out = Vec::with_capacity(old.len() + items.size_hint().0);
    loop {
        if let Some((_, x)) = items.next_if(|(at, _)| *at == out.len()) {
            out.push(x);
        } else if let Some(x) = old.next() {
            out.push(x);
        } else {
            break;
        }
    }
    // Validated positions leave nothing over; keep any extras rather than drop them.
    out.extend(items.map(|(_, x)| x));
    *v = out;
}

/// `true` if `order` holds each of `0..n` exactly once.
#[must_use]
pub fn is_permutation(order: &[usize], n: usize) -> bool {
    let mut seen = vec![false; n];
    order.len() == n && order.iter().all(|&r| r < n && !std::mem::replace(&mut seen[r], true))
}

fn type_mismatch(column: &str) -> StatError {
    StatError::InvalidArgument { message: format!("value type does not match column {column:?}") }
}

impl DataTable {
    /// Build a table from columns of equal length with unique, non-blank names.
    ///
    /// # Errors
    /// * [`StatError::DimensionMismatch`] if column lengths differ.
    /// * [`StatError::InvalidArgument`] if a name is empty or only whitespace,
    ///   or two columns share a name.
    pub fn new(name: impl Into<String>, columns: Vec<Column>) -> Result<Self> {
        let n_rows = columns.first().map_or(0, |c| c.data.len());
        for c in &columns {
            if c.data.len() != n_rows {
                return Err(StatError::DimensionMismatch {
                    context: "column length",
                    expected: n_rows,
                    found: c.data.len(),
                });
            }
        }
        for (i, c) in columns.iter().enumerate() {
            if c.name.trim().is_empty() {
                return Err(StatError::InvalidArgument { message: format!("column {} has a blank name", i + 1) });
            }
            if columns[..i].iter().any(|d| d.name == c.name) {
                return Err(StatError::InvalidArgument { message: format!("duplicate column name {:?}", c.name) });
            }
        }
        Ok(Self { name: name.into(), columns, n_rows, revision: crate::next_revision() })
    }

    /// Table name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Give the table another name, returning the old one. The data and its
    /// revision are unchanged, so nothing computed from it goes stale.
    pub fn set_name(&mut self, name: impl Into<String>) -> String {
        std::mem::replace(&mut self.name, name.into())
    }

    /// Number of rows.
    #[must_use]
    pub fn n_rows(&self) -> usize {
        self.n_rows
    }

    /// Number of columns.
    #[must_use]
    pub fn n_cols(&self) -> usize {
        self.columns.len()
    }

    /// All columns, in order.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Column `index`, if it exists.
    #[must_use]
    pub fn column(&self, index: usize) -> Option<&Column> {
        self.columns.get(index)
    }

    /// Index of the column called `name`.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    /// Indices of the numeric columns.
    #[must_use]
    pub fn numeric_columns(&self) -> Vec<usize> {
        (0..self.columns.len()).filter(|&i| self.columns[i].as_numeric().is_some()).collect()
    }

    /// Change a column's measurement level.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the index is out of range, or if a text
    /// column would be marked continuous.
    pub fn set_level(&mut self, index: usize, level: MeasurementLevel) -> Result<()> {
        let col = self
            .columns
            .get_mut(index)
            .ok_or_else(|| StatError::InvalidArgument { message: format!("no column {index}") })?;
        if level == MeasurementLevel::Continuous && col.as_numeric().is_none() {
            return Err(StatError::InvalidArgument {
                message: format!("text column {:?} cannot be continuous", col.name),
            });
        }
        if col.level != level {
            col.level = level;
            self.revision = crate::next_revision();
        }
        Ok(())
    }

    /// Advanced by every edit; unique across the process, so an unchanged
    /// revision means unchanged columns and values (the name is left out:
    /// see [`set_name`](Self::set_name)).
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Put back `revision`, a revision this table had with exactly the
    /// columns and values it has now (an undo that restored them), so what
    /// was worked out from that state counts as current again. Passing any
    /// other revision breaks the promise that equal revisions mean equal
    /// data.
    pub fn restore_revision(&mut self, revision: u64) {
        self.revision = revision;
    }

    fn check_column(&self, index: usize) -> Result<()> {
        if index < self.columns.len() {
            Ok(())
        } else {
            Err(StatError::InvalidArgument { message: format!("no column {index}") })
        }
    }

    fn check_name(&self, name: &str, except: Option<usize>) -> Result<()> {
        if name.trim().is_empty() {
            return Err(StatError::InvalidArgument { message: "a column name cannot be blank".into() });
        }
        if self.columns.iter().enumerate().any(|(i, c)| Some(i) != except && c.name == name) {
            return Err(StatError::InvalidArgument { message: format!("there is already a column named {name:?}") });
        }
        Ok(())
    }

    /// Store `value` in cell (`row`, `column`) and return the previous value.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for an out-of-range cell or a value of the
    /// wrong type for the column.
    pub fn set_value(&mut self, row: usize, column: usize, value: CellValue) -> Result<CellValue> {
        self.check_column(column)?;
        if row >= self.n_rows {
            return Err(StatError::InvalidArgument { message: format!("no row {row}") });
        }
        let col = &mut self.columns[column];
        let old = match (&mut col.data, value) {
            (ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. }, CellValue::Number(x)) => {
                CellValue::Number(std::mem::replace(&mut v[row], x))
            }
            (ColumnData::Text(v), CellValue::Text(t)) => CellValue::Text(std::mem::replace(&mut v[row], t)),
            _ => return Err(type_mismatch(&col.name)),
        };
        self.revision = crate::next_revision();
        Ok(old)
    }

    /// Insert rows so that each `(index, values)` pair, taken in strictly
    /// ascending order of index, ends up at that index; `values` holds one value
    /// per column (see [`DataTable::empty_row`]).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the indices are not strictly ascending
    /// or reach past the end, or a row has the wrong width or value types. The
    /// table is unchanged on error.
    pub fn insert_rows(&mut self, rows: Vec<(usize, Vec<CellValue>)>) -> Result<()> {
        let mut last = None;
        // `len` is the table length before each row goes in.
        for (len, (at, values)) in (self.n_rows..).zip(&rows) {
            if last.is_some_and(|l| *at <= l) || *at > len {
                return Err(StatError::InvalidArgument { message: "invalid row insert positions".into() });
            }
            if values.len() != self.columns.len() {
                return Err(StatError::InvalidArgument { message: "inserted row has the wrong width".into() });
            }
            for (c, v) in self.columns.iter().zip(values) {
                if std::mem::discriminant(&c.missing()) != std::mem::discriminant(v) {
                    return Err(type_mismatch(&c.name));
                }
            }
            last = Some(*at);
        }
        // One merge per column (linear in the table length), not one
        // `Vec::insert` per row.
        let k = rows.len();
        let mut per_column: Vec<Vec<(usize, CellValue)>> = self.columns.iter().map(|_| Vec::with_capacity(k)).collect();
        for (at, values) in rows {
            for (bucket, v) in per_column.iter_mut().zip(values) {
                bucket.push((at, v));
            }
        }
        for (c, items) in self.columns.iter_mut().zip(per_column) {
            c.insert_values(items);
        }
        self.n_rows += k;
        self.revision = crate::next_revision();
        Ok(())
    }

    /// A row of missing values, for [`DataTable::insert_rows`].
    #[must_use]
    pub fn empty_row(&self) -> Vec<CellValue> {
        self.columns.iter().map(Column::missing).collect()
    }

    /// Remove `rows` (strictly ascending) and return their values, in order.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the indices are not strictly ascending
    /// or one is out of range. The table is unchanged on error.
    pub fn remove_rows(&mut self, rows: &[usize]) -> Result<Vec<Vec<CellValue>>> {
        if !rows.windows(2).all(|w| w[0] < w[1]) || rows.last().is_some_and(|&r| r >= self.n_rows) {
            return Err(StatError::InvalidArgument { message: "invalid rows to remove".into() });
        }
        // One pass per column (linear in the table length), not one
        // `Vec::remove` per row.
        let mut removed = vec![Vec::with_capacity(self.columns.len()); rows.len()];
        for c in &mut self.columns {
            for (row, value) in removed.iter_mut().zip(c.remove_values(rows)) {
                row.push(value);
            }
        }
        self.n_rows -= rows.len();
        self.revision = crate::next_revision();
        Ok(removed)
    }

    /// Put the rows in a new order: row `i` becomes the row that was at
    /// `order[i]` (as from [`crate::table_ops::sort_order`]).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if `order` is not a permutation of
    /// `0..n_rows()`. The table is unchanged on error.
    pub fn reorder_rows(&mut self, order: &[usize]) -> Result<()> {
        if !is_permutation(order, self.n_rows) {
            return Err(StatError::InvalidArgument { message: "invalid row order".into() });
        }
        for c in &mut self.columns {
            match &mut c.data {
                ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => {
                    *v = order.iter().map(|&r| v[r]).collect();
                }
                ColumnData::Text(v) => *v = order.iter().map(|&r| std::mem::take(&mut v[r])).collect(),
            }
        }
        self.revision = crate::next_revision();
        Ok(())
    }

    /// Insert `column` at position `index` (existing columns from `index` on
    /// move right).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if `index > n_cols()`, the name is blank
    /// or taken, or the length differs from the table's.
    pub fn insert_column(&mut self, index: usize, column: Column) -> Result<()> {
        if index > self.columns.len() {
            return Err(StatError::InvalidArgument { message: format!("cannot insert a column at {index}") });
        }
        self.check_name(&column.name, None)?;
        if column.data.len() != self.n_rows && !self.columns.is_empty() {
            return Err(StatError::DimensionMismatch {
                context: "column length",
                expected: self.n_rows,
                found: column.data.len(),
            });
        }
        if self.columns.is_empty() {
            self.n_rows = column.data.len();
        }
        self.columns.insert(index, column);
        self.revision = crate::next_revision();
        Ok(())
    }

    /// Remove and return column `index`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the index is out of range.
    pub fn remove_column(&mut self, index: usize) -> Result<Column> {
        self.check_column(index)?;
        self.revision = crate::next_revision();
        Ok(self.columns.remove(index))
    }

    /// Rename column `index`, returning the old name.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the index is out of range or the name is
    /// blank or used by another column.
    pub fn rename_column(&mut self, index: usize, name: &str) -> Result<String> {
        self.check_column(index)?;
        self.check_name(name, Some(index))?;
        self.revision = crate::next_revision();
        Ok(std::mem::replace(&mut self.columns[index].name, name.to_owned()))
    }

    /// Replace column `index` (for example with a type-converted copy),
    /// returning the old column.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the index is out of range or the new
    /// name is blank or used by another column; [`StatError::DimensionMismatch`]
    /// if the length differs.
    pub fn replace_column(&mut self, index: usize, column: Column) -> Result<Column> {
        self.check_column(index)?;
        self.check_name(&column.name, Some(index))?;
        if column.data.len() != self.n_rows {
            return Err(StatError::DimensionMismatch {
                context: "column length",
                expected: self.n_rows,
                found: column.data.len(),
            });
        }
        if column.level == MeasurementLevel::Continuous && column.as_numeric().is_none() {
            return Err(StatError::InvalidArgument { message: "a text column cannot be continuous".into() });
        }
        self.revision = crate::next_revision();
        Ok(std::mem::replace(&mut self.columns[index], column))
    }

    /// Set or clear the formula of column `index` without touching its
    /// values, returning the old formula.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the index is out of range.
    pub fn set_formula(&mut self, index: usize, formula: Option<String>) -> Result<Option<String>> {
        self.check_column(index)?;
        self.revision = crate::next_revision();
        Ok(std::mem::replace(&mut self.columns[index].formula, formula))
    }

    /// Replace the value labels of column `index`, returning the old ones.
    /// Blank labels are dropped.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the index is out of range.
    pub fn set_value_labels(
        &mut self,
        index: usize,
        mut labels: BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>> {
        self.check_column(index)?;
        labels.retain(|code, label| !code.is_empty() && !label.trim().is_empty());
        self.revision = crate::next_revision();
        Ok(std::mem::replace(&mut self.columns[index].value_labels, labels))
    }

    /// Show (or stop showing) column `index` in plot tooltips, returning the
    /// old setting.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if the index is out of range.
    pub fn set_hover(&mut self, index: usize, hover: bool) -> Result<bool> {
        self.check_column(index)?;
        self.revision = crate::next_revision();
        Ok(std::mem::replace(&mut self.columns[index].hover, hover))
    }

    /// A column name not yet used, `"{base} {n}"` with the smallest `n ≥ 1`.
    #[must_use]
    pub fn unused_column_name(&self, base: &str) -> String {
        (1..).map(|n| format!("{base} {n}")).find(|c| self.find(c).is_none()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, f64::NAN, 3.5]),
                Column::text("g", vec!["a".into(), "b".into(), String::new()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn shape_and_lookup() {
        let t = sample();
        assert_eq!((t.n_rows(), t.n_cols()), (3, 2));
        assert_eq!(t.find("g"), Some(1));
        assert_eq!(t.find("nope"), None);
        assert_eq!(t.numeric_columns(), vec![0]);
        assert_eq!(t.column(0).unwrap().cell_text(1), "", "NaN displays as missing");
        assert_eq!(t.column(0).unwrap().cell_text(2), "3.5");
        assert_eq!(t.column(1).unwrap().level, MeasurementLevel::Nominal);
    }

    #[test]
    fn rejects_bad_tables() {
        let ragged = vec![Column::numeric("x", vec![1.0]), Column::numeric("y", vec![1.0, 2.0])];
        assert!(DataTable::new("t", ragged).is_err());
        let dup = vec![Column::numeric("x", vec![1.0]), Column::numeric("x", vec![2.0])];
        assert!(DataTable::new("t", dup).is_err());
    }

    #[test]
    fn rejects_blank_column_names() {
        for blank in ["", "   ", "\t"] {
            let cols = vec![Column::numeric("x", vec![1.0]), Column::numeric(blank, vec![2.0])];
            let err = DataTable::new("t", cols).unwrap_err();
            assert!(matches!(&err, StatError::InvalidArgument { message } if message.contains("column 2")), "{err}");
        }
    }

    /// Removing and re-inserting many rows is one pass per column: 200 000
    /// rows, half removed and put back, well within a second in a debug build.
    #[test]
    fn many_row_removals_and_inserts_are_linear() {
        let n = 200_000;
        let build = || {
            DataTable::new(
                "big",
                vec![
                    Column::numeric("x", (0..n).map(|i| i as f64).collect()),
                    Column::text("s", (0..n).map(|i| format!("r{i}")).collect()),
                ],
            )
            .unwrap()
        };
        let original = build();
        let first_half: Vec<usize> = (0..n / 2).collect();
        let evens: Vec<usize> = (0..n).step_by(2).collect();
        for rows in [first_half, evens] {
            let mut t = build();
            let mut states = crate::RowStateManager::new(n);
            states.select(rows.iter().copied().take(3), crate::SelectionMode::Replace);
            let start = std::time::Instant::now();
            let removed = t.remove_rows(&rows).unwrap();
            let removed_states = states.remove_rows(&rows);
            let removal = start.elapsed();
            assert_eq!(t.n_rows(), n - rows.len());
            assert_eq!(states.len(), n - rows.len());
            let kept: Vec<f64> = (0..n).filter(|i| rows.binary_search(i).is_err()).map(|i| i as f64).collect();
            assert_eq!(t.column(0).unwrap().as_numeric().unwrap(), &kept[..]);
            assert_eq!(removed[1], vec![CellValue::Number(rows[1] as f64), CellValue::Text(format!("r{}", rows[1]))]);
            assert!(states.selected_rows().is_empty());
            // Undo: put the rows back where they were.
            let start = std::time::Instant::now();
            t.insert_rows(rows.iter().copied().zip(removed).collect()).unwrap();
            states.insert_rows(&rows.iter().copied().zip(removed_states).collect::<Vec<_>>());
            let insertion = start.elapsed();
            assert_eq!(t.columns(), original.columns());
            assert_eq!(states.selected_rows(), rows[..3].to_vec());
            eprintln!("{} rows: remove {removal:?}, insert {insertion:?}", rows.len());
            assert!(removal + insertion < std::time::Duration::from_secs(5), "{removal:?} + {insertion:?}");
        }
    }

    #[test]
    fn sorted_merge_helpers() {
        let mut v = vec![0, 1, 2, 3, 4, 5];
        assert_eq!(remove_sorted(&mut v, &[0, 2, 5]), vec![0, 2, 5]);
        assert_eq!(v, vec![1, 3, 4]);
        insert_sorted(&mut v, [(0, 0), (2, 2), (5, 5)]);
        assert_eq!(v, vec![0, 1, 2, 3, 4, 5]);
        insert_sorted(&mut v, [(6, 6), (7, 7)]);
        assert_eq!(v, vec![0, 1, 2, 3, 4, 5, 6, 7]);
        let mut empty: Vec<i32> = Vec::new();
        insert_sorted(&mut empty, [(0, 9)]);
        assert_eq!(empty, vec![9]);
    }

    #[test]
    fn cell_edits() {
        let mut t = sample();
        let r0 = t.revision();
        assert!(matches!(t.set_value(1, 0, CellValue::Number(2.0)).unwrap(), CellValue::Number(x) if x.is_nan()));
        assert!(t.revision() > r0);
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap()[1], 2.0);
        assert!(t.set_value(0, 0, CellValue::Text("x".into())).is_err(), "type checked");
        assert!(t.set_value(9, 0, CellValue::Number(1.0)).is_err());
        let old = t.set_value(2, 1, CellValue::Text("z".into())).unwrap();
        assert_eq!(old, CellValue::Text(String::new()));
        let col = t.column(0).unwrap();
        assert_eq!(col.parse_value(" 1e3 ").unwrap(), CellValue::Number(1000.0));
        assert!(matches!(col.parse_value(""), Ok(CellValue::Number(x)) if x.is_nan()));
        assert!(col.parse_value("abc").unwrap_err().to_string().contains("not a number"));
        assert_eq!(t.column(1).unwrap().parse_value("abc").unwrap(), CellValue::Text("abc".into()));
        // Equality ignores the revision.
        let mut u = sample();
        u.set_value(1, 0, CellValue::Number(2.0)).unwrap();
        u.set_value(2, 1, CellValue::Text("z".into())).unwrap();
        u.set_value(2, 1, CellValue::Text("z".into())).unwrap();
        assert_eq!(t, u);
    }

    /// Revisions are unique across tables: two different edits of one table
    /// never end at the same revision, while a clone keeps its original's.
    #[test]
    fn revisions_are_unique_across_tables() {
        let t = sample();
        assert_ne!(t.revision(), sample().revision(), "separately made tables differ");
        let (mut a, mut b) = (t.clone(), t.clone());
        assert_eq!(a.revision(), t.revision());
        a.set_value(0, 0, CellValue::Number(10.0)).unwrap();
        b.set_value(0, 0, CellValue::Number(20.0)).unwrap();
        assert_ne!(a.revision(), b.revision());
        assert!(a.revision() > t.revision() && b.revision() > t.revision());
    }

    #[test]
    fn row_edits() {
        let mut t = sample();
        let empty = t.empty_row();
        t.insert_rows(vec![(0, empty.clone()), (4, vec![CellValue::Number(9.0), CellValue::Text("q".into())])])
            .unwrap();
        assert_eq!(t.n_rows(), 5);
        assert!(t.column(0).unwrap().as_numeric().unwrap()[0].is_nan());
        assert_eq!(t.column(1).unwrap().cell_text(4), "q");
        assert_eq!(t.column(0).unwrap().cell_text(1), "1");
        let removed = t.remove_rows(&[0, 4]).unwrap();
        assert_eq!(t, sample());
        assert_eq!(removed[1], vec![CellValue::Number(9.0), CellValue::Text("q".into())]);
        assert!(t.remove_rows(&[2, 1]).is_err());
        assert!(t.remove_rows(&[3]).is_err());
        assert!(t.insert_rows(vec![(9, empty.clone())]).is_err());
        assert!(t.insert_rows(vec![(0, vec![CellValue::Number(1.0)])]).is_err(), "width");
        assert!(t.insert_rows(vec![(0, vec![CellValue::Text("a".into()), CellValue::Text("b".into())])]).is_err());
        assert_eq!(t, sample(), "failed edits change nothing");
    }

    #[test]
    fn column_edits() {
        let mut t = sample();
        let name = t.unused_column_name("Column");
        assert_eq!(name, "Column 1");
        t.insert_column(1, Column::empty(name, ColumnKind::Text, 3)).unwrap();
        assert_eq!(t.find("g"), Some(2));
        assert!(t.insert_column(0, Column::empty("x", ColumnKind::Numeric, 3)).is_err(), "duplicate");
        assert!(t.insert_column(0, Column::empty("w", ColumnKind::Numeric, 2)).is_err(), "length");
        assert!(t.rename_column(1, "g").is_err());
        assert!(t.rename_column(1, "  ").is_err());
        assert_eq!(t.rename_column(1, "new").unwrap(), "Column 1");
        let removed = t.remove_column(1).unwrap();
        assert_eq!(removed.name, "new");
        assert_eq!(t, sample());

        let (text, lost) = t.column(0).unwrap().converted(ColumnKind::Text);
        assert_eq!((lost, text.level), (0, MeasurementLevel::Nominal));
        assert_eq!(text.data, ColumnData::Text(vec!["1".into(), String::new(), "3.5".into()]));
        let (back, lost) = text.converted(ColumnKind::Numeric);
        assert_eq!(lost, 0);
        assert_eq!(back.as_numeric().unwrap()[2], 3.5);
        let (g, lost) = t.column(1).unwrap().converted(ColumnKind::Numeric);
        assert_eq!(lost, 2, "a and b are not numbers; the blank stays missing");
        assert!(g.as_numeric().unwrap().iter().all(|x| x.is_nan()));
        let old = t.replace_column(0, text).unwrap();
        assert_eq!(t.column(0).unwrap().kind(), ColumnKind::Text);
        t.replace_column(0, old).unwrap();
        assert_eq!(t, sample());
        let mut bad = t.column(1).unwrap().clone();
        bad.level = MeasurementLevel::Continuous;
        assert!(t.replace_column(1, bad).is_err());
    }

    #[test]
    fn conversion_to_numbers_keeps_ordinal_and_reads_markers_as_missing() {
        let mut rating = Column::text("rating", vec!["1".into(), "NA".into(), "3".into(), "bad".into()]);
        rating.level = MeasurementLevel::Ordinal;
        let (num, lost) = rating.converted(ColumnKind::Numeric);
        assert_eq!((lost, num.level), (1, MeasurementLevel::Ordinal), "NA is missing, not lost");
        assert!(num.as_numeric().unwrap()[1].is_nan());
        let (back, _) = num.converted(ColumnKind::Text);
        assert_eq!(back.level, MeasurementLevel::Ordinal, "round trip keeps the level");
        let code = Column::text("code", vec!["7".into(), "8".into()]);
        assert_eq!(code.converted(ColumnKind::Numeric).0.level, MeasurementLevel::Continuous);
        let date = ColumnKind::DateTime(TemporalDisplay::new(TemporalFormat::Date));
        let (when, lost) = Column::text("when", vec!["2026-03-04".into(), "N/A".into()]).converted(date);
        assert_eq!(lost, 0);
        assert!(when.as_numeric().unwrap()[1].is_nan());
        let number = Column::numeric("x", vec![1.0]);
        assert!(matches!(number.parse_value(" NA "), Ok(CellValue::Number(v)) if v.is_nan()));
        assert!(number.parse_value("na").is_err(), "only the listed spellings");
        assert!(matches!(rating.parse_value("NA"), Ok(CellValue::Text(t)) if t == "NA"));
    }

    #[test]
    fn levels() {
        let mut t = sample();
        t.set_level(1, MeasurementLevel::Ordinal).unwrap();
        assert_eq!(t.column(1).unwrap().level, MeasurementLevel::Ordinal);
        assert!(t.set_level(1, MeasurementLevel::Continuous).is_err());
        assert!(t.set_level(9, MeasurementLevel::Nominal).is_err());
        assert_eq!(MeasurementLevel::Ordinal.label(), "ordinal");
    }

    #[test]
    fn date_time_columns() {
        use crate::temporal::{TemporalDisplay, TemporalFormat, seconds_from_civil};
        let day = |d: u32| seconds_from_civil(2026, 3, d, 0, 0, 0.0);
        let date = TemporalDisplay::new(TemporalFormat::Date);
        let mut t = DataTable::new(
            "t",
            vec![
                Column::date_time("d", vec![day(4), f64::NAN, day(13)], date),
                Column::numeric("x", vec![1.0, 2.0, 3.0]),
            ],
        )
        .unwrap();
        let col = t.column(0).unwrap();
        assert_eq!(col.kind(), ColumnKind::DateTime(date));
        assert!(col.kind().is_numeric() && !ColumnKind::Text.is_numeric());
        assert_eq!(col.level, MeasurementLevel::Continuous);
        assert_eq!(col.as_numeric().unwrap()[0], day(4), "analyses see seconds");
        assert_eq!((col.cell_text(0), col.cell_text(1)), ("2026-03-04".into(), String::new()));
        assert_eq!(col.value_text(day(13)), "2026-03-13");
        assert_eq!(col.span_text(86_400.0 * 2.5), "2 12:00:00");
        assert_eq!(t.column(1).unwrap().value_text(2.5), "2.5");
        assert_eq!(t.numeric_columns(), vec![0, 1]);
        // Entry.
        assert_eq!(col.parse_value("2026-03-05").unwrap(), CellValue::Number(day(5)));
        assert!(col.parse_value("tomorrow").unwrap_err().to_string().contains("not a date"));
        t.set_value(1, 0, CellValue::Number(day(9))).unwrap();
        assert!(t.set_value(1, 0, CellValue::Text("x".into())).is_err());
        let empty = t.empty_row();
        assert!(matches!(empty[0], CellValue::Number(x) if x.is_nan()));
        t.insert_rows(vec![(0, empty)]).unwrap();
        assert_eq!(t.remove_rows(&[0]).unwrap().len(), 1);
        // Conversions.
        let col = t.column(0).unwrap().clone();
        let (text, lost) = col.converted(ColumnKind::Text);
        assert_eq!((lost, text.level), (0, MeasurementLevel::Nominal));
        assert_eq!(text.data, ColumnData::Text(vec!["2026-03-04".into(), "2026-03-09".into(), "2026-03-13".into()]));
        let (back, lost) = text.converted(col.kind());
        assert_eq!((back, lost), (col.clone(), 0));
        let (num, _) = col.converted(ColumnKind::Numeric);
        assert_eq!(num.as_numeric(), col.as_numeric());
        assert_eq!(num.converted(col.kind()).0, col);
        let stamp = ColumnKind::date_time(TemporalFormat::DateTime);
        assert_eq!(col.converted(stamp).0.cell_text(0), "2026-03-04 00:00:00");
        let (bad, lost) =
            Column::text("s", vec!["2026-03-04".into(), "nope".into(), String::new()]).converted(col.kind());
        assert_eq!(lost, 1);
        assert!(bad.as_numeric().unwrap()[1].is_nan());
        // Swapping day and month.
        let (swapped, lost) = col.day_month_swapped().unwrap();
        assert_eq!(lost, 1, "13 is not a month");
        assert_eq!(swapped.cell_text(0), "2026-04-03");
        assert_eq!(swapped.cell_text(1), "2026-09-03");
        assert!(t.column(1).unwrap().day_month_swapped().is_none());
        // Date-time columns may be continuous.
        t.set_level(0, MeasurementLevel::Ordinal).unwrap();
        t.set_level(0, MeasurementLevel::Continuous).unwrap();
    }
}
