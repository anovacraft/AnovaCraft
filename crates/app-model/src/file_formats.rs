//! Which file formats the application opens and exports, recognised by
//! extension, and the text-based readers and writers behind them.
//!
//! | Format | Extensions | Open | Export |
//! |---|---|---|---|
//! | Project (data + row states + reports) | `.acraft` | ✓ | ✓ (save) |
//! | Comma-separated values | `.csv` | ✓ | ✓ |
//! | Tab-separated values | `.tsv`, `.tab` | ✓ | ✓ |
//! | Delimited text | `.txt`, `.dat` | ✓ | — |
//! | JSON array of row objects | `.json` | ✓ | ✓ |
//! | Spreadsheet workbook | `.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods` | ✓ | `.xlsx` |
//!
//! The delimiter of every text format except tab-separated values is
//! detected ([`stat_data::detect_delimiter`]), so a `.csv` written with
//! semicolons (as spreadsheets do where the decimal mark is a comma) opens
//! correctly. Spreadsheets are binary formats read by [`crate::files`]; each
//! cell is rendered as text (date cells as ISO 8601 text, through
//! [`spreadsheet_date_text`]), so type inference is the same as for text
//! files. Text and sheets are read as a [`RawSheet`] first, and the table's
//! block (header row, first data row, columns) is detected or given
//! ([`crate::open_data`]).
//!
//! Exported data files carry values only. Measurement levels, row states and
//! reports are kept by project files.

use crate::open_data::{OpenSettings, RawSheet, detect_settings, split_text, table_from_sheet};
use crate::project::{PROJECT_EXTENSION, PROJECT_FORMAT, Project, load_project};
use stat_core::Result;
use stat_data::temporal::{TemporalDisplay, TemporalFormat, from_spreadsheet_serial};
use stat_data::{DataTable, parse_json, read_json_table, write_delimited, write_json_table};

pub use stat_data::unique_names;
use std::path::Path;

/// A kind of file the application can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A project file.
    Project,
    /// Comma-separated values (the delimiter is still detected).
    Csv,
    /// Tab-separated values.
    Tsv,
    /// Other delimited text.
    DelimitedText,
    /// A JSON array of row objects (or a project saved with a `.json` name).
    Json,
    /// A spreadsheet workbook, read by the front end.
    Spreadsheet,
}

impl FileKind {
    /// Whether the kind is delimited text, opened through the Open Data
    /// dialog.
    #[must_use]
    pub fn is_delimited(self) -> bool {
        matches!(self, Self::Csv | Self::Tsv | Self::DelimitedText)
    }

    /// The kind of file at `path`, from its extension (case-insensitive).
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            e if e == PROJECT_EXTENSION => Self::Project,
            "csv" => Self::Csv,
            "tsv" | "tab" => Self::Tsv,
            "txt" | "dat" => Self::DelimitedText,
            "json" => Self::Json,
            "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => Self::Spreadsheet,
            _ => return None,
        })
    }
}

/// Every extension [`FileKind::from_path`] recognises, for file-picker filters.
pub const OPEN_EXTENSIONS: [&str; 12] =
    [PROJECT_EXTENSION, "csv", "tsv", "tab", "txt", "dat", "json", "xlsx", "xlsm", "xlsb", "xls", "ods"];

/// A data export format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Comma-separated values.
    Csv,
    /// Tab-separated values.
    Tsv,
    /// JSON array of row objects.
    Json,
    /// Excel workbook, written by the front end.
    Excel,
}

impl ExportFormat {
    /// All export formats, in menu order.
    pub const ALL: [Self; 4] = [Self::Csv, Self::Tsv, Self::Json, Self::Excel];

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "Comma-separated (.csv)",
            Self::Tsv => "Tab-separated (.tsv)",
            Self::Json => "JSON rows (.json)",
            Self::Excel => "Excel workbook (.xlsx)",
        }
    }

    /// File extension, without the dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Json => "json",
            Self::Excel => "xlsx",
        }
    }

    /// The file contents for text formats; `None` for [`ExportFormat::Excel`].
    #[must_use]
    pub fn write_text(self, table: &DataTable) -> Option<String> {
        match self {
            Self::Csv => Some(write_delimited(table, ',')),
            Self::Tsv => Some(write_delimited(table, '\t')),
            Self::Json => Some(write_json_table(table)),
            Self::Excel => None,
        }
    }
}

