//! Reports as PDF documents, paged on Letter or A4.
//!
//! The document is laid out first (pages of positioned lines, rules and
//! figures) and then written with `krilla`:
//!
//! * text in the bundled Inter, numbers in JetBrains Mono, both embedded as
//!   subsets, with DejaVu Sans for characters they lack (Greek letters,
//!   arrows, combining marks); lines are shaped with `rustybuzz`, so the
//!   widths used for wrapping and right-aligning are the widths drawn;
//!   bold is the regular face filled and outlined;
//! * headings kept with what follows them; tables in rules with their
//!   header row repeated on every page they run onto, numeric columns
//!   right-aligned in the monospace font; tables too wide for the page are
//!   set smaller (down to 6.5 pt), then their text columns wrap, then they
//!   are split into column groups that each repeat the first column;
//! * plots drawn from their SVG as vector graphics (`krilla-svg`), scaled
//!   to the text width;
//! * a footer with the title and "Page n of N", a bookmark for every report
//!   and section heading, and the title in the document information.

use std::sync::Arc;

use krilla::Document;
use krilla::color::rgb;
use krilla::destination::XyzDestination;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla_svg::{SurfaceExt, SvgSettings};
use stat_viz::export::VectorFormat;

use super::{Block, DocTable, ExportOptions, ReportDocument};
use crate::html_export::{INTER, JETBRAINS_MONO};

/// DejaVu Sans, © Bitstream / DejaVu fonts team, Bitstream Vera licence
/// (see `crates/app-gui/assets/fonts/DejaVu-LICENSE.txt`).
pub(crate) const DEJAVU: &[u8] = include_bytes!("../../../app-gui/assets/fonts/DejaVuSans.ttf");

/// Page margin, in points (0.75 in).
const MARGIN: f32 = 54.0;
/// Room kept below the text for the footer.
const FOOTER: f32 = 18.0;

/// Ink, secondary text, rules and fills.
const INK: [u8; 3] = [17, 24, 39];
const MUTED: [u8; 3] = [90, 98, 112];
const RULE: [u8; 3] = [55, 65, 81];
const HAIRLINE: [u8; 3] = [209, 213, 219];
const ZEBRA: [u8; 3] = [246, 247, 249];
const CODE_BG: [u8; 3] = [243, 244, 246];

/// Table text size, the smallest it is shrunk to, cell padding.
const TABLE_SIZE: f32 = 8.5;
const TABLE_MIN_SIZE: f32 = 6.5;
const CELL_PAD: f32 = 5.0;

const BODY_SIZE: f32 = 9.5;
const FORMULA_SIZE: f32 = 8.5;

/// One shaped piece of a line, set in one font.
#[derive(Debug, Clone)]
struct Run {
    font: usize,
    glyphs: Vec<KrillaGlyph>,
    text: String,
    /// Advance at a size of 1 pt.
    width: f32,
}

/// A shaped line of text.
#[derive(Debug, Clone, Default)]
struct Line {
    runs: Vec<Run>,
    /// Advance at a size of 1 pt.
    width: f32,
}

/// The three bundled fonts: Inter, JetBrains Mono, DejaVu Sans.
struct Fonts {
    faces: Vec<rustybuzz::Face<'static>>,
    pdf: Vec<Font>,
}

const TEXT: usize = 0;
const MONO: usize = 1;
const FALLBACK: usize = 2;

fn is_mark(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36F}' | '\u{1AB0}'..='\u{1AFF}' | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}')
}

