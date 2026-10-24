//! Reports exported as documents: PDF, Word (`.docx`), PowerPoint
//! (`.pptx`), HTML, Markdown and Excel workbooks.
//!
//! Every format is written from the same [`ReportDocument`], built from the
//! reports as an export sees them ([`HtmlReport`]: the open sections, in
//! report order) and the data they were run on ([`HtmlSource`]):
//!
//! * a title (the report's, or `"<table> reports"` for every report of a
//!   table), with a line naming the table, its row counts and the export
//!   date;
//! * for each report (when there are several) a heading, then each open
//!   section as a heading with its body: tables (numeric columns
//!   right-aligned), notes, formulas, and plots drawn by
//!   [`crate::report_figure`] (graph canvases by [`crate::graph_figure`]),
//!   the same figures the right-click "Save plot as…" items write.
//!
//! The formats:
//!
//! | Format | Plots | Writer |
//! |---|---|---|
//! | PDF | vector, from the figure's SVG | [`pdf`]: paged on Letter or A4, bundled fonts embedded (subset), bookmarks per heading, page numbers |
//! | Word | 300 ppi PNG with the SVG alongside (Word 2016 and later show the SVG) | [`docx`]: Office Open XML (ECMA-376) with heading styles, header rows that repeat on each page, page numbers |
//! | PowerPoint | 300 ppi PNG with the SVG alongside | [`pptx`]: 16:9 slides, one or more per section, long tables continued over slides |
//! | HTML | inline SVG | [`crate::html_export`] |
//! | Markdown | PNG files in a `<name>_files` folder beside the `.md` file | [`markdown`]: GitHub-flavoured tables |
//! | Excel | PNG pictures | [`xlsx`]: one sheet per report; numbers stored as numbers in the precision shown |

pub mod docx;
pub mod markdown;
pub mod pdf;
pub mod pptx;
pub mod xlsx;

use std::path::{Path, PathBuf};

use stat_viz::export::VectorFormat;

use crate::SavedReport;
use crate::html_export::{self, HtmlReport, HtmlSource, is_numeric_cell};
use crate::report::{ReportSection, SectionContent, TextTable};
use crate::report_figure::section_figure;

/// A document format reports can be exported to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReportFormat {
    /// Portable Document Format, paged for printing.
    #[default]
    Pdf,
    /// Word document (Office Open XML, `.docx`).
    Word,
    /// PowerPoint presentation (Office Open XML, `.pptx`).
    PowerPoint,
    /// A self-contained web page.
    Html,
    /// Markdown text, with plots as PNG files in a folder beside it.
    Markdown,
    /// Excel workbook (`.xlsx`): one sheet per report.
    Excel,
}

impl ReportFormat {
    /// Every format, in the order the export dialog lists them.
    pub const ALL: [ReportFormat; 6] =
        [Self::Pdf, Self::Word, Self::PowerPoint, Self::Html, Self::Markdown, Self::Excel];

    /// File extension (without the dot).
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Word => "docx",
            Self::PowerPoint => "pptx",
            Self::Html => "html",
            Self::Markdown => "md",
            Self::Excel => "xlsx",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Word => "Word document",
            Self::PowerPoint => "PowerPoint slides",
            Self::Html => "Web page (HTML)",
            Self::Markdown => "Markdown",
            Self::Excel => "Excel workbook",
        }
    }

    /// What the format is good for, one line.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Pdf => "Pages ready to print or send; plots stay sharp at any zoom",
            Self::Word => "An editable document for Word, Pages, LibreOffice or Google Docs",
            Self::PowerPoint => "Slides to present: one or more per section, for PowerPoint, Keynote or Google Slides",
            Self::Html => "One web page that opens in any browser",
            Self::Markdown => "Plain text for notebooks, wikis and GitHub; plots saved as pictures beside it",
            Self::Excel => "Every table on a sheet per report, numbers ready to reuse; plots as pictures",
        }
    }

    /// The format whose extension is `ext` (any case), if any.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.to_ascii_lowercase();
        match ext.as_str() {
            "htm" => Some(Self::Html),
            "markdown" => Some(Self::Markdown),
            _ => Self::ALL.into_iter().find(|f| f.extension() == ext),
        }
    }

    /// Whether the format is paged, so a paper size applies.
    #[must_use]
    pub fn is_paged(self) -> bool {
        matches!(self, Self::Pdf | Self::Word)
    }

    /// The name used in saved settings and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Word => "word",
            Self::PowerPoint => "powerpoint",
            Self::Html => "html",
            Self::Markdown => "markdown",
            Self::Excel => "excel",
        }
    }

    /// The format named `key` (as [`Self::key`] writes it, or its extension).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == key).or_else(|| Self::from_extension(key))
    }
}

