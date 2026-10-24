//! Table operations: each takes one or more tables and builds a new one,
//! leaving the sources untouched.
//!
//! * **Rows** — [`sorted`] (several keys, stable, missing values last),
//!   [`without_duplicates`], [`subset`] (rows and columns) and [`sample_rows`]
//!   (simple or stratified random samples, seeded).
//! * **Merge** — [`combine_rows`] (concatenate tables, matching columns by
//!   name) and [`join`] (inner, left, right and full outer joins on key
//!   columns).
//! * **Reshape** — [`stack`] (wide → long), [`unstack`] (long → wide) and
//!   [`transpose`].
//! * **Summaries** — [`summarize`]: statistics of columns by group.
//!
//! Joins, selection and projection follow the relational algebra of Codd
//! (1970), "A relational model of data for large shared data banks",
//! *Communications of the ACM* 13(6), with the SQL (ISO/IEC 9075) rules for
//! outer joins: a missing key never matches anything, and rows without a
//! partner are padded with missing values. Stacking and unstacking follow the
//! long/wide reshaping of Wickham (2014), "Tidy data", *Journal of Statistical
//! Software* 59(10). Random samples use selection sampling (Knuth, *The Art of
//! Computer Programming* vol. 2, 3rd ed., §3.4.2, Algorithm S), which keeps
//! the rows in their original order; stratified samples allocate the total in
//! proportion to stratum size (Cochran, *Sampling Techniques*, 3rd ed., ch. 5)
//! with largest-remainder rounding so the stratum counts add up exactly.
//!
//! Every operation returns a [`Derived`] table that records, for each result
//! row, the single source row it came from when there is exactly one (so row
//! states can follow), and notes about anything the caller should be told
//! (type changes, rows left out, renamed columns).
//!
//! Values are compared as follows everywhere: numbers and date-times by value
//! (`-0` equals `0`), text exactly; a missing value is `NaN` or blank text.
//! For grouping and duplicate detection two missing values are equal; for
//! join keys they never are.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use stat_core::descriptive::{QuantileMethod, moments, quantile_sorted};
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::table::{Column, ColumnData, ColumnKind, DataTable, MeasurementLevel, format_number};
use crate::temporal::{TemporalDisplay, TemporalFormat};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Order labels numerically when both are numbers, otherwise as text; numbers
/// come before text. This is the order categorical levels are listed in
/// throughout the application, and the order text columns sort in.
#[must_use]
pub fn natural_order(a: &str, b: &str) -> Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.total_cmp(&y).then_with(|| a.cmp(b)),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

/// The source row a result row was copied from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowOrigin {
    /// Which input table (0 for single-table operations; the position in the
    /// input list for [`combine_rows`]; the left table for [`join`]).
    pub source: usize,
    /// Row index in that table.
    pub row: usize,
}

/// A table built by an operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    /// The new table.
    pub table: DataTable,
    /// For each row of `table`, the one source row it came from, or `None`
    /// when it combines several rows or is new (stacked, summarized, padded).
    pub origins: Vec<Option<RowOrigin>>,
    /// Things the caller should tell the user, in plain sentences.
    pub notes: Vec<String>,
}

// ----------------------------------------------------------------------------
// Shared helpers
// ----------------------------------------------------------------------------

fn column_at(table: &DataTable, index: usize) -> Result<&Column> {
    table.column(index).ok_or_else(|| invalid(format!("table {:?} has no column {index}", table.name())))
}

fn check_distinct(columns: &[usize], what: &str) -> Result<()> {
    for (i, c) in columns.iter().enumerate() {
        if columns[..i].contains(c) {
            return Err(invalid(format!("{what} lists a column twice")));
        }
    }
    Ok(())
}

fn is_missing(col: &Column, row: usize) -> bool {
    match &col.data {
        ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => v[row].is_nan(),
        ColumnData::Text(v) => v[row].trim().is_empty(),
    }
}

/// One cell as a hashable key part.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyPart {
    Missing,
    Number(u64),
    Text(String),
}

fn key_part(col: &Column, row: usize) -> KeyPart {
    match &col.data {
        ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => {
            let x = v[row];
            // Adding zero turns -0 into +0, so the two compare equal.
            if x.is_nan() { KeyPart::Missing } else { KeyPart::Number((x + 0.0).to_bits()) }
        }
        ColumnData::Text(v) => {
            if v[row].trim().is_empty() {
                KeyPart::Missing
            } else {
                KeyPart::Text(v[row].clone())
            }
        }
    }
}

fn row_key(cols: &[&Column], row: usize) -> Vec<KeyPart> {
    cols.iter().map(|c| key_part(c, row)).collect()
}

/// Compare two rows of one column: missing values last whatever the
/// direction; numbers and date-times by value; text in [`natural_order`].
fn compare_cells(col: &Column, a: usize, b: usize, descending: bool) -> Ordering {
    match (is_missing(col, a), is_missing(col, b)) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            let o = match &col.data {
                ColumnData::Numeric(v) | ColumnData::DateTime { values: v, .. } => {
                    v[a].partial_cmp(&v[b]).unwrap_or(Ordering::Equal)
                }
                ColumnData::Text(v) => natural_order(&v[a], &v[b]),
            };
            if descending { o.reverse() } else { o }
        }
    }
}

/// A column of `kind` filled from `cells` (`None` = missing). Sources whose
/// storage differs from `kind` are converted: to text through their displayed
/// text; numbers and date-times into each other as seconds.
fn build<'a>(
    name: String,
    kind: ColumnKind,
    level: MeasurementLevel,
    cells: impl Iterator<Item = Option<(&'a Column, usize)>>,
) -> Column {
    let number =
        |c: Option<(&Column, usize)>| c.and_then(|(col, r)| col.as_numeric().map(|v| v[r])).unwrap_or(f64::NAN);
    let data = match kind {
        ColumnKind::Text => {
            ColumnData::Text(cells.map(|c| c.map_or_else(String::new, |(col, r)| col.cell_text(r))).collect())
        }
        ColumnKind::Numeric => ColumnData::Numeric(cells.map(number).collect()),
        ColumnKind::DateTime(display) => ColumnData::DateTime { values: cells.map(number).collect(), display },
    };
    let level = if kind == ColumnKind::Text && level == MeasurementLevel::Continuous {
        MeasurementLevel::Nominal
    } else {
        level
    };
    Column { name, data, level, formula: None, value_labels: std::collections::BTreeMap::new(), hover: false }
}

/// `col` at `rows` (`None` = missing), keeping its name, type, level, value
/// labels and hover setting.
fn gather(col: &Column, rows: impl Iterator<Item = Option<usize>>) -> Column {
    let built = build(col.name.clone(), col.kind(), col.level, rows.map(|r| r.map(|r| (col, r))));
    Column { value_labels: col.value_labels.clone(), hover: col.hover, ..built }
}

/// The common type for columns that are put together: their own type when
/// they all agree; a date-time shown as the first one is when all are
/// date-times; otherwise text. Also says whether anything changed.
fn unify_kind(cols: &[&Column]) -> (ColumnKind, Option<String>) {
    let first = cols[0].kind();
    if cols.iter().all(|c| c.kind() == first) {
        return (first, None);
    }
    if let ColumnKind::DateTime(display) = first
        && cols.iter().all(|c| c.temporal().is_some())
    {
        return (first, Some(format!("{:?} is shown as {} in every table.", cols[0].name, display.format.label())));
    }
    (
        ColumnKind::Text,
        Some(format!("{:?} holds different types of values in different places, so it became text.", cols[0].name)),
    )
}

/// Level for a column of `kind` built from columns whose first is `first`.
fn level_for(kind: ColumnKind, first: &Column) -> MeasurementLevel {
    match (kind, first.level) {
        (ColumnKind::Text, MeasurementLevel::Continuous) => MeasurementLevel::Nominal,
        (_, level) => level,
    }
}

/// Names already used in a table being built, handing out unique ones.
#[derive(Default)]
struct Names(Vec<String>);

impl Names {
    /// `base` if unused, otherwise `"{base} 2"`, `"{base} 3"`, …
    fn unique(&mut self, base: &str) -> String {
        let base = if base.trim().is_empty() { "Column" } else { base };
        let name = if self.0.iter().any(|n| n == base) {
            (2..).map(|k| format!("{base} {k}")).find(|c| !self.0.contains(c)).unwrap_or_default()
        } else {
            base.to_owned()
        };
        self.0.push(name.clone());
        name
    }

