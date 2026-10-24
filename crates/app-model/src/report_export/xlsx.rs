//! Reports as Excel workbooks: one sheet per report, its open sections in
//! order down the sheet. Section titles are bold, tables have a bold
//! header row, and a cell showing a plain number (`0.0312`, `−2.5`,
//! `4.3e-6`, `1,234.5`, `12.5%`) is stored as that number with a number
//! format that shows it in the same precision, so it can be computed with;
//! anything else (`< 0.0001`, `7.25 ▲`, labels) stays text. Plots are
//! inserted as 300 ppi PNG pictures at their document size.

use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Image, Workbook, Worksheet, XlsxError};
use stat_viz::export::VectorFormat;

use super::{Block, ReportDocument, png_size, svg_size};

/// A cell's number and the number format that shows it as the report did,
/// or `None` when the cell is not a plain number.
#[must_use]
pub fn cell_number(cell: &str) -> Option<(f64, String)> {
    let t = cell.trim().replace('−', "-");
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || ".,+-eE%".contains(c)) {
        return None;
    }
    let (body, percent) = match t.strip_suffix('%') {
        Some(b) => (b.to_owned(), true),
        None => (t.clone(), false),
    };
    let grouped = body.contains(',');
    if grouped {
        // Only well-formed thousands grouping: 1,234 or 12,345.67.
        let int = body.split(['.', 'e', 'E']).next().unwrap_or("");
        let digits = int.trim_start_matches(['-', '+']);
        let groups: Vec<&str> = digits.split(',').collect();
        let ok = !groups[0].is_empty()
            && groups[0].len() <= 3
            && groups[1..].iter().all(|g| g.len() == 3)
            && !body[int.len()..].contains(',');
        if !ok {
            return None;
        }
    }
    let plain = body.replace(',', "");
    let value: f64 = plain.parse().ok().filter(|v: &f64| v.is_finite())?;
    let (mantissa, scientific) = match plain.find(['e', 'E']) {
        Some(i) => (&plain[..i], true),
        None => (plain.as_str(), false),
    };
    let decimals = mantissa.split('.').nth(1).map_or(0, str::len);
    let digits = if decimals > 0 { format!(".{}", "0".repeat(decimals)) } else { String::new() };
    let int = if grouped { "#,##0" } else { "0" };
    let format = if scientific {
        format!("0{digits}E+00")
    } else if percent {
        format!("{int}{digits}%")
    } else {
        format!("{int}{digits}")
    };
    Some((if percent { value / 100.0 } else { value }, format))
}

/// A sheet name from `title`: at most 31 characters, none of `[]:*?/\`,
/// not starting or ending with an apostrophe, and unlike the names in
/// `taken` (ignoring case), numbered "(2)", "(3)", … when it would repeat.
fn sheet_name(title: &str, taken: &[String]) -> String {
    let clean: String = title.chars().filter(|c| !"[]:*?/\\".contains(*c)).collect();
    let clean = clean.trim().trim_matches('\'').trim().to_owned();
    let base = if clean.is_empty() { "Report".to_owned() } else { clean };
    let clash = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    let fit = |s: &str, room: usize| s.chars().take(room).collect::<String>().trim_end().to_owned();
    let first = fit(&base, 31);
    if !clash(&first) {
        return first;
    }
    (2..)
        .map(|k| {
            let suffix = format!(" ({k})");
            format!("{}{suffix}", fit(&base, 31 - suffix.chars().count()))
        })
        .find(|n| !clash(n))
        .expect("an unused name")
}

struct Formats {
    title: Format,
    meta: Format,
    heading: Format,
    header: Format,
    header_right: Format,
    note: Format,
    formula: Format,
}

impl Formats {
    fn new() -> Self {
        let muted = Color::RGB(0x5A_6270);
        Self {
            title: Format::new().set_bold().set_font_size(16),
            meta: Format::new().set_font_color(muted),
            heading: Format::new().set_bold().set_font_size(12),
            header: Format::new().set_bold().set_border_bottom(FormatBorder::Thin),
            header_right: Format::new().set_bold().set_border_bottom(FormatBorder::Thin).set_align(FormatAlign::Right),
            note: Format::new().set_font_color(muted),
            formula: Format::new().set_font_name("Consolas"),
        }
    }
}