/// Paper size of paged formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PaperSize {
    /// US Letter, 8.5 × 11 in.
    #[default]
    Letter,
    /// ISO A4, 210 × 297 mm.
    A4,
}

impl PaperSize {
    /// Both sizes.
    pub const ALL: [PaperSize; 2] = [Self::Letter, Self::A4];

    /// Width and height in points (1/72 in).
    #[must_use]
    pub fn points(self) -> (f64, f64) {
        match self {
            Self::Letter => (612.0, 792.0),
            Self::A4 => (595.276, 841.89),
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Letter => "Letter",
            Self::A4 => "A4",
        }
    }

    /// The name used in saved settings and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Letter => "letter",
            Self::A4 => "a4",
        }
    }

    /// The size named `key` (any case).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key().eq_ignore_ascii_case(key))
    }
}

/// Settings of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportOptions {
    /// Paper size of PDF and Word documents.
    pub paper: PaperSize,
}

/// What to export: one report, or several under one title.
#[derive(Debug, Clone, Copy)]
pub enum ExportScope<'a> {
    /// One report, titled with its own title.
    One(HtmlReport<'a>),
    /// Every given report, titled `"<table> reports"`, each report under a
    /// heading of its own.
    All(&'a [HtmlReport<'a>]),
}

/// A document heading: the title, a report (when several are exported) or
/// a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// Depth: 1 for a report among several, 2 for a section of it; a single
    /// report's sections are at depth 1.
    pub level: u8,
    /// The heading text.
    pub text: String,
}

/// One piece of a [`ReportDocument`].
#[derive(Debug)]
pub enum Block<'a> {
    /// A report or section heading.
    Heading(Heading),
    /// Explanatory text (line breaks kept), or why a report could not be
    /// computed.
    Note(String),
    /// Formula text, set in the monospace font.
    Formula(String),
    /// A table.
    Table(DocTable),
    /// A plot.
    Figure(DocFigure<'a>),
}

/// A report table with its columns classified for alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct DocTable {
    /// The headings and cells.
    pub table: TextTable,
    /// For each column, whether every non-empty cell is a number (see
    /// [`is_numeric_cell`]); such columns are right-aligned, headings too.
    pub numeric: Vec<bool>,
}

impl DocTable {
    /// `table` with its numeric columns found.
    #[must_use]
    pub fn new(table: TextTable) -> Self {
        let numeric = (0..table.columns.len())
            .map(|j| {
                let mut cells = table.rows.iter().filter_map(|r| r.get(j)).filter(|c| !c.trim().is_empty()).peekable();
                cells.peek().is_some() && cells.all(|c| is_numeric_cell(c))
            })
            .collect();
        Self { table, numeric }
    }

    /// Whether the cell in column `j` is right-aligned.
    #[must_use]
    pub fn right_aligned(&self, j: usize, cell: &str) -> bool {
        self.numeric.get(j).copied().unwrap_or(false) || is_numeric_cell(cell)
    }
}

/// A plot of a report, drawn on demand in the format a writer needs.
#[derive(Debug, Clone, Copy)]
pub struct DocFigure<'a> {
    kind: FigureKind<'a>,
    source: HtmlSource<'a>,
}

#[derive(Debug, Clone, Copy)]
enum FigureKind<'a> {
    Section(&'a ReportSection),
    Graph(&'a crate::GraphSpec),
}

impl DocFigure<'_> {
    /// The figure as a file in `format`.
    ///
    /// # Errors
    /// Why the figure cannot be drawn (a graph canvas whose data is not
    /// available or no longer fits its columns).
    pub fn render(&self, format: VectorFormat) -> Result<Vec<u8>, String> {
        match self.kind {
            FigureKind::Section(s) => {
                section_figure(s, Some(self.source.rows), format).ok_or_else(|| "this section has no plot".to_owned())
            }
            FigureKind::Graph(spec) => {
                let table = self.source.data.ok_or_else(|| "the graph's data is not available".to_owned())?;
                crate::graph_figure(spec, table, self.source.rows, format).map_err(|e| e.to_string())
            }
        }
    }

    /// A short description for screen readers and for documents that cannot
    /// show the picture: the section title, or "Graph".
    #[must_use]
    pub fn alt_text(&self) -> String {
        match self.kind {
            FigureKind::Section(s) => s.title.clone(),
            FigureKind::Graph(spec) => crate::graph_title(spec),
        }
    }
}