impl Fonts {
    fn load() -> Result<Self, String> {
        let data: [&'static [u8]; 3] = [INTER, JETBRAINS_MONO, DEJAVU];
        let mut faces = Vec::new();
        let mut pdf = Vec::new();
        for bytes in data {
            faces.push(rustybuzz::Face::from_slice(bytes, 0).ok_or("a bundled font could not be read")?);
            pdf.push(Font::new(bytes.into(), 0).ok_or("a bundled font could not be read")?);
        }
        Ok(Self { faces, pdf })
    }

    /// `text` shaped on one line: in the monospace font when `mono`, each
    /// character (with its combining marks) in the first font that has it.
    fn line(&self, text: &str, mono: bool) -> Line {
        let order = if mono { [MONO, FALLBACK, TEXT] } else { [TEXT, FALLBACK, MONO] };
        // Clusters (a character and its marks) and the font of each.
        let mut pieces: Vec<(usize, String)> = Vec::new();
        let mut chars = text.chars().map(|c| if c == '\t' { ' ' } else { c }).peekable();
        while let Some(c) = chars.next() {
            let mut cluster = String::from(c);
            while let Some(&m) = chars.peek().filter(|m| is_mark(**m)) {
                cluster.push(m);
                chars.next();
            }
            let font = if c.is_whitespace() {
                order[0]
            } else {
                order
                    .into_iter()
                    .find(|&f| cluster.chars().all(|ch| self.faces[f].glyph_index(ch).is_some()))
                    .unwrap_or(order[0])
            };
            match pieces.last_mut() {
                Some((f, s)) if *f == font => s.push_str(&cluster),
                _ => pieces.push((font, cluster)),
            }
        }
        let mut line = Line::default();
        for (font, text) in pieces {
            let run = self.shape(font, text);
            line.width += run.width;
            line.runs.push(run);
        }
        line
    }

    fn shape(&self, font: usize, text: String) -> Run {
        let face = &self.faces[font];
        let upem = face.units_per_em() as f32;
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(&text);
        buffer.set_direction(rustybuzz::Direction::LeftToRight);
        buffer.guess_segment_properties();
        let out = rustybuzz::shape(face, &[], buffer);
        let infos = out.glyph_infos();
        let positions = out.glyph_positions();
        let mut glyphs = Vec::with_capacity(infos.len());
        let mut width = 0.0;
        for (i, (info, pos)) in infos.iter().zip(positions).enumerate() {
            let start = info.cluster as usize;
            let end = infos[i + 1..].iter().map(|g| g.cluster as usize).find(|&c| c > start).unwrap_or(text.len());
            let advance = pos.x_advance as f32 / upem;
            width += advance;
            glyphs.push(KrillaGlyph::new(
                GlyphId::new(info.glyph_id),
                advance,
                pos.x_offset as f32 / upem,
                -(pos.y_offset as f32) / upem,
                pos.y_advance as f32 / upem,
                start..end,
                None,
            ));
        }
        Run { font, glyphs, text, width }
    }

    /// `text` wrapped to lines no wider than `max` points at `size`: at
    /// spaces, and inside words too long for a line.
    fn wrap(&self, text: &str, mono: bool, size: f32, max: f32) -> Vec<Line> {
        let mut out = Vec::new();
        for paragraph in text.split('\n') {
            let mut current = String::new();
            for word in paragraph.split(' ') {
                let candidate = if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
                if self.line(&candidate, mono).width * size <= max || current.is_empty() && word.is_empty() {
                    current = candidate;
                    continue;
                }
                if !current.is_empty() {
                    out.push(self.line(&current, mono));
                }
                current = word.to_owned();
                // A word wider than the line breaks between characters.
                while self.line(&current, mono).width * size > max && current.chars().count() > 1 {
                    let mut head = String::new();
                    for c in current.chars() {
                        let mut next = head.clone();
                        next.push(c);
                        if !head.is_empty() && self.line(&next, mono).width * size > max {
                            break;
                        }
                        head = next;
                    }
                    out.push(self.line(&head, mono));
                    current = current[head.len()..].to_owned();
                }
            }
            out.push(self.line(&current, mono));
        }
        out
    }
}

/// A drawn figure: its index in [`Layout::figures`], width and height.
type FigureSlot = (usize, f32, f32);

/// Something drawn on a page.
#[derive(Debug, Clone)]
enum Op {
    /// A line of text with its baseline at `y`.
    Text { x: f32, y: f32, line: Line, size: f32, bold: bool, color: [u8; 3] },
    /// A horizontal rule.
    Rule { x0: f32, x1: f32, y: f32, width: f32, color: [u8; 3] },
    /// A filled rectangle.
    Fill { x: f32, y: f32, w: f32, h: f32, color: [u8; 3] },
    /// Figure `index` scaled into the box.
    Figure { index: usize, x: f32, y: f32, w: f32, h: f32 },
}

/// A bookmark: heading level and text, page and position.
#[derive(Debug, Clone)]
struct Bookmark {
    level: u8,
    text: String,
    page: usize,
    y: f32,
}

/// Column layout of (a column group of) a table.
#[derive(Debug, Clone)]
struct TablePlan {
    columns: Vec<usize>,
    widths: Vec<f32>,
    size: f32,
}

struct Layout<'f> {
    fonts: &'f Fonts,
    width: f32,
    height: f32,
    pages: Vec<Vec<Op>>,
    y: f32,
    bookmarks: Vec<Bookmark>,
    figures: Vec<usvg::Tree>,
}