    fn contains(&self, name: &str) -> bool {
        self.0.iter().any(|n| n == name)
    }
}

fn single_origins(rows: &[usize]) -> Vec<Option<RowOrigin>> {
    rows.iter().map(|&row| Some(RowOrigin { source: 0, row })).collect()
}

/// `table` restricted to `rows` (in the order given) and `columns`.
fn take(table: &DataTable, rows: &[usize], columns: &[usize], name: &str) -> Result<DataTable> {
    let cols = columns.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;
    DataTable::new(name, cols.into_iter().map(|c| gather(c, rows.iter().map(|&r| Some(r)))).collect())
}

fn all_columns(table: &DataTable) -> Vec<usize> {
    (0..table.n_cols()).collect()
}

// ----------------------------------------------------------------------------
// Rows: sort, duplicates, subset, sample
// ----------------------------------------------------------------------------

/// One sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    /// Column index.
    pub column: usize,
    /// Largest first instead of smallest first.
    pub descending: bool,
}

/// The row order that sorts `table` by `keys`: entry `i` is the source row that
/// goes to position `i`.
///
/// The sort is stable (rows that tie on every key keep their order). Numbers
/// and date-times sort by value, text in [`natural_order`] (so `item 2` comes
/// before `item 10` only when both parse as numbers; otherwise by character),
/// and missing values go last in either direction.
///
/// # Errors
/// [`StatError::InvalidArgument`] without keys, or for an unknown or repeated
/// key column.
pub fn sort_order(table: &DataTable, keys: &[SortKey]) -> Result<Vec<usize>> {
    if keys.is_empty() {
        return Err(invalid("choose at least one column to sort by"));
    }
    check_distinct(&keys.iter().map(|k| k.column).collect::<Vec<_>>(), "the sort")?;
    let cols =
        keys.iter().map(|k| column_at(table, k.column).map(|c| (c, k.descending))).collect::<Result<Vec<_>>>()?;
    let mut order: Vec<usize> = (0..table.n_rows()).collect();
    order.sort_by(|&a, &b| {
        cols.iter().map(|(c, d)| compare_cells(c, a, b, *d)).find(|o| o.is_ne()).unwrap_or(Ordering::Equal)
    });
    Ok(order)
}

/// `table` sorted by `keys` (see [`sort_order`]).
///
/// # Errors
/// As for [`sort_order`].
pub fn sorted(table: &DataTable, keys: &[SortKey], name: &str) -> Result<Derived> {
    let order = sort_order(table, keys)?;
    let table = take(table, &order, &all_columns(table), name)?;
    Ok(Derived { table, origins: single_origins(&order), notes: Vec::new() })
}

/// Rows that repeat an earlier row on `columns` (all columns when empty),
/// ascending. The first occurrence of each combination is not listed; two
/// missing values count as equal.
///
/// # Errors
/// [`StatError::InvalidArgument`] for an unknown or repeated column.
pub fn duplicate_rows(table: &DataTable, columns: &[usize]) -> Result<Vec<usize>> {
    let columns = if columns.is_empty() { all_columns(table) } else { columns.to_vec() };
    check_distinct(&columns, "the duplicate check")?;
    let cols = columns.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;
    let mut seen = std::collections::HashSet::new();
    Ok((0..table.n_rows()).filter(|&r| !seen.insert(row_key(&cols, r))).collect())
}

/// `table` without the rows [`duplicate_rows`] finds.
///
/// # Errors
/// As for [`duplicate_rows`].
pub fn without_duplicates(table: &DataTable, columns: &[usize], name: &str) -> Result<Derived> {
    let dups = duplicate_rows(table, columns)?;
    let keep: Vec<usize> = (0..table.n_rows()).filter(|r| dups.binary_search(r).is_err()).collect();
    let notes = vec![match dups.len() {
        0 => "No duplicate rows were found.".to_owned(),
        1 => "Removed 1 duplicate row.".to_owned(),
        n => format!("Removed {n} duplicate rows."),
    }];
    Ok(Derived { table: take(table, &keep, &all_columns(table), name)?, origins: single_origins(&keep), notes })
}

/// The given `rows` (in that order) and `columns` (in that order) of `table`.
///
/// # Errors
/// [`StatError::InvalidArgument`] without columns, for an unknown or repeated
/// column, or a row out of range.
pub fn subset(table: &DataTable, rows: &[usize], columns: &[usize], name: &str) -> Result<Derived> {
    if columns.is_empty() {
        return Err(invalid("choose at least one column"));
    }
    check_distinct(columns, "the subset")?;
    if let Some(&r) = rows.iter().find(|&&r| r >= table.n_rows()) {
        return Err(invalid(format!("no row {}", r + 1)));
    }
    Ok(Derived { table: take(table, rows, columns, name)?, origins: single_origins(rows), notes: Vec::new() })
}

/// How many rows a random sample takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SampleSize {
    /// Exactly this many rows.
    Count(usize),
    /// This fraction of the eligible rows (in `[0, 1]`), rounded to the
    /// nearest whole number of rows.
    Fraction(f64),
}

/// Selection sampling (Knuth, Algorithm S): `n` of `rows`, each subset of size
/// `n` equally likely, kept in their original order.
fn select(rows: &[usize], n: usize, rng: &mut Rng) -> Vec<usize> {
    let total = rows.len();
    let mut out = Vec::with_capacity(n);
    for (t, &r) in rows.iter().enumerate() {
        if out.len() == n {
            break;
        }
        // Select with probability (n − m) / (N − t).
        if rng.next_f64() * ((total - t) as f64) < (n - out.len()) as f64 {
            out.push(r);
        }
    }
    out
}

/// Proportional allocation of `n` among strata of the given sizes, rounded by
/// largest remainder (ties to the earlier stratum).
fn allocate(n: usize, sizes: &[usize]) -> Vec<usize> {
    let total: usize = sizes.iter().sum();
    if total == 0 {
        return vec![0; sizes.len()];
    }
    let quotas: Vec<f64> = sizes.iter().map(|&s| n as f64 * s as f64 / total as f64).collect();
    let mut counts: Vec<usize> = quotas.iter().map(|q| q.floor() as usize).collect();
    let short = n - counts.iter().sum::<usize>();
    let mut by_remainder: Vec<usize> = (0..sizes.len()).collect();
    by_remainder.sort_by(|&a, &b| {
        let (ra, rb) = (quotas[a] - quotas[a].floor(), quotas[b] - quotas[b].floor());
        rb.total_cmp(&ra).then(a.cmp(&b))
    });
    for &h in by_remainder.iter().take(short) {
        counts[h] += 1;
    }
    counts
}

/// A random sample of the `eligible` rows of `table`, ascending.
///
/// Without `strata` every subset of the requested size is equally likely
/// (selection sampling). With `strata` (columns), the eligible rows are split
/// by their values on those columns (two missing values match) and the total
/// is allocated to the strata in proportion to their sizes, rounded by largest
/// remainder; each stratum is then sampled independently. The same `seed`
/// always gives the same sample.
///
/// # Errors
/// [`StatError::InvalidArgument`] if a count exceeds the number of eligible
/// rows, a fraction lies outside `[0, 1]`, an eligible row is out of range or
/// a stratum column is unknown or repeated.
pub fn sample_rows(
    table: &DataTable,
    eligible: &[usize],
    size: SampleSize,
    strata: &[usize],
    seed: u64,
) -> Result<Vec<usize>> {
    if eligible.iter().any(|&r| r >= table.n_rows()) {
        return Err(invalid("an eligible row is out of range"));
    }
    let total = eligible.len();
    let n = match size {
        SampleSize::Count(n) if n <= total => n,
        SampleSize::Count(n) => {
            return Err(invalid(format!("cannot sample {n} rows from {total}")));
        }
        SampleSize::Fraction(f) if (0.0..=1.0).contains(&f) => (f * total as f64).round() as usize,
        SampleSize::Fraction(f) => return Err(invalid(format!("a sampling fraction must lie in [0, 1] (got {f})"))),
    };
    let mut rng = Rng::new(seed);
    let mut out = if strata.is_empty() {
        select(eligible, n, &mut rng)
    } else {
        check_distinct(strata, "the strata")?;
        let cols = strata.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;
        let mut index: HashMap<Vec<KeyPart>, usize> = HashMap::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for &r in eligible {
            let next = groups.len();
            let g = *index.entry(row_key(&cols, r)).or_insert(next);
            if g == next {
                groups.push(Vec::new());
            }
            groups[g].push(r);
        }
        let counts = allocate(n, &groups.iter().map(Vec::len).collect::<Vec<_>>());
        groups.iter().zip(counts).flat_map(|(g, k)| select(g, k, &mut rng)).collect()
    };
    out.sort_unstable();
    Ok(out)
}

