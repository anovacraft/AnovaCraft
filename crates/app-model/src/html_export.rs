//! Reports as self-contained HTML pages.
//!
//! [`report_html`] writes one report, [`reports_html`] all of a document's
//! reports and [`dashboard_html`] a dashboard's tiles on its grid, each as a
//! single HTML5 file that needs nothing else to display:
//!
//! * a heading with the report (or table) title, the table's name, its row
//!   counts and the export date;
//! * each section that is open in the report as a heading, in report order
//!   (sections the user collapsed are left out);
//! * tables as HTML tables, numeric cells right-aligned in the monospace font
//!   so their digits line up (the same cells the report shows);
//! * plots as inline SVG drawn by [`crate::report_figure`] (a graph canvas by
//!   [`crate::graph_figure`]), on a light canvas in either appearance;
//! * light and dark appearance through `prefers-color-scheme`, with the
//!   application's colour tokens;
//! * the bundled Inter and JetBrains Mono fonts embedded as base64
//!   `@font-face` data when the page stays under [`FONT_EMBED_LIMIT`] bytes
//!   with them, otherwise a system font stack.
//!
//! The page loads nothing: no scripts, no external stylesheets, fonts or
//! images, and no URLs (SVG namespace declarations are dropped, as HTML does
//! not need them). Every piece of text from the report or the data is
//! escaped.

use std::fmt::Write as _;

use stat_data::{DataTable, RowStateManager, TemporalDisplay, TemporalFormat};
use stat_viz::export::VectorFormat;

use crate::SavedReport;
use crate::dashboard::GridArea;
use crate::report::{ReportSection, SectionContent, TextTable};
use crate::report_figure::section_figure;
use crate::report_run::ComputedReport;

/// Largest page, in bytes, that still gets the fonts embedded; beyond it the
/// page uses system fonts so that large reports stay light.
pub const FONT_EMBED_LIMIT: usize = 1_500_000;

/// Inter (regular), © 2016–2023 The Inter Project Authors, SIL Open Font
/// License 1.1 (see `crates/app-gui/assets/fonts/Inter-LICENSE.txt`).
pub(crate) const INTER: &[u8] = include_bytes!("../../app-gui/assets/fonts/Inter-Regular.otf");
/// JetBrains Mono (regular), © 2020 The JetBrains Mono Project Authors, SIL
/// Open Font License 1.1 (see `JetBrainsMono-LICENSE.txt` alongside).
pub(crate) const JETBRAINS_MONO: &[u8] = include_bytes!("../../app-gui/assets/fonts/JetBrainsMono-Regular.ttf");

/// The data a report was run on, for the page heading and for figures.
#[derive(Debug, Clone, Copy)]
pub struct HtmlSource<'a> {
    /// The table's name.
    pub table_name: &'a str,
    /// Its row states (counts in the heading; excluded rows drawn hollow).
    pub rows: &'a RowStateManager,
    /// The table itself, needed to draw a graph canvas (whose figure is drawn
    /// from the data); `None` leaves graph canvases as a note.
    pub data: Option<&'a DataTable>,
    /// Export date as shown in the heading, e.g. `2026-10-17`
    /// ([`export_date`] gives today's).
    pub date: &'a str,
}

/// One report to export and which of its sections to leave out.
#[derive(Debug, Clone, Copy)]
pub struct HtmlReport<'a> {
    /// The report heading.
    pub title: &'a str,
    /// The request (a graph canvas is drawn from it and the data).
    pub saved: &'a SavedReport,
    /// The sections, or why the analysis could not be computed (exported as
    /// a note).
    pub sections: std::result::Result<&'a [ReportSection], &'a str>,
    /// Indices (into the sections) of the sections collapsed in the report,
    /// which are not exported.
    pub closed: &'a [usize],
}

impl<'a> HtmlReport<'a> {
    /// A computed report with every section exported.
    #[must_use]
    pub fn of(report: &'a ComputedReport) -> Self {
        Self { title: &report.title, saved: &report.saved, sections: Ok(&report.sections), closed: &[] }
    }
}

