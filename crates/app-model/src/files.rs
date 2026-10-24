//! Reading and writing data files, including the binary spreadsheet
//! formats (`calamine` to read `.xlsx`/`.xlsm`/`.xlsb`/`.xls`/`.ods`,
//! `rust_xlsxwriter` to write `.xlsx`). Shared by the desktop application
//! and scripts; recognising formats and the text readers and writers live in
//! [`crate::file_formats`].

use std::path::Path;

use stat_data::{ColumnData, DataTable, JsonValue, format_number};

use crate::file_formats::{
    ExportFormat, FileKind, OPEN_EXTENSIONS, Opened, name_from_path, read_text_file, read_text_with,
    spreadsheet_date_text, text_sheet,
};
use crate::open_data::{OpenSettings, RawSheet, detect_settings, stack_sheets, table_from_sheet};

/// A data file read as cells, before the Open Data dialog (or detected
/// settings) picks the table from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawFile {
    /// Delimited text, decoded.
    Text {
        /// The file's kind ([`FileKind::Csv`], [`FileKind::Tsv`] or
        /// [`FileKind::DelimitedText`]).
        kind: FileKind,
        /// The text.
        text: String,
        /// How it was decoded, when that is worth saying.
        notes: Vec<String>,
    },
    /// A workbook: every sheet, empty ones included, in workbook order.
    Workbook(Vec<RawSheet>),
}

/// Decode a text file's bytes: UTF-8 (a byte-order mark is dropped), UTF-16
/// with a byte-order mark or with the zero bytes of mostly-ASCII UTF-16
/// text, else Windows-1252 (Latin-1), with a note saying so.
#[must_use]
pub fn decode_text(bytes: Vec<u8>) -> (String, Option<String>) {
    let utf16 = |little: bool, body: &[u8]| -> String {
        let units = body
            .chunks_exact(2)
            .map(|p| if little { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) });
        char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
    };
    match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => return (utf16(true, rest), None),
        [0xFE, 0xFF, rest @ ..] => return (utf16(false, rest), None),
        _ => {}
    }
    // UTF-16 without a byte-order mark: ASCII text has a zero byte in every
    // other position.
    let sample = &bytes[..bytes.len().min(512) & !1];
    if sample.len() >= 4 {
        let zeros = |offset: usize| sample.iter().skip(offset).step_by(2).filter(|&&b| b == 0).count();
        let half = sample.len() / 2;
        if zeros(1) * 10 >= half * 9 && zeros(0) == 0 {
            return (utf16(true, &bytes), None);
        }
        if zeros(0) * 10 >= half * 9 && zeros(1) == 0 {
            return (utf16(false, &bytes), None);
        }
    }
    match String::from_utf8(bytes) {
        Ok(t) => (t.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(t), None),
        // Accept Latin-1/Windows-1252 text from older tools rather than refusing it.
        Err(e) => (
            e.into_bytes().iter().map(|&b| windows_1252(b)).collect(),
            Some("The file is not UTF-8 text; it was read as Windows-1252 (Western European).".to_owned()),
        ),
    }
}

/// The character Windows-1252 assigns to `b` (Latin-1 apart from 0x80–0x9F).
fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    if (0x80..0xA0).contains(&b) { HIGH[usize::from(b - 0x80)] } else { char::from(b) }
}

/// Read the data file at `path` as cells (a delimited text file or a
/// workbook); `None` for a project or JSON file, which has no choices to
/// make.
///
/// # Errors
/// An unsupported extension or an unreadable file.
pub fn read_raw(path: &Path) -> Result<Option<RawFile>, String> {
    let kind = file_kind(path)?;
    if kind == FileKind::Spreadsheet {
        return read_sheets(path).map(|s| Some(RawFile::Workbook(s)));
    }
    if !kind.is_delimited() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let (text, note) = decode_text(bytes);
    Ok(Some(RawFile::Text { kind, text, notes: note.into_iter().collect() }))
}

fn file_kind(path: &Path) -> Result<FileKind, String> {
    FileKind::from_path(path).ok_or_else(|| {
        format!("{} is not a supported file type (expected one of: {})", path.display(), OPEN_EXTENSIONS.join(", "))
    })
}