/// What opening a file produced.
#[derive(Debug, Clone)]
pub enum Opened {
    /// A project: data, row states and reports.
    Project(Box<Project>),
    /// A plain data table, with notes on how its values were read (for
    /// example slash dates read month first) to show the user.
    Table(DataTable, Vec<String>),
}

/// Read a text-based file of `kind` whose contents are `text`. The table is
/// named `name` (projects keep the name they were saved with). Delimited
/// text is read with detected settings (see [`crate::open_data`]).
///
/// # Errors
/// Whatever the format's reader reports; [`stat_core::StatError::InvalidArgument`]
/// for [`FileKind::Spreadsheet`], which is not a text format.
pub fn read_text_file(kind: FileKind, name: &str, text: &str) -> Result<Opened> {
    if kind.is_delimited() {
        let (sheet, delimiter) = text_sheet(kind, name, text, None)?;
        return Ok(table(table_from_sheet(name, &sheet, &detect_settings(&sheet, Some(delimiter)))?));
    }
    Ok(match kind {
        FileKind::Project => Opened::Project(Box::new(load_project(text)?)),
        FileKind::Json => {
            let is_project = parse_json(text)?.get("format").and_then(|f| f.as_str()) == Some(PROJECT_FORMAT);
            if is_project {
                Opened::Project(Box::new(load_project(text)?))
            } else {
                Opened::Table(read_json_table(name, text)?, Vec::new())
            }
        }
        FileKind::Csv | FileKind::Tsv | FileKind::DelimitedText | FileKind::Spreadsheet => {
            return Err(stat_core::StatError::InvalidArgument { message: "spreadsheets are not text files".into() });
        }
    })
}

/// Delimited `text` of `kind` split into a sheet: at `delimiter` when given,
/// else at a tab for [`FileKind::Tsv`] and at the detected delimiter
/// otherwise. Returns the delimiter used.
///
/// # Errors
/// An unterminated quoted field.
pub fn text_sheet(kind: FileKind, name: &str, text: &str, delimiter: Option<char>) -> Result<(RawSheet, char)> {
    let delimiter = delimiter.or((kind == FileKind::Tsv).then_some('\t'));
    split_text(name, text, delimiter)
}

/// Delimited `text` of `kind` read with `settings` (their delimiter, else as
/// [`text_sheet`] picks).
///
/// # Errors
/// An unterminated quoted field, or a block that holds nothing.
pub fn read_text_with(
    kind: FileKind,
    name: &str,
    text: &str,
    settings: &OpenSettings,
) -> Result<(DataTable, Vec<String>)> {
    let (sheet, _) = text_sheet(kind, name, text, settings.delimiter)?;
    table_from_sheet(name, &sheet, settings)
}

fn table((t, notes): (DataTable, Vec<String>)) -> Opened {
    Opened::Table(t, notes)
}

/// ISO 8601 text for a spreadsheet date cell holding `serial` (days, with the
/// time of day as a fraction) in the 1900 or 1904 date system (ECMA-376 Part
/// 1 §18.17.4), so that [`table_from_cells`] reads it as a date:
///
/// * a duration cell gives an ISO 8601 duration (`PT1H30M`);
/// * a serial from 0 up to 1 in a date cell is a time of day (`15:57:58`),
///   as spreadsheets store time-only cells that way;
/// * a whole serial is a date (`2026-10-13`), anything else a date and time
///   (`2026-10-13T15:57:58`).
///
/// Empty for a serial that names no date (29 February 1900 in the 1900
/// system, or a negative serial there), which becomes a missing value.
#[must_use]
pub fn spreadsheet_date_text(serial: f64, date_1904: bool, duration: bool) -> String {
    if duration {
        let millis = (serial * 86_400e3).round();
        return TemporalDisplay::new(TemporalFormat::Duration).iso_text(millis / 1e3);
    }
    if (0.0..1.0).contains(&serial) {
        let millis = (serial * 86_400e3).round();
        return TemporalDisplay::new(TemporalFormat::TimeOfDay).iso_text(millis / 1e3);
    }
    let Some(seconds) = from_spreadsheet_serial(serial, date_1904) else { return String::new() };
    let format = if seconds % 86_400.0 == 0.0 { TemporalFormat::Date } else { TemporalFormat::DateTime };
    TemporalDisplay::new(format).iso_text(seconds)
}