/// Today's date (UTC) as `yyyy-mm-dd`.
#[must_use]
pub fn export_date() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    TemporalDisplay::new(TemporalFormat::Date).text(now)
}

/// `report` as a self-contained HTML page (see the [module](self) docs).
#[must_use]
pub fn report_html(report: HtmlReport<'_>, source: HtmlSource<'_>) -> String {
    let mut body = heading(report.title, 1, source);
    body.push_str(&report_body(report, source, 2));
    page(report.title, &body)
}

/// Every report in `reports` on one HTML page, titled `"<table> reports"`:
/// each report as a second-level heading, its sections below it.
#[must_use]
pub fn reports_html(reports: &[HtmlReport<'_>], source: HtmlSource<'_>) -> String {
    let title = format!("{} reports", source.table_name);
    let mut body = heading(&title, 1, source);
    if reports.is_empty() {
        body.push_str("<p class=\"note\">This table has no reports.</p>\n");
    }
    for r in reports {
        let _ = write!(body, "<section class=\"report\">\n<h2>{}</h2>\n", escape(r.title));
        body.push_str(&report_body(*r, source, 3));
        body.push_str("</section>\n");
    }
    page(&title, &body)
}

/// A dashboard as an export sees it (see [`dashboard_html`]).
#[derive(Debug, Clone, Copy)]
pub struct HtmlDashboard<'a> {
    /// The dashboard's name (the page title).
    pub title: &'a str,
    /// The project it belongs to.
    pub project: &'a str,
    /// Its filters, described (e.g. `Region ∈ {East, West}`).
    pub filters: &'a [String],
    /// Its column pickers that show another column, described (e.g.
    /// `Humidity → Week`).
    pub switchers: &'a [String],
    /// Grid width in columns.
    pub grid_columns: u32,
    /// Export date as shown in the heading.
    pub date: &'a str,
}

/// What one exported dashboard tile shows.
#[derive(Debug, Clone, Copy)]
pub enum HtmlTileContent<'a> {
    /// A heading (`heading`) or a note.
    Text {
        /// The text (may span several lines).
        text: &'a str,
        /// Shown as a heading rather than body text.
        heading: bool,
    },
    /// A report run on its table's filtered rows.
    Report {
        /// The report and its open sections.
        report: HtmlReport<'a>,
        /// Its table and the filtered row states it ran on.
        source: HtmlSource<'a>,
        /// A line under the title, e.g. `Plants · 45 of 60 rows`.
        meta: &'a str,
    },
}

/// One exported dashboard tile: where it sits and what it shows.
#[derive(Debug, Clone, Copy)]
pub struct HtmlTile<'a> {
    /// Its cells on the dashboard's grid.
    pub area: GridArea,
    /// What it shows.
    pub content: HtmlTileContent<'a>,
}