/// Reports laid out as a document, ready for any writer.
#[derive(Debug)]
pub struct ReportDocument<'a> {
    /// The document title.
    pub title: String,
    /// The line under it: table, row counts and export date.
    pub meta: String,
    /// Headings and contents, in order.
    pub blocks: Vec<Block<'a>>,
    /// Reports in the document (one sheet each in a workbook): each report's
    /// title and the range of [`Self::blocks`] holding it.
    pub reports: Vec<(String, std::ops::Range<usize>)>,
}

impl<'a> ReportDocument<'a> {
    /// The document for `scope`.
    #[must_use]
    pub fn new(scope: ExportScope<'a>, source: HtmlSource<'a>) -> Self {
        let meta = format!(
            "Table {} · {} · Exported {}",
            source.table_name,
            html_export::row_counts(source.rows),
            source.date
        );
        let mut doc = match scope {
            ExportScope::One(r) => Self { title: r.title.to_owned(), meta, blocks: Vec::new(), reports: Vec::new() },
            ExportScope::All(_) => {
                Self { title: format!("{} reports", source.table_name), meta, blocks: Vec::new(), reports: Vec::new() }
            }
        };
        match scope {
            ExportScope::One(r) => doc.push_report(r, source, 1),
            ExportScope::All(reports) => {
                if reports.is_empty() {
                    doc.blocks.push(Block::Note("This table has no reports.".into()));
                }
                for r in reports {
                    let start = doc.blocks.len();
                    doc.blocks.push(Block::Heading(Heading { level: 1, text: r.title.to_owned() }));
                    doc.push_report(*r, source, 2);
                    doc.reports.last_mut().expect("pushed").1.start = start;
                }
            }
        }
        doc
    }

    fn push_report(&mut self, r: HtmlReport<'a>, source: HtmlSource<'a>, level: u8) {
        let start = self.blocks.len();
        if let SavedReport::Graph { spec } = r.saved {
            self.blocks.push(Block::Figure(DocFigure { kind: FigureKind::Graph(spec), source }));
        }
        match r.sections {
            Err(message) => self.blocks.push(Block::Note(message.to_owned())),
            Ok(sections) => {
                for (i, s) in sections.iter().enumerate() {
                    if r.closed.contains(&i) {
                        continue;
                    }
                    self.blocks.push(Block::Heading(Heading { level, text: s.title.clone() }));
                    match &s.content {
                        SectionContent::Table(t) => self.blocks.push(Block::Table(DocTable::new(t.clone()))),
                        SectionContent::Note(n) => self.blocks.push(Block::Note(n.clone())),
                        SectionContent::Formula(f) => self.blocks.push(Block::Formula(f.clone())),
                        other => {
                            self.blocks.push(Block::Figure(DocFigure { kind: FigureKind::Section(s), source }));
                            if let SectionContent::EffectPareto(p) = other {
                                self.blocks.push(Block::Table(DocTable::new(p.table())));
                            }
                        }
                    }
                }
            }
        }
        self.reports.push((r.title.to_owned(), start..self.blocks.len()));
    }
}

/// One file of an export: its path relative to the chosen file's folder
/// (the chosen file itself first) and its contents.
pub type ExportFile = (PathBuf, Vec<u8>);

/// The files of `scope` exported as `format`: one file for every format but
/// Markdown, which adds its plots as PNG files in a `<name>_files` folder.
/// `file_name` is the name of the chosen file.
///
/// # Errors
/// The message to show when a writer fails.
pub fn render(
    scope: ExportScope<'_>,
    source: HtmlSource<'_>,
    format: ReportFormat,
    options: ExportOptions,
    file_name: &str,
) -> Result<Vec<ExportFile>, String> {
    let main = PathBuf::from(file_name);
    if format == ReportFormat::Html {
        let html = match scope {
            ExportScope::One(r) => html_export::report_html(r, source),
            ExportScope::All(reports) => html_export::reports_html(reports, source),
        };
        return Ok(vec![(main, html.into_bytes())]);
    }
    let doc = ReportDocument::new(scope, source);
    Ok(match format {
        ReportFormat::Pdf => vec![(main, pdf::write(&doc, options)?)],
        ReportFormat::Word => vec![(main, docx::write(&doc, options)?)],
        ReportFormat::PowerPoint => vec![(main, pptx::write(&doc)?)],
        ReportFormat::Excel => vec![(main, xlsx::write(&doc)?)],
        ReportFormat::Markdown => {
            let stem = Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("report");
            let (text, figures) = markdown::write(&doc, &format!("{stem}_files"));
            let mut files = vec![(main, text.into_bytes())];
            files.extend(figures);
            files
        }
        ReportFormat::Html => unreachable!("written above"),
    })
}