/// Write `blocks` down `sheet` from row 0.
fn fill_sheet(
    sheet: &mut Worksheet,
    title: &str,
    meta: &str,
    blocks: &[Block<'_>],
    f: &Formats,
) -> Result<(), XlsxError> {
    let col = |j: usize| u16::try_from(j).unwrap_or(u16::MAX);
    let mut row: u32 = 0;
    let mut widths: Vec<usize> = Vec::new();
    let mut widen = |j: usize, text: &str| {
        if widths.len() <= j {
            widths.resize(j + 1, 0);
        }
        widths[j] = widths[j].max(text.chars().count());
    };
    sheet.write_string_with_format(row, 0, title, &f.title)?;
    row += 1;
    sheet.write_string_with_format(row, 0, meta, &f.meta)?;
    row += 2;
    for b in blocks {
        match b {
            Block::Heading(h) => {
                if row > 3 {
                    row += 1;
                }
                let format = if h.level == 1 && blocks.iter().any(|b| matches!(b, Block::Heading(x) if x.level > 1)) {
                    &f.title
                } else {
                    &f.heading
                };
                sheet.write_string_with_format(row, 0, &h.text, format)?;
                row += 1;
            }
            Block::Note(n) => {
                for line in n.lines() {
                    sheet.write_string_with_format(row, 0, line, &f.note)?;
                    row += 1;
                }
            }
            Block::Formula(text) => {
                for line in text.lines() {
                    sheet.write_string_with_format(row, 0, line, &f.formula)?;
                    row += 1;
                }
            }
            Block::Table(t) => {
                for (j, h) in t.table.columns.iter().enumerate() {
                    let format = if t.numeric[j] { &f.header_right } else { &f.header };
                    sheet.write_string_with_format(row, col(j), h, format)?;
                    widen(j, h);
                }
                row += 1;
                for cells in &t.table.rows {
                    for (j, cell) in cells.iter().enumerate() {
                        widen(j, cell.trim());
                        if cell.trim().is_empty() {
                            continue;
                        }
                        match cell_number(cell) {
                            Some((value, format)) => {
                                sheet.write_number_with_format(
                                    row,
                                    col(j),
                                    value,
                                    &Format::new().set_num_format(format),
                                )?;
                            }
                            None if t.right_aligned(j, cell) => {
                                sheet.write_string_with_format(
                                    row,
                                    col(j),
                                    cell.trim(),
                                    &Format::new().set_align(FormatAlign::Right),
                                )?;
                            }
                            None => {
                                sheet.write_string(row, col(j), cell.as_str())?;
                            }
                        }
                    }
                    row += 1;
                }
            }
            Block::Figure(fig) => match fig.render(VectorFormat::Png) {
                Ok(png) => {
                    let (w_pt, h_pt) = fig
                        .render(VectorFormat::Svg)
                        .ok()
                        .and_then(|svg| svg_size(&svg))
                        .or_else(|| png_size(&png).map(|(w, h)| (f64::from(w) * 0.24, f64::from(h) * 0.24)))
                        .unwrap_or((540.0, 380.0));
                    let image = Image::new_from_buffer(&png)?;
                    // Shown at its document size: points at 96 pixels per inch.
                    let shown_px = w_pt * 96.0 / 72.0;
                    let natural_px = image.width() * 96.0 / image.width_dpi().max(1.0);
                    let scale = shown_px / natural_px.max(1.0);
                    let image = image.set_scale_width(scale).set_scale_height(scale).set_alt_text(fig.alt_text());
                    sheet.insert_image(row, 0, &image)?;
                    // Default rows are 20 pixels high.
                    row += (h_pt * 96.0 / 72.0 / 20.0).ceil() as u32 + 1;
                }
                Err(e) => {
                    sheet.write_string_with_format(row, 0, &e, &f.note)?;
                    row += 1;
                }
            },
        }
    }
    for (j, w) in widths.iter().enumerate() {
        sheet.set_column_width(col(j), (*w as f64 + 2.0).clamp(8.0, 60.0))?;
    }
    Ok(())
}

/// `doc` as an `.xlsx` file: one sheet per report.
///
/// # Errors
/// When the workbook cannot be written.
pub fn write(doc: &ReportDocument<'_>) -> Result<Vec<u8>, String> {
    let fail = |e: XlsxError| format!("could not write the workbook: {e}");
    let mut workbook = Workbook::new();
    let formats = Formats::new();
    let mut names: Vec<String> = Vec::new();
    if doc.reports.is_empty() {
        let sheet = workbook.add_worksheet();
        let name = sheet_name(&doc.title, &names);
        sheet.set_name(&name).map_err(fail)?;
        fill_sheet(sheet, &doc.title, &doc.meta, &doc.blocks, &formats).map_err(fail)?;
    }
    let single = doc.reports.len() == 1;
    for (title, range) in &doc.reports {
        let name = sheet_name(title, &names);
        let sheet = workbook.add_worksheet();
        sheet.set_name(&name).map_err(fail)?;
        names.push(name);
        // A report among several starts with its own heading: use it as the
        // sheet title rather than repeating it.
        let mut blocks = &doc.blocks[range.clone()];
        if !single {
            if let Some(Block::Heading(_)) = blocks.first() {
                blocks = &blocks[1..];
            }
        }
        fill_sheet(sheet, title, &doc.meta, blocks, &formats).map_err(fail)?;
    }
    workbook.save_to_buffer().map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html_export::HtmlReport;
    use crate::report_export::ExportScope;
    use crate::report_export::tests::{plant_growth, source};

    #[test]
    fn numbers_keep_their_precision() {
        let n = |s: &str| cell_number(s);
        assert_eq!(n("0.0312"), Some((0.0312, "0.0000".into())));
        assert_eq!(n("−2.5"), Some((-2.5, "0.0".into())));
        assert_eq!(n("17"), Some((17.0, "0".into())));
        assert_eq!(n("4.30e-6"), Some((4.3e-6, "0.00E+00".into())));
        assert_eq!(n("1,234.5"), Some((1234.5, "#,##0.0".into())));
        assert_eq!(n("12.5%"), Some((0.125, "0.0%".into())));
        for text in ["< 0.0001", "7.25 ▲", "—", "Intercept", "1,23", "12,3456", "1.2.3", "e", "", "1,234,56"] {
            assert_eq!(n(text), None, "{text}");
        }
    }

    #[test]
    fn sheet_names_are_valid_and_unique() {
        let mut taken = Vec::new();
        for title in [
            "Fit: y by x [model]",
            "Fit y by x model",
            "FIT Y BY X MODEL",
            "''",
            "A very long report title that goes on and on",
        ] {
            let name = sheet_name(title, &taken);
            assert!(name.chars().count() <= 31 && !name.chars().any(|c| "[]:*?/\\".contains(c)), "{name}");
            assert!(!taken.iter().any(|t: &String| t.eq_ignore_ascii_case(&name)), "{name}");
            taken.push(name);
        }
        assert_eq!(taken[0], "Fit y by x model");
        assert_eq!(taken[1], "Fit y by x model (2)");
        assert_eq!(taken[3], "Report");
    }

    /// The workbook opens with calamine, has a sheet per report and holds
    /// the table cells (numbers as numbers).
    #[test]
    fn workbooks_have_a_sheet_per_report() {
        use calamine::{Data, Reader, Xlsx};
        let (table, rows, computed) = plant_growth();
        let all: Vec<HtmlReport<'_>> = computed.iter().map(HtmlReport::of).collect();
        let doc = ReportDocument::new(ExportScope::All(&all), source(&table, &rows));
        let bytes = write(&doc).unwrap();
        let mut book: Xlsx<_> = Xlsx::new(std::io::Cursor::new(bytes)).unwrap();
        let names = book.sheet_names();
        assert_eq!(names.len(), computed.len());
        let range = book.worksheet_range(&names[0]).unwrap();
        let cells: Vec<&Data> = range.used_cells().map(|(_, _, c)| c).collect();
        assert!(cells.iter().any(|c| matches!(c, Data::String(s) if s == &computed[0].title)));
        assert!(cells.iter().any(|c| matches!(c, Data::Float(_))), "numbers are numbers");
    }
}