/// A dashboard as one self-contained HTML page: a heading with the name,
/// project, filters, switched columns and export date, then the tiles laid out on the same
/// grid with CSS grid (each tile spanning the same columns and rows; rows
/// grow to fit their tiles). Report tiles hold their open sections (tables
/// as HTML tables, plots as inline SVG); text tiles their text. On narrow
/// screens the tiles stack in reading order (top to bottom, then left to
/// right). Like [`report_html`], the page loads nothing and escapes all text.
#[must_use]
pub fn dashboard_html(dashboard: HtmlDashboard<'_>, tiles: &[HtmlTile<'_>]) -> String {
    let columns = dashboard.grid_columns.max(1);
    let mut body = String::from("<header>\n");
    let _ = writeln!(body, "<h1>{}</h1>", escape(dashboard.title));
    let filters = if dashboard.filters.is_empty() {
        "no filters".to_owned()
    } else {
        format!("filtered to {}", dashboard.filters.join("; "))
    };
    let filters = if dashboard.switchers.is_empty() {
        filters
    } else {
        format!("{filters} · columns switched: {}", dashboard.switchers.join("; "))
    };
    let _ = writeln!(
        body,
        "<p class=\"meta\">Dashboard of <strong>{}</strong> · {} · Exported {}</p>\n</header>",
        escape(dashboard.project),
        escape(&filters),
        escape(dashboard.date)
    );
    if tiles.is_empty() {
        body.push_str("<p class=\"note\">This dashboard has no tiles.</p>\n");
    }
    let _ = writeln!(body, "<div class=\"dash\" style=\"grid-template-columns: repeat({columns}, minmax(0, 1fr));\">");
    let mut order: Vec<&HtmlTile<'_>> = tiles.iter().collect();
    order.sort_by_key(|t| (t.area.row, t.area.column));
    for t in order {
        let a = t.area;
        let place = format!(
            "grid-column: {} / span {}; grid-row: {} / span {};",
            u64::from(a.column) + 1,
            a.width.max(1),
            u64::from(a.row) + 1,
            a.height.max(1)
        );
        match t.content {
            HtmlTileContent::Text { text, heading } => {
                let class = if heading { "tile text heading" } else { "tile text" };
                let lines: Vec<String> = text.lines().map(escape).collect();
                let inner = if heading {
                    format!("<h2>{}</h2>", lines.join("<br>"))
                } else {
                    format!("<p>{}</p>", lines.join("<br>\n"))
                };
                let _ = writeln!(body, "<section class=\"{class}\" style=\"{place}\">{inner}</section>");
            }
            HtmlTileContent::Report { report, source, meta } => {
                let _ = writeln!(
                    body,
                    "<section class=\"tile\" style=\"{place}\">\n<h2>{}</h2>\n<p class=\"meta\">{}</p>",
                    escape(report.title),
                    escape(meta)
                );
                body.push_str(&report_body(report, source, 3));
                body.push_str("</section>\n");
            }
        }
    }
    body.push_str("</div>\n");
    page_with(dashboard.title, &body, DASHBOARD_STYLE)
}

/// Extra layout for dashboard pages: a wider page and the tile grid.
const DASHBOARD_STYLE: &str = "\
body { max-width: 96rem; }
.dash { display: grid; gap: .75rem; grid-auto-rows: minmax(3.5rem, auto); align-items: stretch; }
.tile { min-width: 0; overflow: auto; border: 1px solid var(--border); border-radius: 8px; background: var(--bg);
  padding: .5rem .9rem .75rem; }
.tile > h2 { font-size: 1.05rem; margin: .25rem 0 0; }
.tile > .meta { font-size: .8rem; margin: 0 0 .25rem; }
.tile section h3 { font-size: .95rem; margin: 1rem 0 .4rem; }
.tile.text { border: none; background: none; padding: .25rem .25rem; }
.tile.text.heading h2 { font-size: 1.4rem; margin: .25rem 0; }
.tile.text p { margin: .25rem 0; color: var(--ink); }
@media (max-width: 48rem) { .dash { display: block; } .tile { margin-bottom: .75rem; } }
";

/// `<header>` with the title and the table's name, row counts and date.
fn heading(title: &str, level: u8, source: HtmlSource<'_>) -> String {
    format!(
        "<header>\n<h{level}>{}</h{level}>\n<p class=\"meta\">Table <strong>{}</strong> · {} · Exported {}</p>\n</header>\n",
        escape(title),
        escape(source.table_name),
        escape(&row_counts(source.rows)),
        escape(source.date),
    )
}

/// The table's row counts for an export heading, e.g. `30 rows, 2 excluded`.
pub(crate) fn row_counts(rows: &RowStateManager) -> String {
    let c = rows.counts();
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut counts = vec![plural(c.total, "row", "rows")];
    if c.excluded > 0 {
        counts.push(format!("{} excluded", c.excluded));
    }
    if c.hidden > 0 {
        counts.push(format!("{} hidden", c.hidden));
    }
    counts.join(", ")
}