// ----------------------------------------------------------------------------
// Merge: combine rows, join
// ----------------------------------------------------------------------------

/// The rows of `tables` one after another, matching columns by name.
///
/// Columns appear in the order they are first met. A column that a table lacks
/// is missing in that table's rows. A column whose type differs between tables
/// becomes text (date-times shown differently keep the first display). With
/// `source_column`, a text column of that name records each row's table name
/// (made unique if a data column already has the name).
///
/// # Errors
/// [`StatError::InvalidArgument`] without tables.
pub fn combine_rows(tables: &[&DataTable], source_column: Option<&str>, name: &str) -> Result<Derived> {
    if tables.is_empty() {
        return Err(invalid("choose at least one table"));
    }
    let mut order: Vec<&str> = Vec::new();
    for t in tables {
        for c in t.columns() {
            if !order.contains(&c.name.as_str()) {
                order.push(&c.name);
            }
        }
    }
    let lengths: Vec<usize> = tables.iter().map(|t| t.n_rows()).collect();
    let mut names = Names::default();
    let mut notes = Vec::new();
    let mut columns = Vec::with_capacity(order.len() + 1);
    for col_name in &order {
        let parts: Vec<Option<&Column>> = tables.iter().map(|t| t.find(col_name).and_then(|i| t.column(i))).collect();
        let present: Vec<&Column> = parts.iter().flatten().copied().collect();
        let (kind, note) = unify_kind(&present);
        notes.extend(note);
        let cells = parts.iter().zip(&lengths).flat_map(|(p, &n)| (0..n).map(move |r| p.map(|c| (c, r))));
        columns.push(build(names.unique(col_name), kind, level_for(kind, present[0]), cells));
    }
    if let Some(source) = source_column {
        let unique = names.unique(source);
        if unique != source {
            notes.push(format!("The source column is called {unique:?} because {source:?} was taken."));
        }
        let labels =
            tables.iter().zip(&lengths).flat_map(|(t, &n)| std::iter::repeat_n(t.name().to_owned(), n)).collect();
        columns.push(Column::text(unique, labels));
    }
    let origins = lengths
        .iter()
        .enumerate()
        .flat_map(|(source, &n)| (0..n).map(move |row| Some(RowOrigin { source, row })))
        .collect();
    Ok(Derived { table: DataTable::new(name, columns)?, origins, notes })
}

/// Which rows a join keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoinKind {
    /// Only rows with a match in both tables.
    Inner,
    /// Every left row; right values missing where there is no match.
    Left,
    /// Every right row; left values missing where there is no match.
    Right,
    /// Every row of both tables.
    Full,
}

/// What the join expects of its keys; a violation stops the join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relationship {
    /// No expectation: every pair of matching rows is joined.
    Any,
    /// Each key appears at most once in the right table.
    ManyToOne,
    /// Each key appears at most once in each table.
    OneToOne,
}

fn storage_class(kind: ColumnKind) -> u8 {
    match kind {
        ColumnKind::Numeric => 0,
        ColumnKind::Text => 1,
        ColumnKind::DateTime(_) => 2,
    }
}

fn key_text(cols: &[&Column], row: usize) -> String {
    cols.iter().map(|c| c.cell_text(row)).collect::<Vec<_>>().join(", ")
}

/// First key value (as text) that occurs on more than one row of `cols`.
fn repeated_key(cols: &[&Column], n_rows: usize) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    (0..n_rows)
        .find(|&r| {
            let k = row_key(cols, r);
            !k.contains(&KeyPart::Missing) && !seen.insert(k)
        })
        .map(|r| key_text(cols, r))
}

/// Join `left` and `right` on pairs of key columns `(left, right)`.
///
/// Rows match when every key pair holds equal, non-missing values. The result
/// lists left rows in order (each with its matches in right-table order, or
/// alone for left and full joins), then the right rows that matched nothing
/// (right and full joins). Each key appears once, holding the left value or,
/// on right-only rows, the right value. The other right columns follow the
/// left columns; a right column whose name is taken gets the right table's
/// name in parentheses.
///
/// Row states can follow a left row that appears exactly once in the result.
///
/// # Errors
/// [`StatError::InvalidArgument`] without keys, for an unknown or repeated key
/// column, key columns of different types (text, numbers, date-times), keys
/// that break `relationship`, or when no row matches (including an empty
/// right table).
pub fn join(
    left: &DataTable,
    right: &DataTable,
    keys: &[(usize, usize)],
    kind: JoinKind,
    relationship: Relationship,
    name: &str,
) -> Result<Derived> {
    if keys.is_empty() {
        return Err(invalid("choose at least one pair of key columns"));
    }
    let left_keys: Vec<usize> = keys.iter().map(|k| k.0).collect();
    let right_keys: Vec<usize> = keys.iter().map(|k| k.1).collect();
    check_distinct(&left_keys, "the left keys")?;
    check_distinct(&right_keys, "the right keys")?;
    let lk = left_keys.iter().map(|&c| column_at(left, c)).collect::<Result<Vec<_>>>()?;
    let rk = right_keys.iter().map(|&c| column_at(right, c)).collect::<Result<Vec<_>>>()?;
    for (a, b) in lk.iter().zip(&rk) {
        if storage_class(a.kind()) != storage_class(b.kind()) {
            return Err(invalid(format!("key columns {:?} and {:?} hold different types of values", a.name, b.name)));
        }
    }
    if relationship != Relationship::Any
        && let Some(k) = repeated_key(&rk, right.n_rows())
    {
        return Err(invalid(format!("the right table has more than one row with key {k}")));
    }
    if relationship == Relationship::OneToOne
        && let Some(k) = repeated_key(&lk, left.n_rows())
    {
        return Err(invalid(format!("the left table has more than one row with key {k}")));
    }

    let mut index: HashMap<Vec<KeyPart>, Vec<usize>> = HashMap::new();
    for r in 0..right.n_rows() {
        let k = row_key(&rk, r);
        if !k.contains(&KeyPart::Missing) {
            index.entry(k).or_default().push(r);
        }
    }
    let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
    let mut right_used = vec![false; right.n_rows()];
    let mut left_alone = 0;
    for l in 0..left.n_rows() {
        let k = row_key(&lk, l);
        match index.get(&k).filter(|_| !k.contains(&KeyPart::Missing)) {
            Some(matches) => {
                for &r in matches {
                    right_used[r] = true;
                    pairs.push((Some(l), Some(r)));
                }
            }
            None => {
                left_alone += 1;
                if matches!(kind, JoinKind::Left | JoinKind::Full) {
                    pairs.push((Some(l), None));
                }
            }
        }
    }
    let right_alone = right_used.iter().filter(|u| !**u).count();
    if matches!(kind, JoinKind::Right | JoinKind::Full) {
        pairs.extend((0..right.n_rows()).filter(|&r| !right_used[r]).map(|r| (None, Some(r))));
    }

    let mut names = Names::default();
    let mut columns = Vec::with_capacity(left.n_cols() + right.n_cols());
    for (i, col) in left.columns().iter().enumerate() {
        let name = names.unique(&col.name);
        let cells: Vec<Option<(&Column, usize)>> = match left_keys.iter().position(|&c| c == i) {
            Some(k) => pairs.iter().map(|&(l, r)| l.map(|l| (col, l)).or_else(|| r.map(|r| (rk[k], r)))).collect(),
            None => pairs.iter().map(|&(l, _)| l.map(|l| (col, l))).collect(),
        };
        columns.push(build(name, col.kind(), col.level, cells.into_iter()));
    }
    let mut notes = Vec::new();
    for (i, col) in right.columns().iter().enumerate() {
        if right_keys.contains(&i) {
            continue;
        }
        let base =
            if names.contains(&col.name) { format!("{} ({})", col.name, right.name()) } else { col.name.clone() };
        let name = names.unique(&base);
        if name != col.name {
            notes.push(format!("{:?} from {:?} is called {name:?}.", col.name, right.name()));
        }
        columns.push(gather(col, pairs.iter().map(|&(_, r)| r)).renamed(name));
    }
    // A join where no row found a partner is almost always the wrong table
    // or key, and would open a document that only looks like a result.
    if !pairs.iter().any(|(l, r)| l.is_some() && r.is_some()) {
        return Err(invalid(if right.n_rows() == 0 {
            format!("{:?} has no rows to join.", right.name())
        } else {
            format!(
                "no rows matched: no key value appears in both {:?} and {:?}. Check the key columns.",
                left.name(),
                right.name()
            )
        }));
    }
    let mut count = vec![0usize; left.n_rows()];
    for (l, _) in &pairs {
        if let Some(l) = l {
            count[*l] += 1;
        }
    }
    let origins =
        pairs.iter().map(|&(l, _)| l.filter(|&l| count[l] == 1).map(|row| RowOrigin { source: 0, row })).collect();
    notes.insert(
        0,
        format!(
            "{} matched pairs; {left_alone} left rows and {right_alone} right rows had no match.",
            pairs.iter().filter(|(l, r)| l.is_some() && r.is_some()).count()
        ),
    );
    Ok(Derived { table: DataTable::new(name, columns)?, origins, notes })
}