/// Open the file at `path`. A workbook yields one table per non-empty sheet;
/// every other format yields exactly one item (an image is a long table of
/// pixels, [`crate::image_table`]). Tables are read with
/// detected settings ([`crate::open_data::detect_settings`]).
///
/// # Errors
/// An unsupported extension, an unreadable file, or content the format's
/// reader rejects.
pub fn open_path(path: &Path) -> Result<Vec<Opened>, String> {
    let name = name_from_path(path);
    if crate::image_table::is_image_path(path) {
        // An image opens as a long table with the default choices (grey,
        // reduced when larger than 1024 pixels on a side).
        let image = crate::image_table::DecodedImage::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = path.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned());
        let options = crate::image_table::ImageOpenOptions::default();
        let (table, info) = crate::image_table::image_table(&image, &name, &file, &options).map_err(|e| e.to_string())?;
        return Ok(vec![Opened::Table(table, vec![info.note()])]);
    }
    let kind = file_kind(path)?;
    if kind == FileKind::Spreadsheet {
        return read_workbook(path, &name);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let (text, note) = decode_text(bytes);
    let mut opened = read_text_file(kind, &name, &text).map_err(|e| e.to_string())?;
    if let (Some(note), Opened::Table(_, notes)) = (note, &mut opened) {
        notes.insert(0, note);
    }
    Ok(vec![opened])
}

/// Open the data file at `path` with settings given as a map (the script
/// function `open_table(path, options)`): the members of
/// [`OpenSettings::to_json`] (any left out are detected), and for a
/// workbook `sheet` (one sheet by name), or `sheets` (a list of names;
/// all non-empty sheets when left out) with `stack` (true to stack them
/// into one table) and `sheet_column` (the stacked table's column of sheet
/// names, `"Sheet"` by default). Projects and JSON files take no options.
///
/// # Errors
/// An unsupported or unreadable file, invalid options, a sheet that is not
/// in the workbook, or a block that holds nothing.
pub fn open_path_with(path: &Path, options: &JsonValue) -> Result<Vec<Opened>, String> {
    let name = name_from_path(path);
    let Some(raw) = read_raw(path)? else {
        return Err(format!("{} takes no open options (only text files and workbooks do)", path.display()));
    };
    let settings_for = |sheet: &RawSheet, delimiter: Option<char>| {
        OpenSettings::from_json(options, &detect_settings(sheet, delimiter)).map_err(|e| e.to_string())
    };
    match raw {
        RawFile::Text { kind, text, notes: decoded } => {
            if options.get("sheet").is_some() || options.get("sheets").is_some() {
                return Err(format!("{} is not a workbook, so it has no sheets", path.display()));
            }
            let given = OpenSettings::from_json(options, &OpenSettings::default()).map_err(|e| e.to_string())?;
            let (sheet, delimiter) = text_sheet(kind, &name, &text, given.delimiter).map_err(|e| e.to_string())?;
            let settings = settings_for(&sheet, Some(delimiter))?;
            let (table, mut notes) = read_text_with(kind, &name, &text, &settings).map_err(|e| e.to_string())?;
            notes.splice(0..0, decoded);
            Ok(vec![Opened::Table(table, notes)])
        }
        RawFile::Workbook(sheets) => {
            let text_list = |key: &str| -> Result<Option<Vec<String>>, String> {
                match options.get(key) {
                    None => Ok(None),
                    Some(JsonValue::String(s)) => Ok(Some(vec![s.clone()])),
                    Some(JsonValue::Array(items)) => items
                        .iter()
                        .map(|i| i.as_str().map(str::to_owned).ok_or_else(|| format!("{key} must list sheet names")))
                        .collect::<Result<Vec<_>, _>>()
                        .map(Some),
                    Some(_) => Err(format!("{key} must be a sheet name or a list of them")),
                }
            };
            let wanted = match (text_list("sheet")?, text_list("sheets")?) {
                (Some(a), Some(b)) => [a, b].concat(),
                (a, b) => a.or(b).unwrap_or_default(),
            };
            for w in &wanted {
                if !sheets.iter().any(|s| &s.name == w) {
                    let list: Vec<String> = sheets.iter().map(|s| format!("{:?}", s.name)).collect();
                    return Err(format!("{} has no sheet {w:?}; its sheets are {}", path.display(), list.join(", ")));
                }
            }
            let chosen: Vec<&RawSheet> = if wanted.is_empty() {
                sheets.iter().filter(|s| !s.is_blank()).collect()
            } else {
                wanted.iter().filter_map(|w| sheets.iter().find(|s| &s.name == w)).collect()
            };
            if chosen.is_empty() {
                return Err(format!("{} has no data", path.display()));
            }
            let settings: Vec<OpenSettings> = chosen.iter().map(|s| settings_for(s, None)).collect::<Result<_, _>>()?;
            if options.get("stack").and_then(JsonValue::as_bool).unwrap_or(false) {
                let column = options.get("sheet_column").and_then(JsonValue::as_str).unwrap_or("Sheet");
                let pairs: Vec<(&RawSheet, &OpenSettings)> = chosen.iter().copied().zip(&settings).collect();
                let (t, notes) = stack_sheets(&name, &pairs, column).map_err(|e| e.to_string())?;
                return Ok(vec![Opened::Table(t, notes)]);
            }
            let several = sheets.len() > 1 || !wanted.is_empty();
            chosen
                .iter()
                .zip(&settings)
                .map(|(s, settings)| {
                    let table_name = if several { sheet_table_name(&name, &s.name) } else { name.clone() };
                    table_from_sheet(&table_name, s, settings)
                        .map(|(t, n)| Opened::Table(t, n))
                        .map_err(|e| format!("sheet {:?}: {e}", s.name))
                })
                .collect()
        }
    }
}