/// The open sections of `r`, each under an `<h{level}>`.
fn report_body(r: HtmlReport<'_>, source: HtmlSource<'_>, level: u8) -> String {
    let mut out = String::new();
    if let SavedReport::Graph { spec } = r.saved {
        let figure = source.data.ok_or_else(|| "the graph's data is not available".to_owned()).and_then(|table| {
            crate::graph_figure(spec, table, source.rows, VectorFormat::Svg).map_err(|e| e.to_string())
        });
        match figure {
            Ok(svg) => {
                let _ = writeln!(out, "<figure class=\"plot\">{}</figure>", inline_svg(&svg));
            }
            Err(e) => {
                let _ = writeln!(out, "<p class=\"note\">{}</p>", escape(&e));
            }
        }
    }
    let sections = match r.sections {
        Ok(sections) => sections,
        Err(message) => {
            let _ = writeln!(out, "<p class=\"note\">{}</p>", escape(message));
            &[]
        }
    };
    for (i, s) in sections.iter().enumerate() {
        if r.closed.contains(&i) {
            continue;
        }
        let _ = write!(out, "<section>\n<h{level}>{}</h{level}>\n", escape(&s.title));
        out.push_str(&section_body(s, source));
        out.push_str("</section>\n");
    }
    out
}

fn section_body(s: &ReportSection, source: HtmlSource<'_>) -> String {
    match &s.content {
        SectionContent::Table(t) => html_table(t),
        SectionContent::Note(n) => {
            let lines: Vec<String> = n.lines().map(escape).collect();
            format!("<p class=\"note\">{}</p>\n", lines.join("<br>\n"))
        }
        SectionContent::Formula(f) => format!("<pre class=\"formula\"><code>{}</code></pre>\n", escape(f)),
        other => {
            let mut out = match section_figure(s, Some(source.rows), VectorFormat::Svg) {
                Some(svg) => format!("<figure class=\"plot\">{}</figure>\n", inline_svg(&svg)),
                None => String::new(),
            };
            // An effect Pareto's numbers follow its chart.
            if let SectionContent::EffectPareto(p) = other {
                out.push_str(&html_table(&p.table()));
            }
            out
        }
    }
}

/// `true` for a table cell holding a number: possibly signed (`-` or `−`),
/// in scientific notation, a percentage, grouped with commas, a p-value
/// bound such as `< 0.0001`, infinite, flagged with `▲`, or `—`.
#[must_use]
pub fn is_numeric_cell(cell: &str) -> bool {
    let t = cell.trim();
    let t = t.strip_suffix(['▲', '▼']).map_or(t, str::trim_end);
    matches!(t, "—" | "∞" | "−∞" | "-∞" | "inf" | "-inf" | "NaN")
        || (t.chars().any(|c| c.is_ascii_digit())
            && t.chars().all(|c| c.is_ascii_digit() || " .,+-−eE%<>=±".contains(c)))
}