impl Column {
    fn renamed(mut self, name: String) -> Self {
        self.name = name;
        self
    }
}

// ----------------------------------------------------------------------------
// Reshape: stack, unstack, transpose
// ----------------------------------------------------------------------------

/// Wide → long: the chosen `columns` become two columns, `label_name` (the
/// column each value came from, as text) and `value_name` (the value). Each
/// source row becomes one row per stacked column, in column order, with the
/// other columns repeated. Missing values are kept as rows.
///
/// Stacked columns of different types give a text value column.
///
/// # Errors
/// [`StatError::InvalidArgument`] without columns, for an unknown or repeated
/// column, or if the two new names are blank, equal, or used by a kept column.
pub fn stack(table: &DataTable, columns: &[usize], label_name: &str, value_name: &str, name: &str) -> Result<Derived> {
    if columns.is_empty() {
        return Err(invalid("choose at least one column to stack"));
    }
    check_distinct(columns, "the stack")?;
    let stacked = columns.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;
    let (label_name, value_name) = (label_name.trim(), value_name.trim());
    if label_name.is_empty() || value_name.is_empty() {
        return Err(invalid("the label and value columns need names"));
    }
    if label_name == value_name {
        return Err(invalid("the label and value columns need different names"));
    }
    let kept: Vec<&Column> =
        table.columns().iter().enumerate().filter(|(i, _)| !columns.contains(i)).map(|(_, c)| c).collect();
    if let Some(c) = kept.iter().find(|c| c.name == label_name || c.name == value_name) {
        return Err(invalid(format!("there is already a column named {:?}", c.name)));
    }
    let k = stacked.len();
    let rows: Vec<usize> = (0..table.n_rows()).flat_map(|r| std::iter::repeat_n(r, k)).collect();
    let mut out: Vec<Column> = kept.iter().map(|c| gather(c, rows.iter().map(|&r| Some(r)))).collect();
    out.push(Column::text(label_name, rows.iter().enumerate().map(|(i, _)| stacked[i % k].name.clone()).collect()));
    let (kind, note) = unify_kind(&stacked);
    let cells = rows.iter().enumerate().map(|(i, &r)| Some((stacked[i % k], r)));
    out.push(build(value_name.to_owned(), kind, level_for(kind, stacked[0]), cells));
    let notes = note
        .map(|_| format!("The stacked columns hold different types of values, so {value_name:?} is text."))
        .into_iter()
        .collect();
    Ok(Derived { table: DataTable::new(name, out)?, origins: vec![None; rows.len()], notes })
}

/// Long → wide: one row per combination of the `ids` columns (in order of first
/// appearance; two missing values match) and, for each `values` column, one
/// column per level of `label` (levels in natural order). A single value
/// column's new columns are named by the level; with several, by
/// `"{value column} {level}"`. Rows with a missing label are left out (and
/// counted in a note); cells with no row are missing.
///
/// # Errors
/// [`StatError::InvalidArgument`] without value columns, for unknown or
/// repeated columns, a column used in two roles, a label with no levels, or
/// two rows with the same IDs and label (summarize them first).
pub fn unstack(table: &DataTable, label: usize, values: &[usize], ids: &[usize], name: &str) -> Result<Derived> {
    if values.is_empty() {
        return Err(invalid("choose at least one value column"));
    }
    let mut roles: Vec<usize> = vec![label];
    roles.extend(values);
    roles.extend(ids);
    check_distinct(&roles, "unstacking (a column can have only one role)")?;
    let label_col = column_at(table, label)?;
    let value_cols = values.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;
    let id_cols = ids.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;

    let mut levels: Vec<String> =
        (0..table.n_rows()).filter(|&r| !is_missing(label_col, r)).map(|r| label_col.cell_text(r)).collect();
    levels.sort_by(|a, b| natural_order(a, b));
    levels.dedup();
    if levels.is_empty() {
        return Err(invalid(format!("{:?} has no values to make columns from", label_col.name)));
    }
    let mut group_of: HashMap<Vec<KeyPart>, usize> = HashMap::new();
    let mut first_rows: Vec<usize> = Vec::new();
    // cell[(group, level)] = source row
    let mut cells: HashMap<(usize, usize), usize> = HashMap::new();
    let mut skipped = 0;
    for r in 0..table.n_rows() {
        if is_missing(label_col, r) {
            skipped += 1;
            continue;
        }
        let next = first_rows.len();
        let g = *group_of.entry(row_key(&id_cols, r)).or_insert(next);
        if g == next {
            first_rows.push(r);
        }
        let text = label_col.cell_text(r);
        let level = levels.iter().position(|l| *l == text).unwrap_or_default();
        match cells.entry((g, level)) {
            Entry::Vacant(e) => {
                e.insert(r);
            }
            Entry::Occupied(e) => {
                let ids = if id_cols.is_empty() { String::new() } else { format!("ID {} and ", key_text(&id_cols, r)) };
                return Err(invalid(format!(
                    "rows {} and {} have the same {ids}label {text:?}; summarize them by group first",
                    e.get() + 1,
                    r + 1
                )));
            }
        }
    }
    let mut names = Names::default();
    let mut out: Vec<Column> = id_cols
        .iter()
        .map(|c| {
            let col = gather(c, first_rows.iter().map(|&r| Some(r)));
            let unique = names.unique(&col.name);
            col.renamed(unique)
        })
        .collect();
    let mut notes = Vec::new();
    for v in &value_cols {
        for (li, level) in levels.iter().enumerate() {
            let base = if value_cols.len() == 1 { level.clone() } else { format!("{} {level}", v.name) };
            let unique = names.unique(&base);
            if unique != base {
                notes.push(format!("The column for {base:?} is called {unique:?} because the name was taken."));
            }
            let cells = (0..first_rows.len()).map(|g| cells.get(&(g, li)).map(|&r| (*v, r)));
            out.push(build(unique, v.kind(), v.level, cells));
        }
    }
    if skipped > 0 {
        notes.push(format!("{skipped} rows with no {:?} value were left out.", label_col.name));
    }
    Ok(Derived { table: DataTable::new(name, out)?, origins: vec![None; first_rows.len()], notes })
}

/// Rows ↔ columns: each chosen column becomes a row and each row a column.
///
/// The first result column, `Column`, holds the transposed columns' names.
/// Each new column is named by the row's value in `label` (when given and not
/// missing) or `Row n`; names are made unique. The new columns share the
/// transposed columns' type when they all have the same one, and are text
/// otherwise.
///
/// # Errors
/// [`StatError::InvalidArgument`] without columns, for an unknown or repeated
/// column, or if `label` is also transposed.
pub fn transpose(table: &DataTable, columns: &[usize], label: Option<usize>, name: &str) -> Result<Derived> {
    if columns.is_empty() {
        return Err(invalid("choose at least one column to transpose"));
    }
    check_distinct(columns, "the transpose")?;
    if label.is_some_and(|l| columns.contains(&l)) {
        return Err(invalid("the column of names cannot also be transposed"));
    }
    let cols = columns.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;
    let label_col = label.map(|l| column_at(table, l)).transpose()?;
    let (kind, note) = unify_kind(&cols);
    let level = if kind == ColumnKind::Text {
        if cols.iter().all(|c| c.level == cols[0].level) { level_for(kind, cols[0]) } else { MeasurementLevel::Nominal }
    } else if cols.iter().all(|c| c.level == cols[0].level) {
        cols[0].level
    } else {
        MeasurementLevel::Continuous
    };
    let mut names = Names::default();
    let mut out = vec![Column::text(names.unique("Column"), cols.iter().map(|c| c.name.clone()).collect())];
    for r in 0..table.n_rows() {
        let base = label_col
            .filter(|c| !is_missing(c, r))
            .map_or_else(|| format!("Row {}", r + 1), |c| c.cell_text(r).trim().to_owned());
        let cells = cols.iter().map(|c| Some((*c, r)));
        out.push(build(names.unique(&base), kind, level, cells));
    }
    let notes = note
        .map(|_| "The transposed columns hold different types of values, so the new columns are text.".to_owned())
        .into_iter()
        .collect();
    Ok(Derived { table: DataTable::new(name, out)?, origins: vec![None; cols.len()], notes })
}