impl Layout<'_> {
    fn text_width(&self) -> f32 {
        self.width - 2.0 * MARGIN
    }

    fn bottom(&self) -> f32 {
        self.height - MARGIN - FOOTER
    }

    fn at_top(&self) -> bool {
        self.y <= MARGIN + 0.01
    }

    fn new_page(&mut self) {
        self.pages.push(Vec::new());
        self.y = MARGIN;
    }

    /// Start a new page unless `h` points fit below the cursor (or the page
    /// is still empty).
    fn ensure(&mut self, h: f32) {
        if self.y + h > self.bottom() && !self.at_top() {
            self.new_page();
        }
    }

    fn push(&mut self, op: Op) {
        self.pages.last_mut().expect("a page").push(op);
    }

    fn text(&mut self, x: f32, line: Line, size: f32, bold: bool, color: [u8; 3]) {
        let y = self.y;
        self.push(Op::Text { x, y, line, size, bold, color });
    }

    /// Lines of text at the cursor, breaking pages between lines.
    fn paragraph(&mut self, lines: Vec<Line>, size: f32, color: [u8; 3], bold: bool, x: f32) {
        let lh = size * 1.4;
        for line in lines {
            self.ensure(lh);
            self.y += size;
            self.text(x, line, size, bold, color);
            self.y += lh - size;
        }
    }

    fn heading_size(level: u8, deepest: u8) -> f32 {
        if level < deepest { 14.0 } else { 11.5 }
    }

    fn heading(&mut self, level: u8, deepest: u8, text: &str, keep: f32) {
        let size = Self::heading_size(level, deepest);
        let lines = self.fonts.wrap(text, false, size, self.text_width());
        let h = lines.len() as f32 * size * 1.3 + size * 0.6;
        self.y += if level < deepest { 10.0 } else { 4.0 };
        self.ensure(h + keep);
        self.bookmarks.push(Bookmark { level, text: text.to_owned(), page: self.pages.len() - 1, y: self.y });
        for line in lines {
            self.y += size;
            self.text(MARGIN, line, size, true, INK);
            self.y += size * 0.3;
        }
        if level < deepest {
            let y = self.y + 2.0;
            self.push(Op::Rule { x0: MARGIN, x1: self.width - MARGIN, y, width: 0.5, color: HAIRLINE });
        }
        self.y += size * 0.6;
    }

    /// Natural widths of a table's columns at `size` (widest cell plus
    /// padding) and the narrowest each may wrap to.
    fn column_widths(&self, t: &DocTable, cells: &[Vec<Line>], size: f32) -> (Vec<f32>, Vec<f32>) {
        let n = t.table.columns.len();
        let mut natural = vec![0.0f32; n];
        let mut least = vec![0.0f32; n];
        for row in cells {
            for (j, line) in row.iter().enumerate().take(n) {
                natural[j] = natural[j].max(line.width * size + 2.0 * CELL_PAD);
            }
        }
        for j in 0..n {
            least[j] = if t.numeric[j] {
                natural[j]
            } else {
                let words = t.table.rows.iter().filter_map(|r| r.get(j)).chain(std::iter::once(&t.table.columns[j]));
                let widest = words
                    .flat_map(|c| c.split_whitespace())
                    .map(|w| self.fonts.line(w, false).width)
                    .fold(0.0f32, f32::max);
                (widest * size + 2.0 * CELL_PAD).min(natural[j]).min(self.text_width() * 0.45)
            };
        }
        (natural, least)
    }

    /// How to set `t`: at the largest size (from 8.5 pt down to 6.5 pt) at
    /// which it fits; else wrapping text columns; else in column groups.
    fn plan_table(&self, t: &DocTable, cells: &[Vec<Line>]) -> Vec<TablePlan> {
        let w = self.text_width();
        let n = t.table.columns.len();
        let (natural, _) = self.column_widths(t, cells, TABLE_SIZE);
        let total: f32 = natural.iter().sum();
        if total <= w {
            return vec![TablePlan { columns: (0..n).collect(), widths: natural, size: TABLE_SIZE }];
        }
        let size = (TABLE_SIZE * w / total).max(TABLE_MIN_SIZE);
        let (natural, least) = self.column_widths(t, cells, size);
        let fit = |cols: &[usize]| -> Option<Vec<f32>> {
            let nat: f32 = cols.iter().map(|&j| natural[j]).sum();
            if nat <= w {
                return Some(cols.iter().map(|&j| natural[j]).collect());
            }
            let min: f32 = cols.iter().map(|&j| least[j]).sum();
            (min <= w).then(|| {
                let spare = w - min;
                let give: f32 = cols.iter().map(|&j| natural[j] - least[j]).sum();
                cols.iter()
                    .map(|&j| least[j] + if give > 0.0 { spare * (natural[j] - least[j]) / give } else { 0.0 })
                    .collect()
            })
        };
        let all: Vec<usize> = (0..n).collect();
        if let Some(widths) = fit(&all) {
            return vec![TablePlan { columns: all, widths, size }];
        }
        // Column groups, each led by the first column.
        let mut plans = Vec::new();
        let mut next = 1;
        while next < n {
            let mut cols = vec![0, next];
            next += 1;
            while next < n {
                let mut more = cols.clone();
                more.push(next);
                if fit(&more).is_none() {
                    break;
                }
                cols = more;
                next += 1;
            }
            let widths = fit(&cols).unwrap_or_else(|| {
                // Even two columns overflow: share the width out.
                let min: f32 = cols.iter().map(|&j| least[j]).sum();
                cols.iter().map(|&j| least[j] * w / min).collect()
            });
            plans.push(TablePlan { columns: cols, widths, size });
        }
        if plans.is_empty() {
            plans.push(TablePlan { columns: vec![0], widths: vec![w], size });
        }
        plans
    }

    fn table(&mut self, t: &DocTable) {
        // Every cell shaped once at a size of 1 pt: the header row first.
        let header: Vec<Line> = t.table.columns.iter().map(|c| self.fonts.line(c, false)).collect();
        let mut cells = vec![header];
        for row in &t.table.rows {
            cells.push(row.iter().enumerate().map(|(j, c)| self.fonts.line(c, t.right_aligned(j, c))).collect());
        }
        let plans = self.plan_table(t, &cells);
        for (g, plan) in plans.iter().enumerate() {
            if g > 0 {
                self.y += 8.0;
            }
            self.table_group(t, &cells, plan);
        }
    }

    /// Lines of each cell of row `r` (0 = header) at `plan`'s widths.
    fn row_lines(&self, t: &DocTable, cells: &[Vec<Line>], plan: &TablePlan, r: usize) -> Vec<Vec<Line>> {
        plan.columns
            .iter()
            .zip(&plan.widths)
            .map(|(&j, &w)| {
                let line = &cells[r][j];
                if line.width * plan.size + 2.0 * CELL_PAD <= w + 0.01 {
                    vec![line.clone()]
                } else {
                    let text = if r == 0 { &t.table.columns[j] } else { &t.table.rows[r - 1][j] };
                    let mono = r > 0 && t.right_aligned(j, text);
                    self.fonts.wrap(text, mono, plan.size, w - 2.0 * CELL_PAD)
                }
            })
            .collect()
    }

    fn table_group(&mut self, t: &DocTable, cells: &[Vec<Line>], plan: &TablePlan) {
        let size = plan.size;
        let lh = size * 1.3;
        let vpad = size * 0.35;
        let row_height =
            |lines: &[Vec<Line>]| lines.iter().map(Vec::len).max().unwrap_or(1).max(1) as f32 * lh + 2.0 * vpad;
        let header = self.row_lines(t, cells, plan, 0);
        let header_h = row_height(&header);
        let table_w: f32 = plan.widths.iter().sum();
        let x0 = MARGIN;
        let first_row_h = if t.table.rows.is_empty() { 0.0 } else { row_height(&self.row_lines(t, cells, plan, 1)) };
        self.ensure(header_h + first_row_h + 2.0);
        let start_fragment = |l: &mut Self| {
            l.push(Op::Rule { x0, x1: x0 + table_w, y: l.y, width: 0.8, color: RULE });
            l.cells(t, plan, &header, true);
            l.y += header_h;
            l.push(Op::Rule { x0, x1: x0 + table_w, y: l.y, width: 0.4, color: RULE });
        };
        start_fragment(self);
        for r in 1..cells.len() {
            let lines = self.row_lines(t, cells, plan, r);
            let h = row_height(&lines);
            if self.y + h > self.bottom() {
                self.push(Op::Rule { x0, x1: x0 + table_w, y: self.y, width: 0.4, color: HAIRLINE });
                self.new_page();
                start_fragment(self);
            }
            if r % 2 == 0 {
                self.push(Op::Fill { x: x0, y: self.y, w: table_w, h, color: ZEBRA });
            }
            self.cells(t, plan, &lines, false);
            self.y += h;
        }
        self.push(Op::Rule { x0, x1: x0 + table_w, y: self.y, width: 0.8, color: RULE });
    }

    /// One row of cells at the cursor.
    fn cells(&mut self, t: &DocTable, plan: &TablePlan, lines: &[Vec<Line>], header: bool) {
        let size = plan.size;
        let lh = size * 1.3;
        let vpad = size * 0.35;
        let mut x = MARGIN;
        for ((&j, &w), cell) in plan.columns.iter().zip(&plan.widths).zip(lines) {
            for (k, line) in cell.iter().enumerate() {
                let right = t.numeric[j] || (!header && line.runs.first().is_some_and(|r| r.font == MONO));
                let lx = if right { x + w - CELL_PAD - line.width * size } else { x + CELL_PAD };
                let y = self.y + vpad + k as f32 * lh + size * 0.95;
                self.push(Op::Text { x: lx, y, line: line.clone(), size, bold: header, color: INK });
            }
            x += w;
        }
    }

    fn figure(&mut self, index: usize, w: f32, h: f32) {
        let tw = self.text_width();
        let mut fw = w.min(tw);
        let mut fh = h * fw / w;
        let room = self.bottom() - MARGIN;
        if fh > room {
            fw *= room / fh;
            fh = room;
        }
        self.ensure(fh);
        let x = MARGIN + (tw - fw) / 2.0;
        let y = self.y;
        self.push(Op::Figure { index, x, y, w: fw, h: fh });
        self.y += fh;
    }
}