/// A report table as `<table>`: numeric cells (and the headings of columns
/// whose cells are all numeric or empty) right-aligned in the monospace font.
fn html_table(t: &TextTable) -> String {
    let numeric_column: Vec<bool> = (0..t.columns.len())
        .map(|j| {
            let mut cells = t.rows.iter().filter_map(|r| r.get(j)).filter(|c| !c.trim().is_empty()).peekable();
            cells.peek().is_some() && cells.all(|c| is_numeric_cell(c))
        })
        .collect();
    let class = |num: bool| if num { " class=\"num\"" } else { "" };
    let mut out = String::from("<div class=\"table\"><table>\n<thead><tr>");
    for (j, h) in t.columns.iter().enumerate() {
        let _ = write!(out, "<th scope=\"col\"{}>{}</th>", class(numeric_column[j]), escape(h));
    }
    out.push_str("</tr></thead>\n<tbody>\n");
    for row in &t.rows {
        out.push_str("<tr>");
        for cell in row {
            let _ = write!(out, "<td{}>{}</td>", class(is_numeric_cell(cell)), escape(cell));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table></div>\n");
    out
}

/// An SVG file as an inline `<svg>` element: without the XML declaration
/// and namespace declarations (HTML parsers supply the SVG namespace).
fn inline_svg(svg: &[u8]) -> String {
    let text = String::from_utf8_lossy(svg);
    let start = text.find("<svg").unwrap_or(0);
    let mut out = text[start..].trim_end().to_owned();
    for attr in [" xmlns=\"http://www.w3.org/2000/svg\"", " xmlns:xlink=\"http://www.w3.org/1999/xlink\""] {
        out = out.replace(attr, "");
    }
    out
}

/// `text` with `&`, `<`, `>`, `"` and `'` escaped, for HTML text and
/// attribute values.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Standard base64 (RFC 4648 §4) with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The `@font-face` rules embedding the bundled fonts.
fn font_faces() -> String {
    format!(
        "/* Inter: copyright 2016-2023 The Inter Project Authors. JetBrains Mono: copyright 2020 The JetBrains \
         Mono Project Authors. Both are licensed under the SIL Open Font License, Version 1.1, which allows \
         embedding them in documents. */\n\
         @font-face {{ font-family: \"AC Inter\"; font-style: normal; font-weight: 100 900; \
         src: url(data:font/otf;base64,{}) format(\"opentype\"); }}\n\
         @font-face {{ font-family: \"AC JetBrains Mono\"; font-style: normal; font-weight: 400; \
         src: url(data:font/ttf;base64,{}) format(\"truetype\"); }}\n",
        base64(INTER),
        base64(JETBRAINS_MONO)
    )
}

/// Colour tokens and layout. Light values are the application's light plot
/// palette (canvas, ink, muted ink, frame, rules, accent); dark values its
/// dark palette.
const STYLE: &str = "\
:root {
  color-scheme: light dark;
  --bg: #ffffff; --surface: #f8fafc; --ink: #1e293b; --muted: #64748b; --border: #e2e8f0;
  --rule: rgba(15, 23, 42, 0.06); --accent: #0284c7; --canvas: #ffffff;
  --sans: \"AC Inter\", Inter, system-ui, -apple-system, \"Segoe UI\", Roboto, \"Helvetica Neue\", Arial, sans-serif;
  --mono: \"AC JetBrains Mono\", \"JetBrains Mono\", ui-monospace, \"SF Mono\", Menlo, Consolas, \"DejaVu Sans Mono\", monospace;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #16171b; --surface: #1d1f24; --ink: #e5e7eb; --muted: #9ca3af; --border: #2b2d33;
    --rule: rgba(255, 255, 255, 0.05); --accent: #38bdf8; --canvas: #ffffff;
  }
}
* { box-sizing: border-box; }
body { margin: 0 auto; max-width: 60rem; padding: 2rem 1rem 4rem; background: var(--bg); color: var(--ink);
  font-family: var(--sans); font-size: 15px; line-height: 1.5; }