// ----------------------------------------------------------------------------
// Summaries
// ----------------------------------------------------------------------------

/// A statistic [`summarize`] can compute for each group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Statistic {
    /// Number of non-missing values.
    Count,
    /// Number of missing values.
    Missing,
    /// Arithmetic mean (compensated summation).
    Mean,
    /// Sample standard deviation (divisor `n − 1`; missing for `n < 2`).
    StdDev,
    /// Smallest value.
    Min,
    /// Largest value.
    Max,
    /// Median.
    Median,
    /// The quantile at this probability (in `[0, 1]`), by the chosen
    /// Hyndman–Fan definition.
    Quantile(f64),
    /// Sum (compensated).
    Sum,
}

impl Statistic {
    /// Short label used in result column names, e.g. `mean`, `25% quantile`.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Count => "N".into(),
            Self::Missing => "missing".into(),
            Self::Mean => "mean".into(),
            Self::StdDev => "SD".into(),
            Self::Min => "min".into(),
            Self::Max => "max".into(),
            Self::Median => "median".into(),
            Self::Quantile(p) => format!("{}% quantile", format_number((p * 100.0 * 1e9).round() / 1e9)),
            Self::Sum => "sum".into(),
        }
    }
}

/// What [`summarize`] computes.
#[derive(Debug, Clone, PartialEq)]
pub struct SummarizeOptions {
    /// Grouping columns (none: one row for all rows).
    pub groups: Vec<usize>,
    /// Columns to summarize.
    pub columns: Vec<usize>,
    /// Statistics for each summarized column.
    pub statistics: Vec<Statistic>,
    /// Quantile definition for [`Statistic::Quantile`] (Hyndman & Fan 1996);
    /// [`Statistic::Median`] is always the conventional median (type 2).
    pub quantile_method: QuantileMethod,
}

/// Statistics of `columns` for each group of `rows` (the rows to use, e.g. the
/// rows not excluded).
///
/// Groups are the distinct combinations of the grouping columns' values among
/// `rows` (two missing values match), listed in sort order (missing last). The
/// result has the grouping columns, `Rows` (rows in the group), then one
/// column per summarized column and statistic, named `"{column} {statistic}"`.
/// Statistics use the group's non-missing values; with none, a location is
/// missing (and a sum zero). Date-time columns give locations (mean, median,
/// quantiles, minimum, maximum) as date-times and the standard deviation as a
/// duration; the sum is given only for durations. Text columns give counts
/// only. Each restriction is reported in a note.
///
/// # Errors
/// [`StatError::InvalidArgument`] without columns or statistics, for unknown or
/// repeated columns, a column both grouped and summarized, a quantile
/// probability outside `[0, 1]`, or a row out of range.
pub fn summarize(table: &DataTable, rows: &[usize], options: &SummarizeOptions, name: &str) -> Result<Derived> {
    if options.columns.is_empty() || options.statistics.is_empty() {
        return Err(invalid("choose at least one column and one statistic"));
    }
    let mut roles = options.groups.clone();
    roles.extend(&options.columns);
    check_distinct(&roles, "summarizing (a column cannot be both grouped and summarized)")?;
    if let Some(Statistic::Quantile(p)) =
        options.statistics.iter().find(|s| matches!(s, Statistic::Quantile(p) if !(0.0..=1.0).contains(p)))
    {
        return Err(invalid(format!("a quantile probability must lie in [0, 1] (got {p})")));
    }
    if rows.iter().any(|&r| r >= table.n_rows()) {
        return Err(invalid("a row is out of range"));
    }
    let group_cols = options.groups.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;
    let cols = options.columns.iter().map(|&c| column_at(table, c)).collect::<Result<Vec<_>>>()?;

    let mut index: HashMap<Vec<KeyPart>, usize> = HashMap::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for &r in rows {
        let next = members.len();
        let g = *index.entry(row_key(&group_cols, r)).or_insert(next);
        if g == next {
            members.push(Vec::new());
        }
        members[g].push(r);
    }
    members.sort_by(|a, b| {
        group_cols.iter().map(|c| compare_cells(c, a[0], b[0], false)).find(|o| o.is_ne()).unwrap_or(Ordering::Equal)
    });
    if members.is_empty() && group_cols.is_empty() {
        members.push(Vec::new());
    }

    let mut names = Names::default();
    let mut out: Vec<Column> = group_cols
        .iter()
        .map(|c| {
            let col = gather(c, members.iter().map(|m| m.first().copied()));
            let unique = names.unique(&col.name);
            col.renamed(unique)
        })
        .collect();
    out.push(Column::numeric(names.unique("Rows"), members.iter().map(|m| m.len() as f64).collect()));
    let mut notes = Vec::new();
    for col in &cols {
        let display = col.temporal();
        let is_text = col.as_numeric().is_none();
        let instants = display.is_some_and(|d| d.format != TemporalFormat::Duration);
        let mut skipped = Vec::new();
        // Each group's non-missing values, sorted.
        let groups: Vec<(Vec<f64>, usize, usize)> = members
            .iter()
            .map(|m| {
                let mut v: Vec<f64> = col
                    .as_numeric()
                    .map_or_else(Vec::new, |x| m.iter().map(|&r| x[r]).filter(|x| !x.is_nan()).collect());
                v.sort_by(f64::total_cmp);
                let missing = m.iter().filter(|&&r| is_missing(col, r)).count();
                (v, missing, m.len())
            })
            .collect();
        for &stat in &options.statistics {
            let counts = matches!(stat, Statistic::Count | Statistic::Missing);
            if !counts && (is_text || (instants && stat == Statistic::Sum)) {
                skipped.push(stat.label());
                continue;
            }
            let values: Vec<f64> = groups
                .iter()
                .map(|(v, missing, rows)| group_statistic(stat, v, rows - missing, *missing, options.quantile_method))
                .collect();
            let column_name = names.unique(&format!("{} {}", col.name, stat.label()));
            let column = match (stat, display) {
                (Statistic::Count | Statistic::Missing, _) | (_, None) => Column::numeric(column_name, values),
                (Statistic::StdDev, Some(d)) => Column::date_time(
                    column_name,
                    values,
                    TemporalDisplay { format: TemporalFormat::Duration, offset_minutes: 0, ..d },
                ),
                (_, Some(d)) => Column::date_time(column_name, values, d),
            };
            out.push(column);
        }
        if !skipped.is_empty() {
            let why = if is_text { "it holds text" } else { "it holds dates or times" };
            notes.push(format!("{:?}: no {} because {why}.", col.name, skipped.join(", ")));
        }
    }
    Ok(Derived { table: DataTable::new(name, out)?, origins: vec![None; members.len()], notes })
}