/// The name of the table read from sheet `sheet` of the workbook `file`
/// when the workbook has several sheets.
#[must_use]
pub fn sheet_table_name(file: &str, sheet: &str) -> String {
    format!("{file} \u{2013} {sheet}")
}

/// Every sheet of the workbook at `path` as cells, empty sheets included.
///
/// # Errors
/// The file cannot be opened as a workbook.
pub fn read_sheets(path: &Path) -> Result<Vec<RawSheet>, String> {
    use calamine::{Reader, open_workbook_auto};
    let mut workbook = open_workbook_auto(path).map_err(|e| format!("could not open {}: {e}", path.display()))?;
    Ok(workbook.worksheets().into_iter().map(|(sheet, range)| RawSheet::new(sheet, sheet_cells(&range))).collect())
}

/// Every non-empty sheet of the workbook at `path` as a table, named
/// `name` (or `name – sheet` when there are several sheets), each read with
/// detected settings.
///
/// # Errors
/// The file cannot be opened, or has no data.
pub fn read_workbook(path: &Path, name: &str) -> Result<Vec<Opened>, String> {
    let sheets = read_sheets(path)?;
    let several = sheets.len() > 1;
    let mut tables = Vec::new();
    for sheet in &sheets {
        if sheet.is_blank() {
            if several {
                continue;
            }
            return Err(format!("sheet {:?}: the sheet is empty", sheet.name));
        }
        let table_name = if several { sheet_table_name(name, &sheet.name) } else { name.to_owned() };
        match table_from_sheet(&table_name, sheet, &detect_settings(sheet, None)) {
            Ok((t, notes)) => tables.push(Opened::Table(t, notes)),
            Err(e) => return Err(format!("sheet {:?}: {e}", sheet.name)),
        }
    }
    if tables.is_empty() {
        return Err(format!("{} has no data", path.display()));
    }
    Ok(tables)
}

/// The sheet named `sheet` of the workbook at `path` as a table named
/// `name – sheet`, with notes on cells that did not fit their column's type.
///
/// # Errors
/// The file cannot be opened, has no such sheet (the message lists the
/// sheets), or the sheet has no data.
pub fn read_sheet(path: &Path, sheet: &str, name: &str) -> Result<(DataTable, Vec<String>), String> {
    let sheets = read_sheets(path)?;
    let Some(found) = sheets.iter().find(|s| s.name == sheet) else {
        let list: Vec<String> = sheets.iter().map(|n| format!("{:?}", n.name)).collect();
        return Err(format!("{} has no sheet {sheet:?}; its sheets are {}", path.display(), list.join(", ")));
    };
    if found.is_blank() {
        return Err(format!("sheet {sheet:?}: the sheet is empty"));
    }
    table_from_sheet(&sheet_table_name(name, sheet), found, &detect_settings(found, None))
        .map_err(|e| format!("sheet {sheet:?}: {e}"))
}