/// A table from a grid of cell texts, as read from a spreadsheet, with the
/// table's block detected ([`crate::open_data::detect_block`]): title rows
/// above it are skipped, fully empty rows and columns are dropped, blank
/// header cells are named `Column N`, and repeated names get a ` (2)`,
/// ` (3)`, … suffix.
///
/// # Errors
/// [`stat_core::StatError::InvalidArgument`] if the grid has no non-empty cells.
pub fn table_from_cells(name: &str, grid: Vec<Vec<String>>) -> Result<DataTable> {
    table_from_cells_with_notes(name, grid).map(|(t, _)| t)
}

/// [`table_from_cells`], also returning notes on how values were read (see
/// [`stat_data::table_from_records_with_notes`]).
///
/// # Errors
/// As for [`table_from_cells`].
pub fn table_from_cells_with_notes(name: &str, grid: Vec<Vec<String>>) -> Result<(DataTable, Vec<String>)> {
    let sheet = RawSheet::new(name, grid);
    if sheet.is_blank() {
        return Err(stat_core::StatError::InvalidArgument { message: "the sheet is empty".into() });
    }
    table_from_sheet(name, &sheet, &detect_settings(&sheet, None))
}

/// A table name from a file path: the file name without its extension.
#[must_use]
pub fn name_from_path(path: &Path) -> String {
    path.file_stem().map_or_else(|| "Untitled".to_owned(), |s| s.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::save_project;
    use stat_data::{Column, ColumnData, RowStateManager};

    #[test]
    fn kinds_from_extensions() {
        let k = |p: &str| FileKind::from_path(Path::new(p));
        assert_eq!(k("a/b.CSV"), Some(FileKind::Csv));
        assert_eq!(k("b.acraft"), Some(FileKind::Project));
        assert_eq!(k("b.ACRAFT"), Some(FileKind::Project));
        assert_eq!(k("old.statproj"), None, "the pre-release extension is not supported");
        assert_eq!(k("b.tab"), Some(FileKind::Tsv));
        assert_eq!(k("b.dat"), Some(FileKind::DelimitedText));
        assert_eq!(k("b.Xlsx"), Some(FileKind::Spreadsheet));
        assert_eq!(k("b.ods"), Some(FileKind::Spreadsheet));
        assert_eq!(k("b.json"), Some(FileKind::Json));
        assert_eq!(k("b.pdf"), None);
        assert_eq!(k("noext"), None);
        for e in OPEN_EXTENSIONS {
            assert!(k(&format!("x.{e}")).is_some(), "{e}");
        }
        assert_eq!(name_from_path(Path::new("/x/y/data.v2.csv")), "data.v2");
    }

    #[test]
    fn every_text_export_reads_back() {
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, f64::NAN, -3.25]),
                Column::text("g", vec!["a".into(), "b, c".into(), String::new()]),
                Column::date_time(
                    "d",
                    vec![1_791_907_078.25, f64::NAN, -86_400.0],
                    stat_data::TemporalDisplay {
                        format: stat_data::TemporalFormat::DateTime,
                        fraction_digits: 2,
                        offset_minutes: 120,
                    },
                ),
            ],
        )
        .unwrap();
        for f in ExportFormat::ALL {
            let Some(text) = f.write_text(&t) else {
                assert_eq!(f, ExportFormat::Excel);
                continue;
            };
            let kind = FileKind::from_path(Path::new(&format!("out.{}", f.extension()))).unwrap();
            let Opened::Table(back, _) = read_text_file(kind, "t", &text).unwrap() else { panic!("table expected") };
            let x = back.column(0).unwrap().as_numeric().unwrap();
            assert_eq!((x[0], x[2]), (1.0, -3.25), "{f:?}");
            assert!(x[1].is_nan());
            assert_eq!(back.column(1).unwrap().data, t.column(1).unwrap().data, "{f:?}");
            assert_eq!(back.column(2).unwrap().data, t.column(2).unwrap().data, "{f:?}");
        }
    }

    #[test]
    fn spreadsheet_dates() {
        assert_eq!(spreadsheet_date_text(46_308.0, false, false), "2026-10-13");
        assert_eq!(spreadsheet_date_text(46_308.0 + 57_478.0 / 86_400.0, false, false), "2026-10-13T15:57:58");
        assert_eq!(spreadsheet_date_text(44_846.0, true, false), "2026-10-13", "1904 system");
        assert_eq!(spreadsheet_date_text(0.5, false, false), "12:00:00");
        assert_eq!(spreadsheet_date_text(1.5, false, true), "P1DT12H");
        assert_eq!(spreadsheet_date_text(60.0, false, false), "", "29 February 1900");
        let g = |rows: &[&[String]]| rows.iter().map(|r| r.to_vec()).collect();
        let cell = |x: f64| spreadsheet_date_text(x, false, false);
        let t = table_from_cells("s", g(&[&["d".into()], &[cell(46_308.0)], &[cell(46_309.25)]])).unwrap();
        let d = t.column(0).unwrap();
        assert_eq!(d.temporal().unwrap().format, stat_data::TemporalFormat::DateTime);
        assert_eq!(d.cell_text(1), "2026-10-14 06:00:00");
        let (_, notes) =
            table_from_cells_with_notes("s", g(&[&["d".into()], &["3/4/2026".into()], &["5/6/2026".into()]])).unwrap();
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn projects_open_from_either_extension() {
        let table = crate::sample::longley();
        let rows = RowStateManager::new(table.n_rows());
        let text =
            save_project(&Project { external: None, origin: None, scripts: Vec::new(), table, rows, reports: vec![] });
        for kind in [FileKind::Project, FileKind::Json] {
            assert!(matches!(read_text_file(kind, "x", &text).unwrap(), Opened::Project(_)));
        }
        assert!(read_text_file(FileKind::Spreadsheet, "x", "").is_err());
        let detected = read_text_file(FileKind::DelimitedText, "x", "a;b\n1;2\n").unwrap();
        assert!(matches!(detected, Opened::Table(t, _) if t.n_cols() == 2));
    }

    /// A `.csv` written with semicolons (decimal-comma locales) used to be
    /// split at commas only and fail or open as one column.
    #[test]
    fn csv_delimiter_is_detected() {
        let Opened::Table(t, _) = read_text_file(FileKind::Csv, "d", "a;b\n1,5;2,5\n").unwrap() else { panic!() };
        assert_eq!(t.n_cols(), 2);
        assert_eq!(t.column(0).unwrap().data, ColumnData::Numeric(vec![1.5]), "decimal comma detected");
        let Opened::Table(t, _) = read_text_file(FileKind::Csv, "d", "x;y\nA;B\n").unwrap() else { panic!() };
        assert_eq!(t.n_cols(), 2);
        // Tab-separated files keep the tab.
        let Opened::Table(t, _) = read_text_file(FileKind::Tsv, "d", "a,b\tc\n1\t2\n").unwrap() else { panic!() };
        assert_eq!(t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a,b", "c"]);
        // Repeated and blank names open, renamed as a workbook's are.
        let Opened::Table(t, notes) = read_text_file(FileKind::Csv, "d", "a,a,\n1,2,3\n").unwrap() else { panic!() };
        assert_eq!(t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "a (2)", "Column 3"]);
        assert_eq!(notes.len(), 1);
        let g = vec![vec!["a".to_owned(), "a".to_owned(), String::new()], vec!["1".into(), "2".into(), "3".into()]];
        assert_eq!(
            table_from_cells("d", g).unwrap().columns().iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
            ["a", "a (2)", "Column 3"]
        );
    }

    #[test]
    fn spreadsheet_cells() {
        let g = |rows: &[&[&str]]| rows.iter().map(|r| r.iter().map(|s| (*s).to_owned()).collect()).collect();
        let t = table_from_cells(
            "s",
            g(&[&["x", "", "x", ""], &["1", "", "a", ""], &[], &["2.5", "", "b"], &["", "", "", ""]]),
        )
        .unwrap();
        // Empty column 2 and 4 are dropped, the blank row is skipped, the duplicate name is suffixed.
        assert_eq!(t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["x", "x (2)"]);
        assert_eq!(t.column(0).unwrap().data, ColumnData::Numeric(vec![1.0, 2.5]));
        assert_eq!(t.n_rows(), 2);
        assert!(table_from_cells("s", g(&[&["", ""], &[]])).is_err());
        assert_eq!(
            unique_names(vec![String::new(), "a".into(), "a".into(), "a (2)".into()]),
            ["Column 1", "a", "a (2)", "a (2) (2)"]
        );
    }
}