/// One statistic of a group's sorted non-missing `values`.
/// `count` is the number of non-missing values (text included), `values` the
/// sorted numbers among them.
fn group_statistic(stat: Statistic, values: &[f64], count: usize, missing: usize, method: QuantileMethod) -> f64 {
    let q = |p: f64, m: QuantileMethod| quantile_sorted(values, p, m).unwrap_or(f64::NAN);
    match stat {
        Statistic::Count => count as f64,
        Statistic::Missing => missing as f64,
        Statistic::Sum if values.is_empty() => 0.0,
        _ if values.is_empty() => f64::NAN,
        Statistic::Mean | Statistic::StdDev | Statistic::Sum => match moments(values) {
            Ok(m) => match stat {
                Statistic::Mean => m.mean,
                Statistic::StdDev => m.std_dev.unwrap_or(f64::NAN),
                _ => m.sum,
            },
            Err(_) => f64::NAN,
        },
        Statistic::Min => values[0],
        Statistic::Max => values[values.len() - 1],
        // The conventional median (middle value, or the average of the two
        // middle values) whatever `method` is: Hyndman–Fan type 2 at 0.5,
        // which types 5–9 match but types 1, 3 and 4 need not.
        Statistic::Median => q(0.5, QuantileMethod::AveragedInverseCdf),
        Statistic::Quantile(p) => q(p, method),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::CellValue;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    fn people() -> DataTable {
        DataTable::new(
            "people",
            vec![
                Column::text("name", s(&["Ann", "Bo", "Cy", "Di", "Ed", "Flo"])),
                Column::text("group", s(&["b", "a", "b", "", "a", "b"])),
                Column::numeric("x", vec![3.0, 1.0, f64::NAN, 2.0, 1.0, 5.0]),
            ],
        )
        .unwrap()
    }

    fn texts(t: &DataTable, c: &str) -> Vec<String> {
        let col = t.column(t.find(c).unwrap()).unwrap();
        (0..t.n_rows()).map(|r| col.cell_text(r)).collect()
    }

    #[test]
    fn natural_order_puts_numbers_first_and_by_value() {
        let mut v = s(&["b", "10", "2", "a", "1.5"]);
        v.sort_by(|a, b| natural_order(a, b));
        assert_eq!(v, s(&["1.5", "2", "10", "a", "b"]));
    }

    #[test]
    fn sort_is_stable_with_missing_last_both_ways() {
        let t = people();
        let up = sort_order(&t, &[SortKey { column: 2, descending: false }]).unwrap();
        assert_eq!(up, vec![1, 4, 3, 0, 5, 2]); // ties (Bo, Ed) keep order; missing last
        let down = sort_order(&t, &[SortKey { column: 2, descending: true }]).unwrap();
        assert_eq!(down, vec![5, 0, 3, 1, 4, 2]);
        let two =
            sorted(&t, &[SortKey { column: 1, descending: false }, SortKey { column: 2, descending: true }], "sorted")
                .unwrap();
        assert_eq!(texts(&two.table, "name"), s(&["Bo", "Ed", "Flo", "Ann", "Cy", "Di"]));
        assert_eq!(two.origins[0], Some(RowOrigin { source: 0, row: 1 }));
        assert!(sort_order(&t, &[]).is_err());
        assert!(sort_order(&t, &[SortKey { column: 9, descending: false }]).is_err());
    }

    #[test]
    fn sorting_dates_and_negative_zero() {
        let d = TemporalDisplay::new(TemporalFormat::Date);
        let t = DataTable::new(
            "d",
            vec![
                Column::date_time("when", vec![86_400.0, f64::NAN, 0.0, -86_400.0], d),
                Column::numeric("z", vec![0.0, -0.0, 1.0, 0.0]),
            ],
        )
        .unwrap();
        assert_eq!(sort_order(&t, &[SortKey { column: 0, descending: false }]).unwrap(), vec![3, 2, 0, 1]);
        // -0 and 0 tie, so the stable sort keeps their order.
        assert_eq!(sort_order(&t, &[SortKey { column: 1, descending: false }]).unwrap(), vec![0, 1, 3, 2]);
        assert_eq!(duplicate_rows(&t, &[1]).unwrap(), vec![1, 3]);
    }

    #[test]
    fn duplicates_keep_first_and_match_missing() {
        let t = people();
        assert_eq!(duplicate_rows(&t, &[1]).unwrap(), vec![2, 4, 5]);
        assert_eq!(duplicate_rows(&t, &[1, 2]).unwrap(), vec![4]);
        assert!(duplicate_rows(&t, &[]).unwrap().is_empty());
        let d = without_duplicates(&t, &[1], "d").unwrap();
        assert_eq!(texts(&d.table, "name"), s(&["Ann", "Bo", "Di"]));
        assert_eq!(d.notes, vec!["Removed 3 duplicate rows.".to_owned()]);
        let blank = DataTable::new("b", vec![Column::text("g", s(&["", " ", "x"]))]).unwrap();
        assert_eq!(duplicate_rows(&blank, &[]).unwrap(), vec![1]);
    }

    #[test]
    fn subset_rows_and_columns() {
        let t = people();
        let d = subset(&t, &[4, 0], &[2, 0], "sub").unwrap();
        assert_eq!(d.table.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["x", "name"]);
        assert_eq!(texts(&d.table, "name"), s(&["Ed", "Ann"]));
        assert_eq!(d.origins, vec![Some(RowOrigin { source: 0, row: 4 }), Some(RowOrigin { source: 0, row: 0 })]);
        assert!(subset(&t, &[6], &[0], "x").is_err());
        assert!(subset(&t, &[0], &[], "x").is_err());
        assert!(subset(&t, &[0], &[0, 0], "x").is_err());
    }

    #[test]
    fn samples_are_reproducible_and_sized() {
        let t = people();
        let all: Vec<usize> = (0..6).collect();
        let a = sample_rows(&t, &all, SampleSize::Count(3), &[], 7).unwrap();
        assert_eq!(a, sample_rows(&t, &all, SampleSize::Count(3), &[], 7).unwrap());
        assert_eq!(a.len(), 3);
        assert!(a.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(sample_rows(&t, &all, SampleSize::Fraction(0.5), &[], 1).unwrap().len(), 3);
        assert_eq!(sample_rows(&t, &all, SampleSize::Count(6), &[], 1).unwrap(), all);
        assert!(sample_rows(&t, &all, SampleSize::Count(0), &[], 1).unwrap().is_empty());
        assert!(sample_rows(&t, &all, SampleSize::Count(7), &[], 1).is_err());
        assert!(sample_rows(&t, &all, SampleSize::Fraction(1.5), &[], 1).is_err());
        // Only eligible rows are drawn.
        let some = [1, 3, 5];
        assert!(sample_rows(&t, &some, SampleSize::Count(2), &[], 3).unwrap().iter().all(|r| some.contains(r)));
    }

    #[test]
    fn stratified_samples_allocate_in_proportion() {
        assert_eq!(allocate(5, &[3, 2, 1]), vec![2, 2, 1]); // quotas 2.5, 1.67, 0.83
        assert_eq!(allocate(4, &[3, 2, 1]), vec![2, 1, 1]);
        assert_eq!(allocate(3, &[1, 1, 1, 1]), vec![1, 1, 1, 0]);
        assert_eq!(allocate(0, &[2, 2]), vec![0, 0]);
        let t = people();
        let all: Vec<usize> = (0..6).collect();
        for seed in 0..50 {
            // Strata: b (3 rows), a (2), missing (1); 4 rows → 2, 1, 1.
            let rows = sample_rows(&t, &all, SampleSize::Count(4), &[1], seed).unwrap();
            let count = |g: &str| rows.iter().filter(|&&r| t.column(1).unwrap().cell_text(r) == g).count();
            assert_eq!((count("b"), count("a"), count("")), (2, 1, 1), "seed {seed}");
        }
    }

    #[test]
    fn selection_sampling_is_uniform() {
        // 20 rows, samples of 5, 10 000 seeds: each row's inclusion count is
        // Binomial(10 000, 1/4); a chi-square test of equal counts must pass.
        let t = DataTable::new("u", vec![Column::numeric("i", (0..20).map(f64::from).collect())]).unwrap();
        let rows: Vec<usize> = (0..20).collect();
        let mut hits = [0usize; 20];
        let draws = 10_000;
        for seed in 0..draws {
            for r in sample_rows(&t, &rows, SampleSize::Count(5), &[], seed).unwrap() {
                hits[r] += 1;
            }
        }
        let expected = draws as f64 * 5.0 / 20.0;
        let chi2: f64 = hits.iter().map(|&h| (h as f64 - expected).powi(2) / expected).sum();
        // Each count has variance n p (1 − p), so scale to the binomial: chi2 · 4/3 ~ χ²(19) approximately.
        let p = 1.0
            - stat_core::ContinuousDistribution::cdf(&stat_core::ChiSquared::new(19.0).unwrap(), chi2 / 0.75).unwrap();
        assert!(p > 0.001, "chi2 {chi2}, p {p}, hits {hits:?}");
    }

    #[test]
    fn combine_matches_by_name_and_fills_missing() {
        let a =
            DataTable::new("A", vec![Column::numeric("x", vec![1.0, 2.0]), Column::text("g", s(&["p", "q"]))]).unwrap();
        let b = DataTable::new("B", vec![Column::numeric("y", vec![9.0]), Column::numeric("x", vec![3.0])]).unwrap();
        let d = combine_rows(&[&a, &b], Some("Source"), "AB").unwrap();
        let names: Vec<&str> = d.table.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["x", "g", "y", "Source"]);
        assert_eq!(texts(&d.table, "x"), s(&["1", "2", "3"]));
        assert_eq!(texts(&d.table, "g"), s(&["p", "q", ""]));
        assert_eq!(texts(&d.table, "y"), s(&["", "", "9"]));
        assert_eq!(texts(&d.table, "Source"), s(&["A", "A", "B"]));
        assert_eq!(d.origins[2], Some(RowOrigin { source: 1, row: 0 }));
        assert!(d.notes.is_empty());
        // Combining with an empty table returns the table.
        let empty = DataTable::new("E", vec![Column::numeric("x", vec![]), Column::text("g", vec![])]).unwrap();
        assert_eq!(combine_rows(&[&a, &empty], None, "A").unwrap().table, a);
    }

    #[test]
    fn combine_mixed_types_become_text() {
        let a = DataTable::new("A", vec![Column::numeric("v", vec![1.5])]).unwrap();
        let b = DataTable::new("B", vec![Column::text("v", s(&["high"]))]).unwrap();
        let d = combine_rows(&[&a, &b], Some("v"), "AB").unwrap();
        assert_eq!(d.table.column(0).unwrap().kind(), ColumnKind::Text);
        assert_eq!(d.table.column(0).unwrap().level, MeasurementLevel::Nominal);
        assert_eq!(texts(&d.table, "v"), s(&["1.5", "high"]));
        assert_eq!(d.table.column(1).unwrap().name, "v 2");
        assert_eq!(d.notes.len(), 2);
        let dd =
            DataTable::new("D", vec![Column::date_time("v", vec![0.0], TemporalDisplay::new(TemporalFormat::Date))])
                .unwrap();
        let dt = DataTable::new(
            "T",
            vec![Column::date_time("v", vec![60.0], TemporalDisplay::new(TemporalFormat::DateTime))],
        )
        .unwrap();
        let d = combine_rows(&[&dd, &dt], None, "DT").unwrap();
        assert_eq!(d.table.column(0).unwrap().as_numeric().unwrap(), &[0.0, 60.0]);
        assert!(d.table.column(0).unwrap().temporal().is_some());
    }

    fn orders() -> (DataTable, DataTable) {
        let left = DataTable::new(
            "orders",
            vec![
                Column::text("cust", s(&["c1", "c2", "c3", "", "c1"])),
                Column::numeric("amount", vec![10.0, 20.0, 30.0, 40.0, 50.0]),
            ],
        )
        .unwrap();
        let right = DataTable::new(
            "customers",
            vec![
                Column::text("region", s(&["N", "S", "E", "W"])),
                Column::text("cust", s(&["c1", "c2", "c9", ""])),
                Column::numeric("amount", vec![1.0, 2.0, 3.0, 4.0]),
            ],
        )
        .unwrap();
        (left, right)
    }

    #[test]
    fn join_kinds() {
        let (l, r) = orders();
        let keys = [(0, 1)];
        let inner = join(&l, &r, &keys, JoinKind::Inner, Relationship::ManyToOne, "j").unwrap();
        let names: Vec<&str> = inner.table.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["cust", "amount", "region", "amount (customers)"]);
        assert_eq!(texts(&inner.table, "cust"), s(&["c1", "c2", "c1"]));
        assert_eq!(texts(&inner.table, "region"), s(&["N", "S", "N"]));
        let left = join(&l, &r, &keys, JoinKind::Left, Relationship::Any, "j").unwrap();
        assert_eq!(texts(&left.table, "amount"), s(&["10", "20", "30", "40", "50"]));
        assert_eq!(texts(&left.table, "region"), s(&["N", "S", "", "", "N"]));
        assert_eq!(left.origins[2], Some(RowOrigin { source: 0, row: 2 }));
        let right = join(&l, &r, &keys, JoinKind::Right, Relationship::Any, "j").unwrap();
        // Missing keys never match: the right row with a blank key stays alone.
        assert_eq!(texts(&right.table, "cust"), s(&["c1", "c2", "c1", "c9", ""]));
        assert_eq!(texts(&right.table, "amount"), s(&["10", "20", "50", "", ""]));
        assert_eq!(right.origins[3], None);
        let full = join(&l, &r, &keys, JoinKind::Full, Relationship::Any, "j").unwrap();
        assert_eq!(full.table.n_rows(), 7);
        assert_eq!(full.notes[0], "3 matched pairs; 2 left rows and 2 right rows had no match.");
        // Nothing matching is refused rather than reported as a success.
        let all: Vec<usize> = (0..r.n_cols()).collect();
        let empty = subset(&r, &[], &all, "none").unwrap().table;
        let e = join(&l, &empty, &keys, JoinKind::Left, Relationship::Any, "j").unwrap_err().to_string();
        assert!(e.contains("has no rows to join"), "{e}");
        let e = join(&l, &r, &[(1, 2)], JoinKind::Full, Relationship::Any, "j").unwrap_err().to_string();
        assert!(e.starts_with("no rows matched"), "{e}");
    }

    #[test]
    fn join_checks() {
        let (l, r) = orders();
        assert!(join(&l, &r, &[(0, 1)], JoinKind::Inner, Relationship::OneToOne, "j").is_err());
        let err = join(&r, &l, &[(1, 0)], JoinKind::Inner, Relationship::ManyToOne, "j").unwrap_err();
        assert!(err.to_string().contains("c1"), "{err}");
        assert!(join(&l, &r, &[(0, 2)], JoinKind::Inner, Relationship::Any, "j").is_err());
        assert!(join(&l, &r, &[], JoinKind::Inner, Relationship::Any, "j").is_err());
        // One-to-many from the left: repeated left rows lose their origin.
        let many = join(&r, &l, &[(1, 0)], JoinKind::Inner, Relationship::Any, "j").unwrap();
        assert_eq!(many.origins, vec![None, None, Some(RowOrigin { source: 0, row: 1 })]);
    }

    /// Small deterministic tables for the brute-force comparison.
    fn random_table(name: &str, rows: usize, seed: u64) -> DataTable {
        let mut rng = Rng::new(seed);
        let key = |rng: &mut Rng| match rng.below(5) {
            0 => String::new(),
            k => format!("k{k}"),
        };
        let k1: Vec<String> = (0..rows).map(|_| key(&mut rng)).collect();
        let k2: Vec<f64> = (0..rows).map(|_| if rng.below(6) == 0 { f64::NAN } else { rng.below(2) as f64 }).collect();
        let v: Vec<f64> = (0..rows).map(|i| i as f64).collect();
        DataTable::new(name, vec![Column::text("k1", k1), Column::numeric("k2", k2), Column::numeric("v", v)]).unwrap()
    }

    /// The SQL definition, as nested loops.
    fn nested_loop(l: &DataTable, r: &DataTable, kind: JoinKind) -> Vec<(Option<usize>, Option<usize>)> {
        let lk = [l.column(0).unwrap(), l.column(1).unwrap()];
        let rk = [r.column(0).unwrap(), r.column(1).unwrap()];
        let matches = |a: usize, b: usize| {
            lk.iter().zip(&rk).all(|(x, y)| !is_missing(x, a) && !is_missing(y, b) && x.value(a) == y.value(b))
        };
        let mut out = Vec::new();
        let mut used = vec![false; r.n_rows()];
        for a in 0..l.n_rows() {
            let mut any = false;
            for b in 0..r.n_rows() {
                if matches(a, b) {
                    out.push((Some(a), Some(b)));
                    used[b] = true;
                    any = true;
                }
            }
            if !any && matches!(kind, JoinKind::Left | JoinKind::Full) {
                out.push((Some(a), None));
            }
        }
        if matches!(kind, JoinKind::Right | JoinKind::Full) {
            out.extend((0..r.n_rows()).filter(|&b| !used[b]).map(|b| (None, Some(b))));
        }
        out
    }

    #[test]
    fn hash_join_equals_nested_loop_join() {
        for seed in 0..40 {
            let l = random_table("L", 12, seed);
            let r = random_table("R", 9, seed + 1000);
            for kind in [JoinKind::Inner, JoinKind::Left, JoinKind::Right, JoinKind::Full] {
                let got = join(&l, &r, &[(0, 0), (1, 1)], kind, Relationship::Any, "j").unwrap().table;
                let pairs = nested_loop(&l, &r, kind);
                assert_eq!(got.n_rows(), pairs.len(), "seed {seed} {kind:?}");
                for (i, (a, b)) in pairs.iter().enumerate() {
                    let lv = a.map(|a| l.column(2).unwrap().value(a));
                    let rv = b.map(|b| r.column(2).unwrap().value(b));
                    assert_eq!(
                        lv.unwrap_or(CellValue::Number(f64::NAN)).to_string_lossy(),
                        got.column(2).unwrap().cell_text(i)
                    );
                    assert_eq!(
                        rv.unwrap_or(CellValue::Number(f64::NAN)).to_string_lossy(),
                        got.column(3).unwrap().cell_text(i)
                    );
                }
            }
        }
    }

    impl CellValue {
        fn to_string_lossy(&self) -> String {
            match self {
                CellValue::Number(x) if x.is_nan() => String::new(),
                CellValue::Number(x) => format_number(*x),
                CellValue::Text(t) => t.clone(),
            }
        }
    }

    fn wide() -> DataTable {
        DataTable::new(
            "wide",
            vec![
                Column::text("id", s(&["s1", "s2", "s3"])),
                Column::numeric("before", vec![5.0, f64::NAN, 7.0]),
                Column::numeric("after", vec![6.0, 8.0, 9.0]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn stack_then_unstack_round_trips() {
        let t = wide();
        let long = stack(&t, &[1, 2], "time", "score", "long").unwrap();
        assert_eq!(long.table.n_rows(), 6);
        assert_eq!(texts(&long.table, "id"), s(&["s1", "s1", "s2", "s2", "s3", "s3"]));
        assert_eq!(texts(&long.table, "time"), s(&["before", "after", "before", "after", "before", "after"]));
        assert_eq!(texts(&long.table, "score"), s(&["5", "6", "", "8", "7", "9"]));
        let back = unstack(&long.table, 1, &[2], &[0], "wide").unwrap();
        // Levels come out in natural order (after, before), so compare by name.
        for c in ["id", "before", "after"] {
            assert_eq!(texts(&back.table, c), texts(&t, c));
        }
        assert_eq!(back.table.n_cols(), 3);
    }

    #[test]
    fn stack_checks_names_and_types() {
        let t = wide();
        assert!(stack(&t, &[1], "id", "v", "x").is_err());
        assert!(stack(&t, &[1], "v", "v", "x").is_err());
        assert!(stack(&t, &[], "l", "v", "x").is_err());
        let d = stack(&t, &[0, 1], "l", "v", "x").unwrap();
        assert_eq!(d.table.column(d.table.find("v").unwrap()).unwrap().kind(), ColumnKind::Text);
        assert_eq!(d.notes.len(), 1);
    }

    #[test]
    fn unstack_reports_duplicates_and_missing_labels() {
        let long = DataTable::new(
            "long",
            vec![
                Column::text("id", s(&["a", "a", "b", "b"])),
                Column::text("k", s(&["x", "y", "x", ""])),
                Column::numeric("v", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("w", vec![5.0, 6.0, 7.0, 8.0]),
            ],
        )
        .unwrap();
        let d = unstack(&long, 1, &[2, 3], &[0], "wide").unwrap();
        let names: Vec<&str> = d.table.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "v x", "v y", "w x", "w y"]);
        assert_eq!(texts(&d.table, "v y"), s(&["2", ""]));
        assert_eq!(d.notes, vec!["1 rows with no \"k\" value were left out.".to_owned()]);
        let err = unstack(&long, 1, &[2], &[], "wide").unwrap_err();
        assert!(err.to_string().contains("rows 1 and 3"), "{err}");
        assert!(unstack(&long, 1, &[1], &[0], "wide").is_err());
    }

    #[test]
    fn transpose_twice_returns_the_values() {
        let t = wide();
        let once = transpose(&t, &[1, 2], Some(0), "t").unwrap();
        let names: Vec<&str> = once.table.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Column", "s1", "s2", "s3"]);
        assert_eq!(texts(&once.table, "s2"), s(&["", "8"]));
        let twice = transpose(&once.table, &[1, 2, 3], Some(0), "tt").unwrap();
        assert_eq!(texts(&twice.table, "Column"), s(&["s1", "s2", "s3"]));
        assert_eq!(texts(&twice.table, "before"), texts(&t, "before"));
        assert_eq!(texts(&twice.table, "after"), texts(&t, "after"));
        // Mixed types become text; missing labels become row numbers.
        let mixed = transpose(&people(), &[0, 2], None, "m").unwrap();
        assert_eq!(mixed.table.column(1).unwrap().name, "Row 1");
        assert_eq!(texts(&mixed.table, "Row 1"), s(&["Ann", "3"]));
        assert_eq!(mixed.notes.len(), 1);
        let dup =
            DataTable::new("d", vec![Column::text("n", s(&["a", "a", ""])), Column::numeric("v", vec![1.0, 2.0, 3.0])])
                .unwrap();
        let names: Vec<String> =
            transpose(&dup, &[1], Some(0), "d").unwrap().table.columns().iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, ["Column", "a", "a 2", "Row 3"]);
    }

    fn options(groups: Vec<usize>, columns: Vec<usize>, statistics: Vec<Statistic>) -> SummarizeOptions {
        SummarizeOptions { groups, columns, statistics, quantile_method: QuantileMethod::default() }
    }

    #[test]
    fn summarize_by_group() {
        let t = people();
        let all: Vec<usize> = (0..6).collect();
        let stats = vec![Statistic::Count, Statistic::Missing, Statistic::Mean, Statistic::StdDev, Statistic::Sum];
        let d = summarize(&t, &all, &options(vec![1], vec![2], stats), "sum").unwrap();
        let names: Vec<&str> = d.table.columns().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["group", "Rows", "x N", "x missing", "x mean", "x SD", "x sum"]);
        assert_eq!(texts(&d.table, "group"), s(&["a", "b", ""]));
        assert_eq!(texts(&d.table, "Rows"), s(&["2", "3", "1"]));
        assert_eq!(texts(&d.table, "x N"), s(&["2", "2", "1"]));
        assert_eq!(texts(&d.table, "x missing"), s(&["0", "1", "0"]));
        assert_eq!(texts(&d.table, "x mean"), s(&["1", "4", "2"]));
        assert_eq!(texts(&d.table, "x SD"), s(&["0", &format_number(2f64.sqrt()), ""]));
        assert_eq!(texts(&d.table, "x sum"), s(&["2", "8", "2"]));
        // Without groups: one row; only included rows count.
        let d = summarize(&t, &[0, 1], &options(vec![], vec![2], vec![Statistic::Max]), "s").unwrap();
        assert_eq!(texts(&d.table, "x max"), s(&["3"]));
        let none = summarize(&t, &[], &options(vec![], vec![2], vec![Statistic::Mean, Statistic::Sum]), "s").unwrap();
        assert_eq!(
            (texts(&none.table, "Rows"), texts(&none.table, "x mean"), texts(&none.table, "x sum")),
            (s(&["0"]), s(&[""]), s(&["0"]))
        );
    }

    #[test]
    fn summarize_quantiles_and_types() {
        let d = TemporalDisplay::new(TemporalFormat::Date);
        let t = DataTable::new(
            "t",
            vec![
                Column::date_time("day", vec![0.0, 86_400.0 * 2.0, 86_400.0 * 4.0], d),
                Column::text("note", s(&["a", "", "c"])),
                Column::numeric("x", vec![1.0, 2.0, 4.0]),
            ],
        )
        .unwrap();
        let stats =
            vec![Statistic::Count, Statistic::Median, Statistic::StdDev, Statistic::Sum, Statistic::Quantile(0.25)];
        let out = summarize(&t, &[0, 1, 2], &options(vec![], vec![0, 1, 2], stats), "s").unwrap().table;
        assert_eq!(texts(&out, "day median"), s(&["1970-01-03"]));
        assert_eq!(
            out.column(out.find("day SD").unwrap()).unwrap().temporal().unwrap().format,
            TemporalFormat::Duration
        );
        assert!(out.find("day sum").is_none());
        assert!(out.find("note median").is_none());
        assert_eq!(texts(&out, "note N"), s(&["2"]));
        // Type 8 quantile of 1, 2, 4 at p = 0.25: position (n + 1/3) p + 1/3 = 7/6.
        let q = out.column(out.find("x 25% quantile").unwrap()).unwrap().as_numeric().unwrap()[0];
        assert!((q - 7.0 / 6.0).abs() < 1e-12, "{q}");
        assert!(summarize(&t, &[0], &options(vec![2], vec![2], vec![Statistic::Mean]), "s").is_err());
        assert!(summarize(&t, &[0], &options(vec![], vec![2], vec![Statistic::Quantile(2.0)]), "s").is_err());
    }
}