/// A sheet's cells as text, dates in the form the table reader recognises.
fn sheet_cells(range: &calamine::Range<calamine::Data>) -> Vec<Vec<String>> {
    use calamine::{Data, ExcelDateTime, ExcelDateTimeType};
    // Without calamine's optional chrono support a date cell's date system and
    // kind are not exposed, but the cell compares equal to one built with the
    // same value, kind and system.
    let date_text = |dt: &ExcelDateTime| {
        let x = dt.as_f64();
        let is = |kind, date_1904| *dt == ExcelDateTime::new(x, kind, date_1904);
        let date_1904 = is(ExcelDateTimeType::DateTime, true) || is(ExcelDateTimeType::TimeDelta, true);
        let duration = is(ExcelDateTimeType::TimeDelta, date_1904);
        spreadsheet_date_text(x, date_1904, duration)
    };
    range
        .rows()
        .map(|row| {
            row.iter()
                .map(|cell| match cell {
                    Data::Empty | Data::Error(_) => String::new(),
                    Data::Float(x) => format_number(*x),
                    Data::Int(i) => i.to_string(),
                    Data::String(s) => s.clone(),
                    Data::DateTime(dt) => date_text(dt),
                    Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .collect()
}

/// `table` with each column that has value labels turned into a text column
/// of the labels (values without a label kept as they are), for an export
/// that writes labels; `None` when no column has labels.
#[must_use]
pub fn with_value_labels(table: &DataTable) -> Option<DataTable> {
    if table.columns().iter().all(|c| c.value_labels.is_empty()) {
        return None;
    }
    let columns = table
        .columns()
        .iter()
        .map(|c| {
            if c.value_labels.is_empty() {
                return c.clone();
            }
            let level =
                if c.level == stat_data::MeasurementLevel::Ordinal { c.level } else { stat_data::MeasurementLevel::Nominal };
            let text = (0..table.n_rows()).map(|r| c.level_text(r)).collect();
            stat_data::Column { level, hover: c.hover, ..stat_data::Column::text(c.name.clone(), text) }
        })
        .collect();
    DataTable::new(table.name(), columns).ok()
}

/// Write `table` to `path` in `format`.
///
/// # Errors
/// The file cannot be written.
pub fn export_table(table: &DataTable, format: ExportFormat, path: &Path) -> Result<(), String> {
    match format.write_text(table) {
        Some(text) => std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display())),
        None => write_xlsx(table, path).map_err(|e| format!("could not write {}: {e}", path.display())),
    }
}

fn write_xlsx(table: &DataTable, path: &Path) -> Result<(), rust_xlsxwriter::XlsxError> {
    use rust_xlsxwriter::{Format, Workbook};
    use stat_data::temporal::to_spreadsheet_serial;
    use stat_data::{TemporalDisplay, TemporalFormat};
    let mut workbook = Workbook::new();
    let bold = Format::new().set_bold();
    // Spreadsheet number formats for date cells (ECMA-376 §18.8.31 codes).
    let date_format = |d: &TemporalDisplay| {
        let seconds = if d.fraction_digits == 0 {
            "ss".to_owned()
        } else {
            format!("ss.{}", "0".repeat(usize::from(d.fraction_digits.min(3))))
        };
        Format::new().set_num_format(match d.format {
            TemporalFormat::Date => "yyyy-mm-dd".to_owned(),
            TemporalFormat::DateTime => format!("yyyy-mm-dd hh:mm:{seconds}"),
            TemporalFormat::TimeOfDay => format!("hh:mm:{seconds}"),
            TemporalFormat::Duration => format!("[h]:mm:{seconds}"),
        })
    };
    let sheet = workbook.add_worksheet();
    // Sheet names are limited to 31 characters and exclude []:*?/\
    let sheet_name: String =
        table.name().chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect::<String>().trim().to_owned();
    if !sheet_name.is_empty() {
        sheet.set_name(sheet_name)?;
    }
    for (j, column) in table.columns().iter().enumerate() {
        let col = u16::try_from(j).unwrap_or(u16::MAX);
        sheet.write_string_with_format(0, col, column.name.as_str(), &bold)?;
        for r in 0..table.n_rows() {
            let row = u32::try_from(r + 1).unwrap_or(u32::MAX);
            match &column.data {
                ColumnData::Numeric(v) if v[r].is_finite() => {
                    sheet.write_number(row, col, v[r])?;
                }
                ColumnData::Numeric(_) => {}
                ColumnData::Text(v) if !v[r].is_empty() => {
                    sheet.write_string(row, col, v[r].as_str())?;
                }
                ColumnData::Text(_) => {}
                ColumnData::DateTime { values, display } if values[r].is_finite() => {
                    // Dates are written at the column's display offset, as shown.
                    let x = values[r];
                    let serial = match display.format {
                        TemporalFormat::Duration | TemporalFormat::TimeOfDay => Some(x / 86_400.0),
                        _ => to_spreadsheet_serial(x + f64::from(display.offset_minutes) * 60.0),
                    };
                    match serial {
                        Some(serial) => sheet.write_number_with_format(row, col, serial, &date_format(display))?,
                        // Before 1900 the 1900 date system has no serial; keep the text.
                        None => sheet.write_string(row, col, display.iso_text(x))?,
                    };
                }
                ColumnData::DateTime { .. } => {}
            }
        }
    }
    sheet.set_freeze_panes(1, 0)?;
    workbook.save(path)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use stat_data::TemporalFormat;
    use stat_data::temporal::seconds_from_civil;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
    }

    /// Workbooks saved by LibreOffice (`tests/data/make_date_workbooks.py`)
    /// in the 1900 and 1904 date systems and as OpenDocument read back as the
    /// dates, times and durations written into them.
    #[test]
    fn reads_dates_from_libreoffice_workbooks() {
        let at = |y, m, d, h, mi, s| seconds_from_civil(y, m, d, h, mi, s);
        let dates = [
            at(2026, 10, 13, 0, 0, 0.0),
            at(1900, 3, 1, 0, 0, 0.0),
            at(2000, 2, 29, 0, 0, 0.0),
            at(1904, 1, 2, 0, 0, 0.0),
        ];
        let stamps = [at(2026, 10, 13, 15, 57, 58.0), at(1900, 3, 2, 6, 0, 0.0), at(2099, 12, 31, 23, 59, 59.0), 0.0];
        let clocks = [57_478.0, 1.0, 43_200.0, 86_399.0];
        let took = [5_400.0, 129_600.0, 0.0, 184_500.0];
        for name in ["dates_1900.xlsx", "dates_1904.xlsx", "dates_1900.ods"] {
            let opened = open_path(&fixture(name)).unwrap();
            let [Opened::Table(t, notes)] = opened.as_slice() else { panic!("{name}: one table expected") };
            assert!(notes.is_empty(), "{name}: {notes:?}");
            let column = |c: &str| t.column(t.find(c).unwrap()).unwrap();
            // OpenDocument stores a time of day as a length of time
            // (`office:time-value`), so it reads as a duration.
            let clock = if name.ends_with(".ods") { TemporalFormat::Duration } else { TemporalFormat::TimeOfDay };
            for (c, expected, format) in [
                ("date", dates, TemporalFormat::Date),
                ("stamp", stamps, TemporalFormat::DateTime),
                ("clock", clocks, clock),
                ("took", took, TemporalFormat::Duration),
            ] {
                let col = column(c);
                assert_eq!(col.temporal().map(|d| d.format), Some(format), "{name} {c}");
                assert_eq!(col.as_numeric().unwrap(), expected, "{name} {c}");
            }
        }
    }

    /// UTF-16 text (spreadsheets' "Unicode text") used to be read byte by
    /// byte as Latin-1, so every value carried zero characters and no column
    /// was numeric.
    #[test]
    fn utf16_text_is_decoded() {
        let text = "\u{feff}x\ty\n1\t2\n";
        let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let bare: Vec<u8> = text[3..].encode_utf16().flat_map(u16::to_le_bytes).collect();
        let dir = std::env::temp_dir().join(format!("anovacraft-utf16-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (i, bytes) in [le, be, bare].into_iter().enumerate() {
            let path = dir.join(format!("u{i}.txt"));
            std::fs::write(&path, bytes).unwrap();
            let opened = open_path(&path).unwrap();
            let [Opened::Table(t, notes)] = opened.as_slice() else { panic!() };
            assert!(notes.is_empty(), "{notes:?}");
            assert_eq!(t.column(0).unwrap().name, "x", "case {i}");
            assert_eq!(t.column(0).unwrap().as_numeric().unwrap(), &[1.0], "case {i}");
            assert_eq!(t.column(1).unwrap().as_numeric().unwrap(), &[2.0], "case {i}");
        }
        // Other non-UTF-8 text is read as Windows-1252, with a note.
        let path = dir.join("latin.csv");
        std::fs::write(&path, b"name,\x80\nCaf\xe9,1\n").unwrap();
        let opened = open_path(&path).unwrap();
        let [Opened::Table(t, notes)] = opened.as_slice() else { panic!() };
        assert_eq!(t.column(1).unwrap().name, "\u{20ac}");
        assert_eq!(t.column(0).unwrap().cell_text(0), "Caf\u{e9}");
        assert!(notes[0].contains("Windows-1252"), "{notes:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A workbook whose table starts at C4 below a title, read with detected
    /// and with given settings, and two sheets stacked.
    #[test]
    fn workbook_blocks_and_options() {
        use rust_xlsxwriter::Workbook;
        let dir = std::env::temp_dir().join(format!("anovacraft-blocks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("offset.xlsx");
        let mut book = Workbook::new();
        for (name, first) in [("Line 1", 1.0), ("Line 2", 10.0)] {
            let sheet = book.add_worksheet();
            sheet.set_name(name).unwrap();
            sheet.write_string(0, 0, "Weights by shift").unwrap();
            sheet.write_string(3, 2, "Shift").unwrap();
            sheet.write_string(3, 3, "Weight").unwrap();
            for i in 0..3u32 {
                sheet.write_string(4 + i, 2, ["A", "B", "C"][i as usize]).unwrap();
                sheet.write_number(4 + i, 3, first + f64::from(i)).unwrap();
            }
        }
        book.save(&path).unwrap();
        let opened = open_path(&path).unwrap();
        assert_eq!(opened.len(), 2);
        let Opened::Table(t, _) = &opened[0] else { panic!() };
        assert_eq!(t.name(), "offset \u{2013} Line 1");
        assert_eq!(t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Shift", "Weight"]);
        assert_eq!(t.n_rows(), 3);
        // Given settings: no names, values from row 5, only column 4.
        let options = stat_data::parse_json(
            r#"{"sheet": "Line 2", "header_row": 0, "data_start": 5, "first_column": 4, "last_column": 4}"#,
        )
        .unwrap();
        let opened = open_path_with(&path, &options).unwrap();
        let [Opened::Table(t, _)] = opened.as_slice() else { panic!() };
        assert_eq!(t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Column 1"]);
        assert_eq!(t.column(0).unwrap().as_numeric().unwrap(), &[10.0, 11.0, 12.0]);
        let options = stat_data::parse_json(r#"{"stack": true}"#).unwrap();
        let opened = open_path_with(&path, &options).unwrap();
        let [Opened::Table(t, _)] = opened.as_slice() else { panic!() };
        assert_eq!(t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Sheet", "Shift", "Weight"]);
        assert_eq!(t.n_rows(), 6);
        assert!(open_path_with(&path, &stat_data::parse_json(r#"{"sheet": "nope"}"#).unwrap()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Date columns exported to `.xlsx` read back unchanged.
    #[test]
    fn date_columns_survive_xlsx_export() {
        let original = open_path(&fixture("dates_1900.xlsx")).unwrap();
        let [Opened::Table(t, _)] = original.as_slice() else { panic!() };
        let dir = std::env::temp_dir().join(format!("anovacraft-dates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.xlsx");
        export_table(t, ExportFormat::Excel, &path).unwrap();
        let back = open_path(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let [Opened::Table(b, _)] = back.as_slice() else { panic!() };
        for (x, y) in t.columns().iter().zip(b.columns()) {
            assert_eq!(x.temporal().map(|d| d.format), y.temporal().map(|d| d.format), "{}", x.name);
            assert_eq!(x.as_numeric(), y.as_numeric(), "{}", x.name);
        }
    }
}