/// The height a block needs at the top of a page to keep its heading with
/// it: a table's header and first row, two lines of text, or a figure.
fn keep_height(block: Option<&Block<'_>>, figure_height: Option<f32>) -> f32 {
    match block {
        Some(Block::Table(t)) => TABLE_SIZE * 1.9 * if t.table.rows.is_empty() { 1.0 } else { 2.0 },
        Some(Block::Note(_)) => BODY_SIZE * 2.8,
        Some(Block::Formula(_)) => FORMULA_SIZE * 2.8,
        Some(Block::Figure(_)) => figure_height.unwrap_or(120.0),
        Some(Block::Heading(_)) | None => 0.0,
    }
}

fn svg_options() -> usvg::Options<'static> {
    let mut db = usvg::fontdb::Database::new();
    for bytes in [INTER, DEJAVU, JETBRAINS_MONO] {
        db.load_font_data(bytes.to_vec());
    }
    let family = db.faces().next().and_then(|f| f.families.first()).map_or_else(|| "Inter".to_owned(), |f| f.0.clone());
    db.set_sans_serif_family(family.clone());
    let mut options = usvg::Options { font_family: family, ..usvg::Options::default() };
    options.fontdb = Arc::new(db);
    options
}

/// `doc` as a PDF file.
///
/// # Errors
/// When a bundled font cannot be read or the PDF cannot be written.
pub fn write(doc: &ReportDocument<'_>, options: ExportOptions) -> Result<Vec<u8>, String> {
    let fonts = Fonts::load()?;
    let (pw, ph) = options.paper.points();
    let svg = svg_options();
    let mut l = Layout {
        fonts: &fonts,
        width: pw as f32,
        height: ph as f32,
        pages: Vec::new(),
        y: MARGIN,
        bookmarks: Vec::new(),
        figures: Vec::new(),
    };
    l.new_page();
    // Title and the line under it.
    let title_lines = fonts.wrap(&doc.title, false, 18.0, l.text_width());
    l.paragraph(title_lines, 18.0, INK, true, MARGIN);
    let meta = fonts.wrap(&doc.meta, false, 9.0, l.text_width());
    l.paragraph(meta, 9.0, MUTED, false, MARGIN);
    l.y += 2.0;
    let x1 = l.width - MARGIN;
    let y = l.y;
    l.push(Op::Rule { x0: MARGIN, x1, y, width: 0.75, color: RULE });
    l.y += 10.0;

    // Figures drawn ahead, so their size is known for keeping headings with
    // them.
    // Per block: the figure's index and size, or why it could not be drawn.
    let mut figures: Vec<Option<Result<FigureSlot, String>>> = Vec::with_capacity(doc.blocks.len());
    for b in &doc.blocks {
        figures.push(match b {
            Block::Figure(f) => Some(f.render(VectorFormat::Svg).and_then(|bytes| {
                let (w, h) = super::svg_size(&bytes).unwrap_or((540.0, 380.0));
                let tree =
                    usvg::Tree::from_data(&bytes, &svg).map_err(|e| format!("the plot could not be drawn: {e}"))?;
                l.figures.push(tree);
                Ok((l.figures.len() - 1, w as f32, h as f32))
            })),
            _ => None,
        });
    }
    let deepest = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading(h) => Some(h.level),
            _ => None,
        })
        .max()
        .unwrap_or(1);
    let tw = l.text_width();
    for (i, b) in doc.blocks.iter().enumerate() {
        match b {
            Block::Heading(h) => {
                let next = doc.blocks.get(i + 1);
                let fig_h = match figures.get(i + 1) {
                    Some(Some(Ok((_, w, h)))) => Some(h * w.min(tw) / w),
                    _ => None,
                };
                l.heading(h.level, deepest, &h.text, keep_height(next, fig_h));
            }
            Block::Note(n) => {
                let lines = fonts.wrap(n, false, BODY_SIZE, tw);
                l.paragraph(lines, BODY_SIZE, MUTED, false, MARGIN);
                l.y += 6.0;
            }
            Block::Formula(f) => {
                let lines = fonts.wrap(f, true, FORMULA_SIZE, tw - 12.0);
                let lh = FORMULA_SIZE * 1.45;
                l.y += 2.0;
                for line in lines {
                    l.ensure(lh);
                    let (y, w) = (l.y, tw);
                    l.push(Op::Fill { x: MARGIN, y, w, h: lh, color: CODE_BG });
                    l.y += FORMULA_SIZE * 1.05;
                    l.text(MARGIN + 6.0, line, FORMULA_SIZE, false, INK);
                    l.y += lh - FORMULA_SIZE * 1.05;
                }
                l.y += 8.0;
            }
            Block::Table(t) => {
                if t.table.columns.is_empty() {
                    continue;
                }
                l.table(t);
                l.y += 12.0;
            }
            Block::Figure(_) => {
                match &figures[i] {
                    Some(Ok((index, w, h))) => l.figure(*index, *w, *h),
                    Some(Err(e)) => {
                        let lines = fonts.wrap(e, false, BODY_SIZE, tw);
                        l.paragraph(lines, BODY_SIZE, MUTED, false, MARGIN);
                    }
                    None => {}
                }
                l.y += 12.0;
            }
        }
    }
    render(doc, &fonts, &l)
}