header { border-bottom: 1px solid var(--border); margin-bottom: 1.5rem; }
h1 { font-size: 1.6rem; margin: 0 0 .25rem; font-weight: 650; }
h2 { font-size: 1.25rem; margin: 2rem 0 .75rem; font-weight: 650; }
h3 { font-size: 1.05rem; margin: 1.5rem 0 .5rem; font-weight: 600; }
.report > section h3 { color: var(--ink); }
.report { border-top: 1px solid var(--border); margin-top: 2rem; }
.meta { color: var(--muted); margin: 0 0 1rem; }
.meta strong { color: var(--ink); font-weight: 600; }
.note { color: var(--muted); }
.table { overflow-x: auto; margin: .5rem 0 1rem; }
table { border-collapse: collapse; font-size: .9rem; }
th, td { padding: .3rem .75rem; text-align: left; vertical-align: top; border-bottom: 1px solid var(--rule); }
th { font-weight: 600; border-bottom: 1px solid var(--border); white-space: nowrap; }
tbody tr:nth-child(even) { background: var(--surface); }
.num { text-align: right; font-family: var(--mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
th.num { font-family: var(--sans); }
pre.formula { font-family: var(--mono); font-size: .85rem; background: var(--surface); border: 1px solid var(--border);
  border-radius: 6px; padding: .75rem; overflow-x: auto; white-space: pre-wrap; }
figure.plot { margin: .5rem 0 1rem; display: inline-block; max-width: 100%; background: var(--canvas);
  border: 1px solid var(--border); border-radius: 6px; padding: .25rem; }
figure.plot svg { display: block; max-width: 100%; height: auto; font-family: var(--sans); }
@media print { body { max-width: none; } figure.plot, table { break-inside: avoid; } }
";

/// The whole page around `body`, with fonts embedded when it stays small.
fn page(title: &str, body: &str) -> String {
    page_with(title, body, "")
}

/// [`page`] with `extra` style rules after the shared ones.
fn page_with(title: &str, body: &str, extra: &str) -> String {
    let fonts_size = (INTER.len() + JETBRAINS_MONO.len()).div_ceil(3) * 4;
    let fonts = if body.len() + STYLE.len() + fonts_size < FONT_EMBED_LIMIT { font_faces() } else { String::new() };
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"generator\" content=\"AnovaCraft\">\n<title>{}</title>\n<style>\n{fonts}{STYLE}{extra}</style>\n</head>\n\
         <body>\n{body}</body>\n</html>\n",
        escape(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::SectionKind;

    fn source<'a>(table: &'a DataTable, rows: &'a RowStateManager) -> HtmlSource<'a> {
        HtmlSource { table_name: table.name(), rows, date: "2026-10-17", data: Some(table) }
    }

    /// The Plant growth sample with its reports computed.
    fn plant_growth() -> (DataTable, RowStateManager, Vec<ComputedReport>) {
        let info = crate::sample::catalog().into_iter().find(|s| s.name.starts_with("Plant growth")).unwrap();
        let sample = (info.build)();
        let rows = RowStateManager::new(sample.table.n_rows());
        let reports = sample.reports.iter().map(|r| crate::run_report(r, &sample.table, &rows).unwrap()).collect();
        (sample.table, rows, reports)
    }

    #[test]
    fn escapes_text() {
        assert_eq!(escape("<a href=\"x\">&'</a>"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;");
        let table = DataTable::new("<b>&", vec![stat_data::Column::numeric("x", vec![1.0])]).unwrap();
        let rows = RowStateManager::new(1);
        let mut t = TextTable::new(["<th>"]);
        t.push(["</td><script>"]);
        let report = ComputedReport {
            saved: SavedReport::Distribution {
                request: crate::DistributionRequest { column: 0, quantile_method: Default::default(), censoring: None },
                options: Default::default(),
            },
            title: "Title <i>".into(),
            sections: vec![
                ReportSection {
                    kind: SectionKind::SummaryStatistics,
                    title: "S & P".into(),
                    content: SectionContent::Table(t),
                },
                ReportSection {
                    kind: SectionKind::SummaryStatistics,
                    title: "n".into(),
                    content: SectionContent::Note("a < b\nc > d".into()),
                },
                ReportSection {
                    kind: SectionKind::SummaryStatistics,
                    title: "f".into(),
                    content: SectionContent::Formula("x < 1 && y".into()),
                },
            ],
        };
        let html = report_html(HtmlReport::of(&report), source(&table, &rows));
        for raw in ["<b>&", "<th>", "</td><script>", "Title <i>", "S & P", "a < b", "x < 1 &&"] {
            assert!(!html.contains(raw), "{raw} must be escaped");
        }
        for escaped in
            ["&lt;b&gt;&amp;", "&lt;/td&gt;&lt;script&gt;", "S &amp; P", "a &lt; b<br>", "x &lt; 1 &amp;&amp; y"]
        {
            assert!(html.contains(escaped), "{escaped}");
        }
        assert!(!html.contains("<script"));
    }

    #[test]
    fn exports_open_sections_with_tables_and_plots() {
        let (table, rows, reports) = plant_growth();
        let report = &reports[0];
        let plot = report.sections.iter().position(|s| crate::report_figure::is_plot(&s.content)).expect("a plot");
        let table_index =
            report.sections.iter().position(|s| matches!(s.content, SectionContent::Table(_))).expect("a table");
        let html = report_html(HtmlReport::of(report), source(&table, &rows));
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!("<h1>{}</h1>", escape(&report.title))));
        assert!(html.contains("Table <strong>"));
        assert!(html.contains(&format!("{} rows", table.n_rows())), "row counts");
        assert!(html.contains("2026-10-17"));
        assert!(!html.contains("http:") && !html.contains("https:"), "no URLs at all");
        assert!(html.contains("<svg") && html.contains("</svg>"));
        assert!(html.contains("prefers-color-scheme: dark"));
        assert!(html.contains("@font-face"), "fonts embedded");
        assert!(html.len() < FONT_EMBED_LIMIT + 10_000, "{}", html.len());
        // Every section heading, every table cell.
        for s in &report.sections {
            assert!(html.contains(&format!("<h2>{}</h2>", escape(&s.title))), "{}", s.title);
            if let SectionContent::Table(t) = &s.content {
                for cell in t.rows.iter().flatten().chain(&t.columns) {
                    let class = if is_numeric_cell(cell) { " class=\"num\"" } else { "" };
                    let td = format!("<td{class}>{}</td>", escape(cell));
                    let th = format!(">{}</th>", escape(cell));
                    assert!(html.contains(&td) || html.contains(&th), "{cell}");
                }
            }
        }
        // Closing sections leaves them out.
        let closed = [plot, table_index];
        let html2 = report_html(HtmlReport { closed: &closed, ..HtmlReport::of(report) }, source(&table, &rows));
        for i in closed {
            let heading = format!("<h2>{}</h2>", escape(&report.sections[i].title));
            assert!(!html2.contains(&heading), "{heading}");
        }
        assert_eq!(html2.matches("<svg").count() + 1, html.matches("<svg").count());
    }

    #[test]
    fn all_reports_on_one_page() {
        let (table, rows, computed) = plant_growth();
        let reports: Vec<HtmlReport<'_>> = computed.iter().map(HtmlReport::of).collect();
        let html = reports_html(&reports, source(&table, &rows));
        assert!(html.contains(&format!("<h1>{} reports</h1>", escape(table.name()))));
        for r in &computed {
            assert!(html.contains(&format!("<h2>{}</h2>", escape(&r.title))));
        }
        assert!(!html.contains("http:") && !html.contains("https:"));
        let failed = HtmlReport { sections: Err("singular <matrix>"), ..HtmlReport::of(&computed[0]) };
        let html = reports_html(&[failed], source(&table, &rows));
        assert!(html.contains("<p class=\"note\">singular &lt;matrix&gt;</p>"));
        let none = reports_html(&[], source(&table, &rows));
        assert!(none.contains("no reports"));
    }

    #[test]
    fn graph_canvases_are_drawn_from_the_data() {
        let t = crate::sample::longley();
        let rows = RowStateManager::new(t.n_rows());
        let mut spec = crate::GraphSpec::default();
        spec.encoding.x = vec![t.columns()[1].name.clone()];
        spec.encoding.y = vec![t.columns()[0].name.clone()];
        let saved = SavedReport::Graph { spec: Box::new(spec) };
        let report = crate::run_report(&saved, &t, &rows).unwrap();
        let html = report_html(HtmlReport::of(&report), source(&t, &rows));
        assert_eq!(html.matches("<svg").count(), 1);
        let without = HtmlSource { data: None, ..source(&t, &rows) };
        let html = report_html(HtmlReport::of(&report), without);
        assert!(!html.contains("<svg") && html.contains("not available"));
    }

    #[test]
    fn large_pages_use_system_fonts() {
        let table = DataTable::new("t", vec![stat_data::Column::numeric("x", vec![1.0])]).unwrap();
        let rows = RowStateManager::new(1);
        let mut t = TextTable::new(["value"]);
        for i in 0..40_000 {
            t.push([format!("{i}.123456")]);
        }
        let report = ComputedReport {
            saved: SavedReport::Distribution {
                request: crate::DistributionRequest { column: 0, quantile_method: Default::default(), censoring: None },
                options: Default::default(),
            },
            title: "Big".into(),
            sections: vec![ReportSection {
                kind: SectionKind::SummaryStatistics,
                title: "T".into(),
                content: SectionContent::Table(t),
            }],
        };
        let html = report_html(HtmlReport::of(&report), source(&table, &rows));
        assert!(!html.contains("@font-face") && html.contains("ui-monospace"));
    }

    #[test]
    fn numeric_cells_and_base64() {
        for n in ["500.61", "−2.5", "4.3e-6", "< 0.0001", "12%", "—", "1,234", "7.25 ▲", "∞"] {
            assert!(is_numeric_cell(n), "{n}");
        }
        for t in ["Normal (identity link)", "REML", "Fertilizer: B", "", "e", "2 levels"] {
            assert!(!is_numeric_cell(t), "{t}");
        }
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(export_date().len(), 10);
    }

    /// A dashboard page lays its tiles out on the same grid with CSS grid:
    /// each tile spans its columns and rows, report tiles hold their
    /// sections (plots as SVG), text tiles their text, and nothing loads.
    #[test]
    fn dashboard_pages_keep_the_grid() {
        use crate::dashboard::{TileContent, TileOutput, compute_dashboard};
        let file = crate::sample::greenhouse_project();
        let dashboard = &file.dashboards[0];
        let results = compute_dashboard(&file, dashboard);
        let reports: Vec<_> = results
            .tiles
            .iter()
            .map(|t| match &t.output {
                TileOutput::Report { report, .. } => Some(report.as_ref().expect("the sample's tiles compute")),
                TileOutput::Text { .. } => None,
            })
            .collect();
        let date = "2026-10-17";
        let mut tiles = Vec::new();
        for ((tile, computed), out) in dashboard.tiles.iter().zip(&reports).zip(&results.tiles) {
            let content = match (&tile.content, computed, &out.output) {
                (TileContent::Text { text, style }, _, _) => {
                    HtmlTileContent::Text { text, heading: *style == crate::dashboard::TextStyle::Heading }
                }
                (TileContent::Report { .. }, Some(r), TileOutput::Report { table, .. }) => {
                    let t = file.table(table).unwrap();
                    let rows = &results.table(table).unwrap().rows;
                    HtmlTileContent::Report {
                        report: HtmlReport::of(r),
                        source: HtmlSource { table_name: t.table.name(), rows, data: Some(&t.table), date },
                        meta: "45 of 60 rows <&>",
                    }
                }
                _ => unreachable!(),
            };
            tiles.push(HtmlTile { area: tile.area, content });
        }
        let filters: Vec<String> = dashboard.filters.iter().map(stat_viz::FilterCondition::describe).collect();
        let html = dashboard_html(
            HtmlDashboard {
                title: "Overview <1>",
                project: "Trial",
                filters: &filters,
                switchers: &["Humidity → Week".to_owned()],
                grid_columns: 12,
                date,
            },
            &tiles,
        );
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h1>Overview &lt;1&gt;</h1>"));
        assert!(html.contains("grid-template-columns: repeat(12, minmax(0, 1fr))"));
        assert!(html.contains("grid-column: 1 / span 12; grid-row: 1 / span 1;"), "the heading spans the grid");
        assert!(html.contains("grid-column: 7 / span 6; grid-row: 3 / span 6;"), "a report tile on the right");
        assert!(html.contains("Greenhouse ∈ {1, 2, 3}"), "the filters are named");
        assert!(html.contains("columns switched: Humidity → Week"), "and the switched columns");
        assert!(html.contains("45 of 60 rows &lt;&amp;&gt;"), "tile notes are escaped");
        assert!(html.contains("<h2>Greenhouse trial: plants and climate</h2>"));
        assert_eq!(html.matches("<section class=\"tile\"").count(), 4, "four report tiles");
        assert!(html.contains("<svg"), "plots as inline SVG");
        assert!(!html.contains("<script") && !html.contains("http://") && !html.contains("https://"));
        // Reading order: the heading comes before the first report.
        assert!(html.find("Greenhouse trial").unwrap() < html.find("class=\"tile\" style").unwrap());
    }
}