/// `path` with `format`'s extension, unless it already has it (or, for
/// HTML and Markdown, a common alternative such as `.htm`).
#[must_use]
pub fn with_extension(path: &Path, format: ReportFormat) -> PathBuf {
    let has = path.extension().and_then(|e| e.to_str()).and_then(ReportFormat::from_extension) == Some(format);
    if has {
        path.to_owned()
    } else {
        let mut name = path.as_os_str().to_owned();
        name.push(".");
        name.push(format.extension());
        PathBuf::from(name)
    }
}

/// Export `scope` as `format` to `path` (its extension added when missing),
/// writing any extra files beside it. Returns the main file's path.
///
/// # Errors
/// The message to show when the export cannot be written.
pub fn export(
    scope: ExportScope<'_>,
    source: HtmlSource<'_>,
    format: ReportFormat,
    options: ExportOptions,
    path: &Path,
) -> Result<PathBuf, String> {
    let path = with_extension(path, format);
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("report").to_owned();
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let files = render(scope, source, format, options, &file_name)?;
    for (i, (rel, bytes)) in files.iter().enumerate() {
        let target = if i == 0 { path.clone() } else { dir.join(rel) };
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, bytes).map_err(|e| format!("could not write {}: {e}", target.display()))?;
    }
    Ok(path)
}

/// Width and height (in points) of an SVG figure written by
/// [`stat_viz::export`], read from its `width`/`height` attributes
/// (`540pt`) or, failing that, its view box.
#[must_use]
pub fn svg_size(svg: &[u8]) -> Option<(f64, f64)> {
    let text = std::str::from_utf8(svg).ok()?;
    let start = text.find("<svg")?;
    let tag = &text[start..start + text[start..].find('>')?];
    let attr = |name: &str| -> Option<f64> {
        let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
        let value = &tag[at..at + tag[at..].find('"')?];
        let number = value.trim_end_matches("pt").trim_end_matches("px");
        let v: f64 = number.parse().ok()?;
        Some(if value.ends_with("px") { v * 0.75 } else { v })
    };
    match (attr("width"), attr("height")) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
        _ => {
            let at = tag.find(" viewBox=\"")? + 10;
            let vb: Vec<f64> =
                tag[at..at + tag[at..].find('"')?].split_whitespace().filter_map(|v| v.parse().ok()).collect();
            (vb.len() == 4 && vb[2] > 0.0 && vb[3] > 0.0).then(|| (vb[2], vb[3]))
        }
    }
}