fn color(c: [u8; 3]) -> rgb::Color {
    rgb::Color::new(c[0], c[1], c[2])
}

fn render(doc: &ReportDocument<'_>, fonts: &Fonts, l: &Layout<'_>) -> Result<Vec<u8>, String> {
    let mut document = Document::new();
    let n_pages = l.pages.len();
    let footer_y = l.height - MARGIN + 14.0;
    for (p, ops) in l.pages.iter().enumerate() {
        let settings = PageSettings::from_wh(l.width, l.height).ok_or("invalid page size")?;
        let mut page = document.start_page_with(settings);
        let mut s = page.surface();
        let mut footer = ops.clone();
        // Footer: title on the left (shortened to fit), page on the right.
        let number = fonts.line(&format!("Page {} of {n_pages}", p + 1), false);
        let room = l.width - 2.0 * MARGIN - number.width * 8.0 - 24.0;
        let mut title: String = doc.title.clone();
        while fonts.line(&title, false).width * 8.0 > room && title.chars().count() > 1 {
            title.pop();
            title = format!("{}…", title.trim_end_matches('…').trim_end());
            if title.chars().count() <= 2 {
                break;
            }
        }
        let x = l.width - MARGIN - number.width * 8.0;
        footer.push(Op::Text {
            x: MARGIN,
            y: footer_y,
            line: fonts.line(&title, false),
            size: 8.0,
            bold: false,
            color: MUTED,
        });
        footer.push(Op::Text { x, y: footer_y, line: number, size: 8.0, bold: false, color: MUTED });
        for op in &footer {
            match op {
                Op::Text { x, y, line, size, bold, color: c } => {
                    s.set_fill(Some(Fill {
                        paint: color(*c).into(),
                        opacity: NormalizedF32::ONE,
                        rule: Default::default(),
                    }));
                    s.set_stroke(bold.then(|| Stroke {
                        paint: color(*c).into(),
                        width: size * 0.045,
                        ..Stroke::default()
                    }));
                    let mut x = *x;
                    for run in &line.runs {
                        s.draw_glyphs(
                            Point::from_xy(x, *y),
                            &run.glyphs,
                            fonts.pdf[run.font].clone(),
                            &run.text,
                            *size,
                            false,
                        );
                        x += run.width * size;
                    }
                    s.set_stroke(None);
                }
                Op::Rule { x0, x1, y, width, color: c } => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(*x0, *y);
                    pb.line_to(*x1, *y);
                    if let Some(path) = pb.finish() {
                        s.set_fill(None);
                        s.set_stroke(Some(Stroke { paint: color(*c).into(), width: *width, ..Stroke::default() }));
                        s.draw_path(&path);
                        s.set_stroke(None);
                    }
                }
                Op::Fill { x, y, w, h, color: c } => {
                    let mut pb = PathBuilder::new();
                    if let Some(rect) = Rect::from_xywh(*x, *y, *w, *h) {
                        pb.push_rect(rect);
                    }
                    if let Some(path) = pb.finish() {
                        s.set_stroke(None);
                        s.set_fill(Some(Fill {
                            paint: color(*c).into(),
                            opacity: NormalizedF32::ONE,
                            rule: Default::default(),
                        }));
                        s.draw_path(&path);
                    }
                }
                Op::Figure { index, x, y, w, h } => {
                    if let Some(size) = Size::from_wh(*w, *h) {
                        s.push_transform(&Transform::from_translate(*x, *y));
                        let _ = s.draw_svg(&l.figures[*index], size, SvgSettings::default());
                        s.pop();
                    }
                }
            }
        }
        s.finish();
        page.finish();
    }
    // Bookmarks: sections under their report.
    let mut outline = Outline::new();
    let top = l.bookmarks.iter().map(|b| b.level).min().unwrap_or(1);
    let mut parent: Option<OutlineNode> = None;
    for b in &l.bookmarks {
        let node = OutlineNode::new(b.text.clone(), XyzDestination::new(b.page, Point::from_xy(MARGIN, b.y)));
        if b.level == top {
            if let Some(p) = parent.take() {
                outline.push_child(p);
            }
            parent = Some(node);
        } else if let Some(p) = parent.as_mut() {
            p.push_child(node);
        } else {
            outline.push_child(node);
        }
    }
    if let Some(p) = parent {
        outline.push_child(p);
    }
    document.set_outline(outline);
    document.set_metadata(Metadata::new().title(doc.title.clone()).creator("AnovaCraft".into()).language("en".into()));
    document.finish().map_err(|e| format!("could not write the PDF: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html_export::HtmlReport;
    use crate::report::TextTable;
    use crate::report_export::tests::{plant_growth, source};
    use crate::report_export::{ExportScope, PaperSize};

    fn layout_of<'f>(fonts: &'f Fonts, doc: &ReportDocument<'_>) -> Layout<'f> {
        let mut l = Layout {
            fonts,
            width: 612.0,
            height: 792.0,
            pages: Vec::new(),
            y: MARGIN,
            bookmarks: Vec::new(),
            figures: Vec::new(),
        };
        l.new_page();
        for b in &doc.blocks {
            if let Block::Table(t) = b {
                l.table(t);
            }
        }
        l
    }

    #[test]
    fn shaping_measures_and_falls_back() {
        let fonts = Fonts::load().unwrap();
        let a = fonts.line("Mean", false);
        assert_eq!(a.runs.len(), 1);
        assert!(a.width > 1.5 && a.width < 3.5, "{}", a.width);
        // Monospace digits all have the same advance.
        let one = fonts.line("1111", true).width;
        let eight = fonts.line("8888", true).width;
        assert!((one - eight).abs() < 1e-4);
        assert_eq!(fonts.line("12.5", true).runs[0].font, MONO);
        // A character Inter lacks comes from another font; marks stay with
        // their base character.
        let mixed = fonts.line("X\u{304} χ² ≤ 0.05", false);
        assert!(mixed.runs.iter().all(|r| r.glyphs.iter().all(|g| g.glyph_id.to_u32() != 0)), "no missing glyphs");
        let wrapped = fonts.wrap("one two three four five six seven", false, 10.0, 60.0);
        assert!(wrapped.len() > 2);
        assert!(wrapped.iter().all(|l| l.width * 10.0 <= 60.0 + 1e-3));
        let long = fonts.wrap(&"x".repeat(200), true, 10.0, 100.0);
        assert!(long.len() > 3 && long.iter().all(|l| l.width * 10.0 <= 100.0 + 1e-3));
    }

    #[test]
    fn wide_tables_shrink_wrap_or_split() {
        let fonts = Fonts::load().unwrap();
        let l = layout_of(
            &fonts,
            &ReportDocument { title: String::new(), meta: String::new(), blocks: vec![], reports: vec![] },
        );
        let narrow = DocTable::new({
            let mut t = TextTable::new(["Term", "Estimate"]);
            t.push(["Intercept", "1.25"]);
            t
        });
        let cells = |t: &DocTable| -> Vec<Vec<Line>> {
            std::iter::once(t.table.columns.iter().map(|c| fonts.line(c, false)).collect())
                .chain(t.table.rows.iter().map(|r| r.iter().map(|c| fonts.line(c, true)).collect()))
                .collect()
        };
        let plan = l.plan_table(&narrow, &cells(&narrow));
        assert_eq!(plan.len(), 1);
        assert!((plan[0].size - TABLE_SIZE).abs() < 1e-6);
        // Forty numeric columns: split into groups, each led by column 0.
        let names: Vec<String> = (0..40).map(|i| format!("Variable {i}")).collect();
        let mut t = TextTable::new(names.clone());
        t.push(names.iter().map(|_| "-0.123456".to_owned()));
        let wide = DocTable::new(t);
        let plans = l.plan_table(&wide, &cells(&wide));
        assert!(plans.len() > 1);
        let mut seen: Vec<usize> = Vec::new();
        for p in &plans {
            assert_eq!(p.columns[0], 0);
            assert!(p.widths.iter().sum::<f32>() <= l.text_width() + 0.5);
            seen.extend(&p.columns[1..]);
        }
        assert_eq!(seen, (1..40).collect::<Vec<_>>());
    }

    #[test]
    fn long_tables_repeat_their_header_on_each_page() {
        let fonts = Fonts::load().unwrap();
        let mut t = TextTable::new(["Row", "Value"]);
        for i in 0..300 {
            t.push([format!("{i}"), format!("{}.5", i * 3)]);
        }
        let doc = ReportDocument {
            title: "T".into(),
            meta: String::new(),
            blocks: vec![Block::Table(DocTable::new(t))],
            reports: vec![],
        };
        let l = layout_of(&fonts, &doc);
        assert!(l.pages.len() >= 3);
        for page in &l.pages {
            let header = page.iter().filter(|op| matches!(op, Op::Text { bold: true, .. })).count();
            assert_eq!(header, 2, "the header row starts every page");
            for op in page {
                if let Op::Text { y, .. } = op {
                    assert!(*y <= l.bottom() + 1e-3);
                }
            }
        }
    }

    /// A whole export: valid PDF with one page per page laid out, the
    /// section titles in the text and bookmarks, checked with pdftotext
    /// when Poppler is installed.
    #[test]
    fn writes_a_pdf_with_text_figures_and_bookmarks() {
        let (table, rows, computed) = plant_growth();
        let all: Vec<HtmlReport<'_>> = computed.iter().map(HtmlReport::of).collect();
        let doc = ReportDocument::new(ExportScope::All(&all), source(&table, &rows));
        for paper in PaperSize::ALL {
            let pdf = write(&doc, ExportOptions { paper }).unwrap();
            assert!(pdf.starts_with(b"%PDF-"));
            assert!(pdf.windows(9).any(|w| w == b"/Outlines"), "bookmarks");
        }
        let pdf = write(&doc, ExportOptions::default()).unwrap();
        let dir = std::env::temp_dir().join(format!("anovacraft-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("r.pdf");
        std::fs::write(&path, &pdf).unwrap();
        if let Ok(out) = std::process::Command::new("pdftotext").arg("-layout").arg(&path).arg("-").output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for r in &computed {
                    assert!(text.contains(&r.title), "{}", r.title);
                }
                assert!(text.contains("Page 1 of"));
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
