//! # stat-data
//!
//! Data shared by every view of the application:
//!
//! * [`table`] — a columnar [`DataTable`] with measurement levels.
//! * [`row_state`] — the [`RowStateManager`] that owns selection, exclusion,
//!   hiding and color for every row, the single source of truth for linked
//!   brushing across views.
//! * [`delimited`] — delimited text (CSV, tab, semicolon, pipe) reading with
//!   delimiter detection and type inference, and writing.
//! * [`json`] — a small JSON value model with a parser and writer, and
//!   conversion between tables and JSON arrays of row objects.
//! * [`table_ops`] — sort, remove duplicates, subset and sample, combine
//!   rows, join, stack, unstack, transpose and summarize by group.
//! * [`temporal`] — dates, times of day and durations: calendar arithmetic,
//!   ISO 8601 parsing and formatting, spreadsheet serial dates.

pub mod delimited;
pub mod json;
pub mod row_state;
pub mod table;
pub mod table_ops;
pub mod temporal;

pub use delimited::{
    detect_delimiter, parse_records, read_csv, read_delimited, read_delimited_with_notes, renamed_note,
    table_from_records, table_from_records_with_notes, unique_names, write_delimited,
};
pub use json::{JsonValue, flatten_object, parse_json, read_json_table, table_from_json_rows, write_json_table};
pub use row_state::{MAX_SUBSETS, RowColor, RowCounts, RowMarker, RowState, RowStateManager, SelectionMode};
pub use table::{
    CellValue, Column, ColumnData, ColumnKind, DataTable, MISSING_MARKERS, MeasurementLevel, format_number,
    is_missing_marker,
};
pub use table_ops::{Derived, RowOrigin, natural_order};
pub use temporal::{TemporalDisplay, TemporalFormat};

/// A revision number no table or row-state change has used before in this
/// process: every [`DataTable`] edit and every saved row-state change takes
/// the next one, so equal revisions mean equal contents even across tables
/// that replaced one another (which per-object counters starting at zero
/// cannot promise).
pub(crate) fn next_revision() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