/// Width and height in pixels of a PNG image, from its header.
#[must_use]
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || &png[12..16] != b"IHDR" {
        return None;
    }
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    Some((be(&png[16..20]), be(&png[20..24])))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::report_run::ComputedReport;
    use stat_data::{DataTable, RowStateManager};

    pub(crate) fn source<'a>(table: &'a DataTable, rows: &'a RowStateManager) -> HtmlSource<'a> {
        HtmlSource { table_name: table.name(), rows, date: "2026-10-17", data: Some(table) }
    }

    /// The Plant growth sample with its reports computed.
    pub(crate) fn plant_growth() -> (DataTable, RowStateManager, Vec<ComputedReport>) {
        let info = crate::sample::catalog().into_iter().find(|s| s.name.starts_with("Plant growth")).unwrap();
        let sample = (info.build)();
        let rows = RowStateManager::new(sample.table.n_rows());
        let reports = sample.reports.iter().map(|r| crate::run_report(r, &sample.table, &rows).unwrap()).collect();
        (sample.table, rows, reports)
    }

    #[test]
    fn documents_hold_the_open_sections_in_order() {
        let (table, rows, computed) = plant_growth();
        let report = &computed[0];
        let one = ReportDocument::new(ExportScope::One(HtmlReport::of(report)), source(&table, &rows));
        assert_eq!(one.title, report.title);
        assert!(one.meta.starts_with(&format!("Table {} · ", table.name())) && one.meta.ends_with("2026-10-17"));
        let headings: Vec<&str> = one
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Heading(h) => Some(h.text.as_str()),
                _ => None,
            })
            .collect();
        let titles: Vec<&str> = report.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(headings, titles);
        assert_eq!(one.reports.len(), 1);
        assert_eq!(one.reports[0].1, 0..one.blocks.len());

        let closed = [0usize];
        let fewer = ReportDocument::new(
            ExportScope::One(HtmlReport { closed: &closed, ..HtmlReport::of(report) }),
            source(&table, &rows),
        );
        assert!(!fewer.blocks.iter().any(|b| matches!(b, Block::Heading(h) if h.text == titles[0])));

        let all: Vec<HtmlReport<'_>> = computed.iter().map(HtmlReport::of).collect();
        let doc = ReportDocument::new(ExportScope::All(&all), source(&table, &rows));
        assert_eq!(doc.title, format!("{} reports", table.name()));
        assert_eq!(doc.reports.len(), computed.len());
        for ((title, range), r) in doc.reports.iter().zip(&computed) {
            assert_eq!(title, &r.title);
            assert!(matches!(&doc.blocks[range.start], Block::Heading(h) if h.level == 1 && h.text == r.title));
        }
        let figures = doc.blocks.iter().filter(|b| matches!(b, Block::Figure(_))).count();
        assert!(figures > 0);
        for b in &doc.blocks {
            if let Block::Figure(f) = b {
                let svg = f.render(VectorFormat::Svg).unwrap();
                assert!(svg_size(&svg).is_some());
                let png = f.render(VectorFormat::Png).unwrap();
                assert!(png_size(&png).is_some_and(|(w, h)| w > 100 && h > 100));
            }
        }
    }

    #[test]
    fn formats_and_paths() {
        for f in ReportFormat::ALL {
            assert_eq!(ReportFormat::from_extension(f.extension()), Some(f));
            assert_eq!(ReportFormat::from_key(f.key()), Some(f));
            assert!(!f.label().is_empty() && !f.description().is_empty());
        }
        assert_eq!(ReportFormat::from_extension("HTM"), Some(ReportFormat::Html));
        assert_eq!(ReportFormat::from_extension("txt"), None);
        assert_eq!(with_extension(Path::new("/a/b"), ReportFormat::Pdf), PathBuf::from("/a/b.pdf"));
        assert_eq!(with_extension(Path::new("/a/b.PDF"), ReportFormat::Pdf), PathBuf::from("/a/b.PDF"));
        assert_eq!(with_extension(Path::new("b.v1"), ReportFormat::Word), PathBuf::from("b.v1.docx"));
        assert_eq!(with_extension(Path::new("b.htm"), ReportFormat::Html), PathBuf::from("b.htm"));
        assert_eq!(PaperSize::from_key("A4"), Some(PaperSize::A4));
        assert_eq!(svg_size(b"<svg xmlns=\"x\" width=\"540pt\" height=\"380pt\">"), Some((540.0, 380.0)));
        assert_eq!(svg_size(b"<svg viewBox=\"0 0 10 20\">"), Some((10.0, 20.0)));
        assert_eq!(svg_size(b"<svg width=\"100px\" height=\"40px\">"), Some((75.0, 30.0)));
    }

    #[test]
    fn exports_write_every_file() {
        let (table, rows, computed) = plant_growth();
        let dir = std::env::temp_dir().join(format!("anovacraft-report-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let all: Vec<HtmlReport<'_>> = computed.iter().map(HtmlReport::of).collect();
        for format in ReportFormat::ALL {
            let path = export(
                ExportScope::All(&all),
                source(&table, &rows),
                format,
                ExportOptions::default(),
                &dir.join("plants"),
            )
            .unwrap();
            assert_eq!(path, dir.join(format!("plants.{}", format.extension())));
            assert!(std::fs::metadata(&path).unwrap().len() > 1000, "{format:?}");
        }
        let figures = std::fs::read_dir(dir.join("plants_files")).unwrap().count();
        assert!(figures > 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod sample_files {
    /// Writes every format of a few samples' reports to
    /// `ANOVACRAFT_EXPORT_SAMPLES` for checking by eye.
    #[test]
    #[ignore = "writes sample exports to ANOVACRAFT_EXPORT_SAMPLES"]
    fn write_sample_exports() {
        use super::*;
        let Some(dir) = std::env::var_os("ANOVACRAFT_EXPORT_SAMPLES") else { return };
        let dir = PathBuf::from(dir);
        for info in crate::sample::catalog().into_iter().take(12) {
            let s = (info.build)();
            let rows = stat_data::RowStateManager::new(s.table.n_rows());
            let computed: Vec<_> =
                s.reports.iter().filter_map(|r| crate::run_report(r, &s.table, &rows).ok()).collect();
            let all: Vec<HtmlReport<'_>> = computed.iter().map(HtmlReport::of).collect();
            let source =
                HtmlSource { table_name: s.table.name(), rows: &rows, date: "2026-10-17", data: Some(&s.table) };
            let stem: String = info.name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            for f in ReportFormat::ALL {
                export(ExportScope::All(&all), source, f, ExportOptions::default(), &dir.join(&stem)).unwrap();
            }
        }
    }
}
