//! Figure pages: several figures on one page, each in a cell of a grid with
//! a panel letter, saved as one file (PNG, SVG, PDF or EPS).
//!
//! A figure is first drawn in the [`VectorFormat::Drawing`] format by any of
//! the export functions ([`crate::export::export_graph`],
//! [`crate::export::export_chart`], …), which keeps its drawing operations
//! as a [`Drawing`] instead of writing a file. [`export_page`] places the
//! drawings in the cells of [`PageLayout::cells`] (each drawing scaled down,
//! never up, to fit its cell, and centred in it), writes a letter in the
//! upper left corner of each cell, and writes the page:
//!
//! * PNG at a chosen pixel density ([`PageOutput::dpi`], recorded in the
//!   file);
//! * PDF and EPS in RGB or device CMYK ([`crate::export::cmyk`]: the simple
//!   complement rule, no colour profile);
//! * any format on a white page or a clear (transparent) one. EPS has no
//!   transparency, so translucent marks are blended with white either way.
//!
//! Sizes are in points (1/72 inch); [`LengthUnit`] converts millimetres and
//! inches. Figures drawn at their cell's size keep their text at its usual
//! point size; a drawing made for a larger cell is shrunk with its text.

use std::fmt::Write as _;

use crate::export::{Canvas, HAlign, PageStyle, VectorFormat, styled_canvas};
use crate::layer::Rgba;
use crate::plot_style::{ExportLook, PlotFont};

/// Colour of panel letters and the page title.
const INK: Rgba = Rgba([30, 32, 40, 255]);

/// First line of an encoded [`Drawing`].
const MAGIC: &str = "ACDRAW 1";

/// A unit for page sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LengthUnit {
    /// Millimetres (25.4 to the inch).
    #[default]
    Millimetres,
    /// Inches (72 points).
    Inches,
}

impl LengthUnit {
    /// Both units.
    pub const ALL: [LengthUnit; 2] = [LengthUnit::Millimetres, LengthUnit::Inches];

    /// Points in `value` of this unit.
    #[must_use]
    pub fn to_points(self, value: f64) -> f64 {
        match self {
            Self::Millimetres => value / 25.4 * 72.0,
            Self::Inches => value * 72.0,
        }
    }

    /// `points` in this unit.
    #[must_use]
    pub fn from_points(self, points: f64) -> f64 {
        match self {
            Self::Millimetres => points / 72.0 * 25.4,
            Self::Inches => points / 72.0,
        }
    }

    /// Short name ("mm", "in").
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Millimetres => "mm",
            Self::Inches => "in",
        }
    }
}

/// How panels are lettered, in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PanelLetters {
    /// A, B, C, …
    #[default]
    Upper,
    /// a, b, c, …
    Lower,
    /// (A), (B), (C), …
    UpperParenthesized,
    /// (a), (b), (c), …
    LowerParenthesized,
    /// No letters.
    None,
}

impl PanelLetters {
    /// Every style.
    pub const ALL: [PanelLetters; 5] = [
        PanelLetters::Upper,
        PanelLetters::Lower,
        PanelLetters::UpperParenthesized,
        PanelLetters::LowerParenthesized,
        PanelLetters::None,
    ];

    /// The letter of panel `index` (0-based): after Z come AA, AB, … as in
    /// spreadsheet column names. Empty for [`PanelLetters::None`].
    #[must_use]
    pub fn letter(self, index: usize) -> String {
        let mut n = index + 1;
        let mut letters = Vec::new();
        while n > 0 {
            letters.push(b'A' + ((n - 1) % 26) as u8);
            n = (n - 1) / 26;
        }
        letters.reverse();
        let upper = String::from_utf8(letters).expect("ASCII letters");
        match self {
            Self::Upper => upper,
            Self::Lower => upper.to_lowercase(),
            Self::UpperParenthesized => format!("({upper})"),
            Self::LowerParenthesized => format!("({})", upper.to_lowercase()),
            Self::None => String::new(),
        }
    }

    /// Display name, showing the first three letters.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Upper => "A, B, C",
            Self::Lower => "a, b, c",
            Self::UpperParenthesized => "(A), (B), (C)",
            Self::LowerParenthesized => "(a), (b), (c)",
            Self::None => "No letters",
        }
    }
}

/// The page: its size, the grid of cells and the lettering.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Columns of the grid (at least 1).
    pub columns: usize,
    /// Space between neighbouring cells, in points.
    pub gap: f64,
    /// Title written across the top of the page (empty for none).
    pub title: String,
    /// Panel lettering.
    pub letters: PanelLetters,
    /// Size of the panel letters in points.
    pub letter_size: f64,
}

impl Default for PageLayout {
    /// A two-column journal figure: 183 mm wide, 120 mm high.
    fn default() -> Self {
        Self {
            width: LengthUnit::Millimetres.to_points(183.0),
            height: LengthUnit::Millimetres.to_points(120.0),
            columns: 2,
            gap: LengthUnit::Millimetres.to_points(2.0),
            title: String::new(),
            letters: PanelLetters::Upper,
            letter_size: 12.0,
        }
    }
}

impl PageLayout {
    /// Smallest page side, in points.
    pub const MIN_SIDE: f64 = 72.0;
    /// Largest page side, in points (about 1.2 m).
    pub const MAX_SIDE: f64 = 3400.0;

    fn size(&self) -> (f64, f64) {
        let side = |v: f64| if v.is_finite() { v.clamp(Self::MIN_SIDE, Self::MAX_SIDE) } else { Self::MIN_SIDE };
        (side(self.width), side(self.height))
    }

    /// Height the title takes at the top of the page.
    fn title_height(&self) -> f64 {
        if self.title.trim().is_empty() { 0.0 } else { 26.0 }
    }

    /// The cell of each panel, `[left, top, right, bottom]` in points with y
    /// downwards, for panels spanning `spans` columns each (clamped to
    /// 1..=columns). Panels fill the grid in reading order; one that does
    /// not fit in what is left of a row starts the next row. Every row has
    /// the same height.
    #[must_use]
    pub fn cells(&self, spans: &[usize]) -> Vec<[f64; 4]> {
        let columns = self.columns.max(1);
        let (width, height) = self.size();
        let gap = if self.gap.is_finite() { self.gap.max(0.0) } else { 0.0 };
        // (row, first column, span) of each panel.
        let mut places = Vec::with_capacity(spans.len());
        let (mut row, mut col) = (0usize, 0usize);
        for &span in spans {
            let span = span.clamp(1, columns);
            if col + span > columns {
                row += 1;
                col = 0;
            }
            places.push((row, col, span));
            col += span;
        }
        let rows = places.last().map_or(1, |p| p.0 + 1);
        let top = self.title_height();
        let column_width = ((width - gap * (columns - 1) as f64) / columns as f64).max(1.0);
        let row_height = ((height - top - gap * (rows - 1) as f64) / rows as f64).max(1.0);
        places
            .into_iter()
            .map(|(r, c, span)| {
                let x0 = c as f64 * (column_width + gap);
                let y0 = top + r as f64 * (row_height + gap);
                [x0, y0, x0 + span as f64 * column_width + (span - 1) as f64 * gap, y0 + row_height]
            })
            .collect()
    }
}

/// How a page is written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageOutput {
    /// File format ([`VectorFormat::Drawing`] keeps the page as a drawing).
    pub format: VectorFormat,
    /// Pixels per inch of a PNG page (36 to 1200).
    pub dpi: f64,
    /// PDF and EPS colours as device CMYK instead of RGB.
    pub cmyk: bool,
    /// A clear background instead of white (PNG, SVG and PDF; EPS pages
    /// are simply left unpainted).
    pub transparent: bool,
    /// Typeface of the text.
    pub font: PlotFont,
}

impl Default for PageOutput {
    fn default() -> Self {
        Self {
            format: VectorFormat::Pdf,
            dpi: crate::raster::PNG_DPI,
            cmyk: false,
            transparent: false,
            font: PlotFont::Sans,
        }
    }
}

impl PageOutput {
    fn style(&self) -> PageStyle {
        PageStyle { dpi: self.dpi, cmyk: self.cmyk, white: Some(!self.transparent), font: self.font }
    }

    /// How a single figure with `look` is written in `format`.
    #[must_use]
    pub fn of_look(look: &ExportLook, format: VectorFormat) -> Self {
        Self { format, dpi: look.pixels_per_inch(), cmyk: false, transparent: look.transparent, font: look.font }
    }
}

/// Size in pixels of a `width` × `height` point page at `dpi`.
#[must_use]
pub fn pixel_size(width: f64, height: f64, dpi: f64) -> (u32, u32) {
    let dpi = dpi.clamp(36.0, 1200.0);
    let px = |v: f64| ((v * dpi / 72.0).ceil() as u32).max(1);
    (px(width), px(height))
}

/// One drawing operation of a [`Drawing`], in the figure's points.
#[derive(Debug, Clone, PartialEq)]
enum Op {
    Line { pts: Vec<[f64; 2]>, color: Rgba, width: f64, dash: Vec<f64>, closed: bool },
    Fill { pts: Vec<[f64; 2]>, color: Rgba },
    FillRings { rings: Vec<Vec<[f64; 2]>>, color: Rgba },
    Text { at: [f64; 2], text: String, size: f64, color: Rgba, align: f64, bold: bool },
    Vertical { at: [f64; 2], text: String, size: f64, color: Rgba, align: f64 },
    Slanted { at: [f64; 2], text: String, size: f64, color: Rgba, align: f64 },
    ClipBegin([f64; 4]),
    ClipEnd,
    Image { rect: [f64; 4], bitmap: crate::bitmap::Bitmap },
}

/// A figure kept as its drawing operations (made by drawing it in the
/// [`VectorFormat::Drawing`] format), so it can be placed on a page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawing {
    /// Width the figure was drawn at, in points.
    pub width: f64,
    /// Height the figure was drawn at, in points.
    pub height: f64,
    ops: Vec<Op>,
}

impl Drawing {
    /// Read the bytes an export function wrote in the
    /// [`VectorFormat::Drawing`] format; `None` for anything else.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut lines = text.lines();
        let head = lines.next()?.strip_prefix(MAGIC)?;
        let mut size = head.split_whitespace().map(str::parse::<f64>);
        let (width, height) = (size.next()?.ok()?, size.next()?.ok()?);
        let mut ops = Vec::new();
        for line in lines {
            ops.push(decode_op(line)?);
        }
        Some(Self { width, height, ops })
    }

    /// The bytes [`Drawing::from_bytes`] reads.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = format!("{MAGIC} {} {}\n", self.width, self.height);
        for op in &self.ops {
            encode_op(&mut out, op);
            out.push('\n');
        }
        out.into_bytes()
    }

    /// Whether nothing is drawn.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Text drawn in the figure, in order (for tests and searches).
    #[must_use]
    pub fn texts(&self) -> Vec<&str> {
        self.ops
            .iter()
            .filter_map(|op| match op {
                Op::Text { text, .. } | Op::Vertical { text, .. } | Op::Slanted { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The scale that fits the drawing into a `width` × `height` box: at most
    /// 1, so drawings are shrunk to fit but never enlarged.
    #[must_use]
    pub fn fit_scale(&self, width: f64, height: f64) -> f64 {
        if self.width <= 0.0 || self.height <= 0.0 {
            return 1.0;
        }
        (width / self.width).min(height / self.height).clamp(0.0, 1.0)
    }

    /// Draw onto `c` with the figure's origin at `origin`, scaled by `k`.
    fn replay(&self, c: &mut dyn Canvas, origin: [f64; 2], k: f64) {
        let at = |p: [f64; 2]| [origin[0] + p[0] * k, origin[1] + p[1] * k];
        let pts = |v: &[[f64; 2]]| v.iter().map(|&p| at(p)).collect::<Vec<_>>();
        for op in &self.ops {
            match op {
                Op::Line { pts: p, color, width, dash, closed } => {
                    let dash: Vec<f64> = dash.iter().map(|d| d * k).collect();
                    c.polyline(&pts(p), *color, width * k, &dash, *closed);
                }
                Op::Fill { pts: p, color } => c.fill(&pts(p), *color),
                Op::FillRings { rings, color } => {
                    let rings: Vec<Vec<[f64; 2]>> = rings.iter().map(|r| pts(r)).collect();
                    c.fill_rings(&rings, *color);
                }
                Op::Text { at: p, text, size, color, align, bold } => {
                    c.text(at(*p), text, size * k, *color, *align, *bold);
                }
                Op::Vertical { at: p, text, size, color, align } => {
                    c.vertical_text(at(*p), text, size * k, *color, *align);
                }
                Op::Slanted { at: p, text, size, color, align } => {
                    c.slanted_text(at(*p), text, size * k, *color, *align);
                }
                Op::ClipBegin(r) => {
                    let (a, b) = (at([r[0], r[1]]), at([r[2], r[3]]));
                    c.clip_begin([a[0], a[1], b[0], b[1]]);
                }
                Op::ClipEnd => c.clip_end(),
                Op::Image { rect: r, bitmap } => {
                    let (a, b) = (at([r[0], r[1]]), at([r[2], r[3]]));
                    c.image([a[0], a[1], b[0], b[1]], bitmap);
                }
            }
        }
    }
}

/// The canvas behind [`VectorFormat::Drawing`]: it keeps every operation.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    ops: Vec<Op>,
}

impl Canvas for Recorder {
    fn polyline(&mut self, pts: &[[f64; 2]], color: Rgba, width: f64, dash: &[f64], closed: bool) {
        self.ops.push(Op::Line { pts: pts.to_vec(), color, width, dash: dash.to_vec(), closed });
    }

    fn fill(&mut self, pts: &[[f64; 2]], color: Rgba) {
        self.ops.push(Op::Fill { pts: pts.to_vec(), color });
    }

    fn fill_rings(&mut self, rings: &[Vec<[f64; 2]>], color: Rgba) {
        self.ops.push(Op::FillRings { rings: rings.to_vec(), color });
    }

    fn text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign, bold: bool) {
        self.ops.push(Op::Text { at, text: text.to_owned(), size, color, align, bold });
    }

    fn vertical_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign) {
        self.ops.push(Op::Vertical { at, text: text.to_owned(), size, color, align });
    }

    fn slanted_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign) {
        self.ops.push(Op::Slanted { at, text: text.to_owned(), size, color, align });
    }

    fn clip_begin(&mut self, rect: [f64; 4]) {
        self.ops.push(Op::ClipBegin(rect));
    }

    fn clip_end(&mut self) {
        self.ops.push(Op::ClipEnd);
    }

    fn image(&mut self, rect: [f64; 4], bitmap: &crate::bitmap::Bitmap) {
        self.ops.push(Op::Image { rect, bitmap: bitmap.clone() });
    }

    fn finish(self: Box<Self>, width: f64, height: f64) -> Vec<u8> {
        Drawing { width, height, ops: self.ops }.to_bytes()
    }
}

// Encoding: one line per operation, a tag letter then numbers separated by
// spaces (f64 written in Rust's shortest round-trip form); text follows a
// tab, with backslash, newline and tab escaped.

fn push_numbers(out: &mut String, values: impl IntoIterator<Item = f64>) {
    for v in values {
        let _ = write!(out, " {v}");
    }
}

fn push_color(out: &mut String, c: Rgba) {
    let [r, g, b, a] = c.0;
    let _ = write!(out, " {r} {g} {b} {a}");
}

fn push_points(out: &mut String, pts: &[[f64; 2]]) {
    let _ = write!(out, " {}", pts.len());
    push_numbers(out, pts.iter().flat_map(|p| [p[0], p[1]]));
}

fn push_text(out: &mut String, text: &str) {
    out.push('\t');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
}

fn encode_op(out: &mut String, op: &Op) {
    match op {
        Op::Line { pts, color, width, dash, closed } => {
            out.push('L');
            push_color(out, *color);
            push_numbers(out, [*width, if *closed { 1.0 } else { 0.0 }, dash.len() as f64]);
            push_numbers(out, dash.iter().copied());
            push_points(out, pts);
        }
        Op::Fill { pts, color } => {
            out.push('F');
            push_color(out, *color);
            push_points(out, pts);
        }
        Op::FillRings { rings, color } => {
            out.push('R');
            push_color(out, *color);
            push_numbers(out, [rings.len() as f64]);
            for r in rings {
                push_points(out, r);
            }
        }
        Op::Text { at, text, size, color, align, bold } => {
            out.push('T');
            push_color(out, *color);
            push_numbers(out, [*size, *align, if *bold { 1.0 } else { 0.0 }, at[0], at[1]]);
            push_text(out, text);
        }
        Op::Vertical { at, text, size, color, align } => {
            out.push('V');
            push_color(out, *color);
            push_numbers(out, [*size, *align, at[0], at[1]]);
            push_text(out, text);
        }
        Op::Slanted { at, text, size, color, align } => {
            out.push('S');
            push_color(out, *color);
            push_numbers(out, [*size, *align, at[0], at[1]]);
            push_text(out, text);
        }
        Op::ClipBegin(r) => {
            out.push('C');
            push_numbers(out, r.iter().copied());
        }
        Op::ClipEnd => out.push('E'),
        Op::Image { rect, bitmap } => {
            out.push('I');
            push_numbers(out, rect.iter().copied());
            push_numbers(out, [f64::from(bitmap.width), f64::from(bitmap.height)]);
            out.push('\t');
            out.push_str(&crate::bitmap::base64_encode(&bitmap.rgba));
        }
    }
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Numbers of an operation line, read in order.
struct Fields<'a>(std::str::SplitWhitespace<'a>);

impl Fields<'_> {
    fn number(&mut self) -> Option<f64> {
        self.0.next()?.parse().ok()
    }

    fn count(&mut self) -> Option<usize> {
        self.0.next()?.parse().ok()
    }

    fn color(&mut self) -> Option<Rgba> {
        let mut c = [0u8; 4];
        for v in &mut c {
            *v = self.0.next()?.parse().ok()?;
        }
        Some(Rgba(c))
    }

    fn points(&mut self) -> Option<Vec<[f64; 2]>> {
        let n = self.count()?;
        (0..n).map(|_| Some([self.number()?, self.number()?])).collect()
    }
}

fn decode_op(line: &str) -> Option<Op> {
    let (head, text) = match line.split_once('\t') {
        Some((h, t)) => (h, Some(unescape(t))),
        None => (line, None),
    };
    let mut chars = head.chars();
    let tag = chars.next()?;
    let mut f = Fields(chars.as_str().split_whitespace());
    Some(match tag {
        'L' => {
            let color = f.color()?;
            let width = f.number()?;
            let closed = f.number()? != 0.0;
            let n = f.count()?;
            let dash = (0..n).map(|_| f.number()).collect::<Option<Vec<_>>>()?;
            Op::Line { pts: f.points()?, color, width, dash, closed }
        }
        'F' => {
            let color = f.color()?;
            Op::Fill { pts: f.points()?, color }
        }
        'R' => {
            let color = f.color()?;
            let n = f.count()?;
            let rings = (0..n).map(|_| f.points()).collect::<Option<Vec<_>>>()?;
            Op::FillRings { rings, color }
        }
        'T' => {
            let color = f.color()?;
            let (size, align, bold) = (f.number()?, f.number()?, f.number()? != 0.0);
            Op::Text { at: [f.number()?, f.number()?], text: text?, size, color, align, bold }
        }
        'V' => {
            let color = f.color()?;
            let (size, align) = (f.number()?, f.number()?);
            Op::Vertical { at: [f.number()?, f.number()?], text: text?, size, color, align }
        }
        'S' => {
            let color = f.color()?;
            let (size, align) = (f.number()?, f.number()?);
            Op::Slanted { at: [f.number()?, f.number()?], text: text?, size, color, align }
        }
        'C' => Op::ClipBegin([f.number()?, f.number()?, f.number()?, f.number()?]),
        'E' => Op::ClipEnd,
        'I' => {
            let rect = [f.number()?, f.number()?, f.number()?, f.number()?];
            let (width, height) = (f.count()? as u32, f.count()? as u32);
            let rgba = crate::bitmap::base64_decode(&text?)?;
            let bitmap = crate::bitmap::Bitmap { width, height, rgba };
            if !bitmap.is_valid() {
                return None;
            }
            Op::Image { rect, bitmap }
        }
        _ => return None,
    })
}

/// Draw the page onto `c`: the title, then each drawing fitted into its
/// cell (missing drawings leave their cell empty), then the letters.
fn draw_page(c: &mut dyn Canvas, layout: &PageLayout, spans: &[usize], drawings: &[Option<&Drawing>]) {
    let (width, _) = layout.size();
    if layout.title_height() > 0.0 {
        c.text([width / 2.0, 17.0], layout.title.trim(), 13.0, INK, 0.5, true);
    }
    let cells = layout.cells(spans);
    for (cell, drawing) in cells.iter().zip(drawings) {
        let Some(d) = drawing else { continue };
        let (w, h) = (cell[2] - cell[0], cell[3] - cell[1]);
        let k = d.fit_scale(w, h);
        let origin = [cell[0] + (w - d.width * k) / 2.0, cell[1] + (h - d.height * k) / 2.0];
        c.clip_begin(*cell);
        d.replay(c, origin, k);
        c.clip_end();
    }
    let size = if layout.letter_size.is_finite() { layout.letter_size.clamp(4.0, 48.0) } else { 12.0 };
    for (i, cell) in cells.iter().enumerate() {
        let letter = layout.letters.letter(i);
        if !letter.is_empty() {
            c.text([cell[0] + 3.0, cell[1] + 3.0 + size * 0.75], &letter, size, INK, 0.0, true);
        }
    }
}

/// The page as a file in `output.format`, with panel `i` spanning
/// `spans[i]` columns and showing `drawings[i]` (`None` leaves the cell
/// empty but still lettered).
#[must_use]
pub fn export_page(layout: &PageLayout, spans: &[usize], drawings: &[Option<&Drawing>], output: PageOutput) -> Vec<u8> {
    let (width, height) = layout.size();
    let mut canvas = styled_canvas(output.format, width, height, output.style());
    draw_page(canvas.as_mut(), layout, spans, drawings);
    canvas.finish(width, height)
}

/// One figure on a page of its own, `width` × `height` points: `drawing`
/// scaled (up or down) to fit, keeping its proportions, and centred.
#[must_use]
pub fn export_figure(drawing: &Drawing, width: f64, height: f64, output: PageOutput) -> Vec<u8> {
    let mut canvas = styled_canvas(output.format, width, height, output.style());
    place_figure(canvas.as_mut(), drawing, width, height);
    canvas.finish(width, height)
}

/// [`export_figure`] drawn into pixels at `dpi` in `font` on white, for
/// previews: `(width, height, RGBA bytes)`.
#[must_use]
pub fn figure_pixels(drawing: &Drawing, width: f64, height: f64, dpi: f64, font: PlotFont) -> (u32, u32, Vec<u8>) {
    let mut raster = crate::raster::Raster::with_font(width, height, dpi, false, font);
    place_figure(&mut raster, drawing, width, height);
    raster.into_rgba()
}

fn place_figure(c: &mut dyn Canvas, d: &Drawing, width: f64, height: f64) {
    if d.width <= 0.0 || d.height <= 0.0 {
        return;
    }
    let k = (width / d.width).min(height / d.height);
    d.replay(c, [(width - d.width * k) / 2.0, (height - d.height * k) / 2.0], k);
}

/// The page size, in points, of a figure drawn with `look` whose own size
/// (on screen) is `natural`: the look's size on paper, else `natural`.
#[must_use]
pub fn look_page_size(look: &ExportLook, natural: (f64, f64)) -> (f64, f64) {
    look.size.map_or(natural, |s| s.points())
}

/// A figure written as `look` says: on a page of the look's size (else
/// `natural`'s own), laid out by `redraw` on a page [`ExportLook::text_size`]
/// times smaller and enlarged to fit, so text, marks and lines grow
/// together. `redraw(width, height)` draws the figure at another size in
/// points (`None` when it cannot, and `natural` is then fitted to the page
/// as drawn).
#[must_use]
pub fn look_figure(
    look: &ExportLook,
    format: VectorFormat,
    natural: &Drawing,
    redraw: Option<&dyn Fn(f64, f64) -> Option<Drawing>>,
) -> Vec<u8> {
    let (width, height) = look_page_size(look, (natural.width, natural.height));
    let k = look.text_scale();
    let drawn = redraw.and_then(|r| r(width / k, height / k));
    export_figure(drawn.as_ref().unwrap_or(natural), width, height, PageOutput::of_look(look, format))
}

/// The page drawn into pixels at `dpi` (as a PNG page would be, on white):
/// `(width, height, RGBA bytes)`, for previews.
#[must_use]
pub fn page_pixels(
    layout: &PageLayout,
    spans: &[usize],
    drawings: &[Option<&Drawing>],
    dpi: f64,
) -> (u32, u32, Vec<u8>) {
    let (width, height) = layout.size();
    let mut raster = crate::raster::Raster::with_style(width, height, dpi, false);
    draw_page(&mut raster, layout, spans, drawings);
    raster.into_rgba()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{ChartPanel, Figure, export_chart};
    use crate::layer::{LinePattern, Mark};

    fn chart(title: &str, width: f64, height: f64) -> Drawing {
        let mut panel = ChartPanel {
            title: "Panel".into(),
            x_title: "Dose\tmg\\kg".into(),
            y_title: "Response".into(),
            marks: vec![Mark::Line {
                points: vec![[0.0, 1.0], [1.0, 3.0], [2.0, 2.0]],
                color: Rgba([200, 30, 40, 128]),
                width: 1.5,
                pattern: LinePattern::Dashed,
                name: "fit".into(),
            }],
            ..ChartPanel::default()
        };
        panel.fit_ranges();
        let bytes = export_chart(&[panel], Figure { title, format: VectorFormat::Drawing, width, height });
        Drawing::from_bytes(&bytes).expect("a drawing")
    }

    #[test]
    fn units_and_letters() {
        assert!((LengthUnit::Millimetres.to_points(25.4) - 72.0).abs() < 1e-12);
        assert!((LengthUnit::Inches.to_points(2.0) - 144.0).abs() < 1e-12);
        assert!((LengthUnit::Millimetres.from_points(72.0) - 25.4).abs() < 1e-12);
        let up: Vec<String> = [0, 1, 25, 26, 27, 701, 702].iter().map(|&i| PanelLetters::Upper.letter(i)).collect();
        assert_eq!(up, ["A", "B", "Z", "AA", "AB", "ZZ", "AAA"]);
        assert_eq!(PanelLetters::Lower.letter(2), "c");
        assert_eq!(PanelLetters::UpperParenthesized.letter(0), "(A)");
        assert_eq!(PanelLetters::LowerParenthesized.letter(3), "(d)");
        assert_eq!(PanelLetters::None.letter(3), "");
    }

    #[test]
    fn drawings_round_trip_exactly() {
        let d = chart("Line\\with \"odd\" text\nand a break", 300.0, 200.0);
        assert!(!d.is_empty());
        assert_eq!((d.width, d.height), (300.0, 200.0));
        let again = Drawing::from_bytes(&d.to_bytes()).unwrap();
        assert_eq!(again, d);
        assert!(d.texts().contains(&"Response"));
        assert!(Drawing::from_bytes(b"<svg/>").is_none());
        assert!(Drawing::from_bytes(b"ACDRAW 1 10 10\nQ 1 2\n").is_none());
    }

    #[test]
    fn cells_fill_rows_and_respect_spans() {
        let layout = PageLayout {
            width: 400.0,
            height: 300.0,
            columns: 2,
            gap: 10.0,
            title: String::new(),
            letters: PanelLetters::Upper,
            letter_size: 12.0,
        };
        let cells = layout.cells(&[1, 1, 2]);
        assert_eq!(cells.len(), 3);
        // Two rows of (300 - 10) / 2 = 145 points; columns of 195.
        assert_eq!(cells[0], [0.0, 0.0, 195.0, 145.0]);
        assert_eq!(cells[1], [205.0, 0.0, 400.0, 145.0]);
        assert_eq!(cells[2], [0.0, 155.0, 400.0, 300.0]);
        // A wide panel after a narrow one moves to the next row; spans are clamped.
        let cells = layout.cells(&[1, 5, 1]);
        // Three rows of (300 - 20) / 3 points.
        let h = 280.0 / 3.0;
        let expected =
            [[0.0, 0.0, 195.0, h], [0.0, h + 10.0, 400.0, 2.0 * h + 10.0], [0.0, 2.0 * h + 20.0, 195.0, 300.0]];
        for (cell, want) in cells.iter().zip(expected) {
            assert!(cell.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9), "{cell:?} vs {want:?}");
        }
        // A title takes space at the top.
        let titled = PageLayout { title: "Figure 1".into(), ..layout };
        assert!(titled.cells(&[1])[0][1] > 20.0);
    }

    #[test]
    fn pages_in_every_format() {
        let layout = PageLayout::default();
        let cells = layout.cells(&[1, 1]);
        let a = chart("First", cells[0][2] - cells[0][0], cells[0][3] - cells[0][1]);
        let b = chart("Second", 540.0, 380.0);
        let drawings = [Some(&a), Some(&b)];
        let spans = [1, 1];
        let svg = String::from_utf8(export_page(
            &layout,
            &spans,
            &drawings,
            PageOutput { format: VectorFormat::Svg, ..PageOutput::default() },
        ))
        .unwrap();
        for text in ["First", "Second", ">A</text>", ">B</text>", "fill=\"#ffffff\""] {
            assert!(svg.contains(text), "{text} in the page");
        }
        // Width: 183 mm.
        assert!(svg.contains("width=\"518.74pt\""), "{}", &svg[..200]);
        let clear = String::from_utf8(export_page(
            &layout,
            &spans,
            &drawings,
            PageOutput { format: VectorFormat::Svg, transparent: true, ..PageOutput::default() },
        ))
        .unwrap();
        assert!(!clear.contains("fill=\"#ffffff\""));
        // The larger drawing is shrunk to its cell, text with it.
        let k = b.fit_scale(cells[1][2] - cells[1][0], cells[1][3] - cells[1][1]);
        assert!(k < 1.0 && k > 0.3);
        assert_eq!(a.fit_scale(cells[0][2] - cells[0][0], cells[0][3] - cells[0][1]), 1.0);

        let pdf =
            export_page(&layout, &spans, &drawings, PageOutput { format: VectorFormat::Pdf, ..PageOutput::default() });
        let pdf = String::from_utf8_lossy(&pdf);
        assert!(pdf.starts_with("%PDF-1.4"));
        assert!(pdf.contains("1 1 1 rg 0 0 518.74"), "white page");
        assert!(pdf.contains(" RG") && !pdf.contains(" K\n"));
        let cmyk = export_page(
            &layout,
            &spans,
            &drawings,
            PageOutput { format: VectorFormat::Pdf, cmyk: true, transparent: true, ..PageOutput::default() },
        );
        let cmyk = String::from_utf8_lossy(&cmyk);
        assert!(cmyk.contains(" K\n") && cmyk.contains(" k\n"));
        assert!(!cmyk.contains(" RG\n") && !cmyk.contains(" rg\n"), "no RGB colour left");
        assert!(!cmyk.contains("re f Q"), "no background");

        let eps = String::from_utf8(export_page(
            &layout,
            &spans,
            &drawings,
            PageOutput { format: VectorFormat::Eps, cmyk: true, ..PageOutput::default() },
        ))
        .unwrap();
        assert!(eps.contains("setcmykcolor") && !eps.contains("setrgbcolor"));
        assert!(eps.contains("0 0 0 0 setcmykcolor 0 0 518.74"));
    }

    #[test]
    fn png_pages_use_the_chosen_density() {
        let layout = PageLayout { width: 144.0, height: 72.0, ..PageLayout::default() };
        let d = chart("", 200.0, 150.0);
        for (dpi, transparent) in [(150.0, false), (600.0, true)] {
            let bytes = export_page(
                &layout,
                &[1],
                &[Some(&d)],
                PageOutput { format: VectorFormat::Png, dpi, transparent, cmyk: false, font: PlotFont::Sans },
            );
            let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
            let mut buf = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut buf).unwrap();
            assert_eq!((info.width, info.height), pixel_size(144.0, 72.0, dpi));
            let ppm = reader.info().pixel_dims.unwrap().xppu;
            assert_eq!(ppm, (dpi / 0.0254).round() as u32);
            // The far corner is outside every mark: white, or clear.
            let corner = &buf[buf.len() - 4..];
            assert_eq!(corner, if transparent { [0, 0, 0, 0] } else { [255, 255, 255, 255] });
        }
        let (w, h, rgba) = page_pixels(&layout, &[1], &[Some(&d)], 72.0);
        assert_eq!((w, h, rgba.len()), (144, 72, 144 * 72 * 4));
    }

    #[test]
    fn cmyk_conversion() {
        assert_eq!(crate::export::cmyk(0.0, 0.0, 0.0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(crate::export::cmyk(1.0, 1.0, 1.0), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(crate::export::cmyk(1.0, 0.0, 0.0), [0.0, 1.0, 1.0, 0.0]);
        let [c, m, y, k] = crate::export::cmyk(0.5, 0.25, 0.25);
        assert!((k - 0.5).abs() < 1e-12 && c.abs() < 1e-12 && (m - 0.5).abs() < 1e-12 && (y - 0.5).abs() < 1e-12);
    }

    /// The text sizes a drawing writes, in order.
    fn text_sizes(d: &Drawing) -> Vec<f64> {
        d.ops
            .iter()
            .filter_map(|op| match op {
                Op::Text { size, .. } | Op::Vertical { size, .. } | Op::Slanted { size, .. } => Some(*size),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn figures_are_written_at_the_size_on_paper() {
        use crate::plot_style::FigureSize;
        let natural = chart("Fit", 400.0, 300.0);
        let redraw = |w: f64, h: f64| Some(chart("Fit", w, h));
        let look = ExportLook {
            size: Some(FigureSize { width: 89.0, height: 60.0, unit: LengthUnit::Millimetres }),
            ..ExportLook::default()
        };
        let (w, h) = (89.0 / 25.4 * 72.0, 60.0 / 25.4 * 72.0);
        let pdf = String::from_utf8_lossy(&look_figure(&look, VectorFormat::Pdf, &natural, Some(&redraw))).into_owned();
        let media = format!("/MediaBox [0 0 {} {}]", crate::export::fmt_points(w), crate::export::fmt_points(h));
        assert!(pdf.contains(&media), "{media} in the PDF");
        let svg = String::from_utf8(look_figure(&look, VectorFormat::Svg, &natural, Some(&redraw))).unwrap();
        assert!(svg.contains(&format!("width=\"{}pt\"", crate::export::fmt_points(w))));
        let eps = String::from_utf8(look_figure(&look, VectorFormat::Eps, &natural, Some(&redraw))).unwrap();
        assert!(eps.contains(&format!("%%HiResBoundingBox: 0 0 {}", crate::export::fmt_points(w))));
        // PNG: the pixels the size and density ask for.
        let png = look_figure(&ExportLook { ppi: 150.0, ..look.clone() }, VectorFormat::Png, &natural, Some(&redraw));
        let info = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap().info().clone();
        assert_eq!((info.width, info.height), pixel_size(w, h, 150.0));
        // Inches too.
        let inches = ExportLook {
            size: Some(FigureSize { width: 3.5, height: 2.5, unit: LengthUnit::Inches }),
            ..ExportLook::default()
        };
        let pdf = String::from_utf8_lossy(&look_figure(&inches, VectorFormat::Pdf, &natural, Some(&redraw))).into_owned();
        assert!(pdf.contains("/MediaBox [0 0 252 180]"));

        // Redrawn at the paper size: text keeps its point size.
        let drawn = Drawing::from_bytes(&look_figure(&look, VectorFormat::Drawing, &natural, Some(&redraw))).unwrap();
        assert_eq!(text_sizes(&drawn), text_sizes(&chart("Fit", w, h)));
        // A picture that cannot be redrawn is fitted to the page as drawn.
        let fitted = Drawing::from_bytes(&look_figure(&look, VectorFormat::Drawing, &natural, None)).unwrap();
        let k = (w / 400.0).min(h / 300.0);
        let expect: Vec<f64> = text_sizes(&natural).iter().map(|s| s * k).collect();
        assert_eq!(text_sizes(&fitted).len(), expect.len());
        assert!(text_sizes(&fitted).iter().zip(&expect).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn text_size_scales_text_and_marks_together() {
        let natural = chart("Fit", 400.0, 300.0);
        let redraw = |w: f64, h: f64| Some(chart("Fit", w, h));
        let look = ExportLook { text_size: 1.5, ..ExportLook::default() };
        let drawn = Drawing::from_bytes(&look_figure(&look, VectorFormat::Drawing, &natural, Some(&redraw))).unwrap();
        // The page keeps the plot's own size; it was laid out 1.5 times smaller.
        assert_eq!((drawn.width, drawn.height), (400.0, 300.0));
        let small = chart("Fit", 400.0 / 1.5, 300.0 / 1.5);
        let expect: Vec<f64> = text_sizes(&small).iter().map(|s| s * 1.5).collect();
        assert!(text_sizes(&drawn).iter().zip(&expect).all(|(a, b)| (a - b).abs() < 1e-6));
        assert!(text_sizes(&drawn).iter().zip(text_sizes(&natural)).all(|(a, b)| (a - b * 1.5).abs() < 1e-6));
        // Out-of-range sizes are kept within 0.5–3.
        assert_eq!(ExportLook { text_size: 9.0, ..ExportLook::default() }.text_scale(), 3.0);
        assert_eq!(ExportLook { text_size: f64::NAN, ..ExportLook::default() }.text_scale(), 1.0);
    }

    #[test]
    fn serif_figures_name_times_and_draw_liberation_serif() {
        let natural = chart("Fit", 300.0, 200.0);
        let serif = ExportLook { font: PlotFont::Serif, ..ExportLook::default() };
        let sans = ExportLook::default();
        let text = |look: &ExportLook, f| String::from_utf8_lossy(&look_figure(look, f, &natural, None)).into_owned();
        let pdf = text(&serif, VectorFormat::Pdf);
        assert!(pdf.contains("/BaseFont /Times-Roman") && pdf.contains("/BaseFont /Times-Bold"));
        assert!(!pdf.contains("Helvetica"));
        assert!(text(&sans, VectorFormat::Pdf).contains("/BaseFont /Helvetica "));
        let eps = text(&serif, VectorFormat::Eps);
        assert!(eps.contains("/Times-Roman /Times-Roman reencode") && eps.contains("/Times-Roman findfont"));
        assert!(!eps.contains("Helvetica"));
        assert!(text(&serif, VectorFormat::Svg).contains("font-family=\"Times New Roman, Times, Liberation Serif, serif\""));
        assert!(text(&sans, VectorFormat::Svg).contains("font-family=\"Helvetica, Arial, Liberation Sans, sans-serif\""));
        // PNG: the same marks, other letter shapes.
        let png_serif = look_figure(&serif, VectorFormat::Png, &natural, None);
        let png_sans = look_figure(&ExportLook { ppi: 300.0, ..sans }, VectorFormat::Png, &natural, None);
        assert_ne!(png_serif, png_sans);
        let (a, b) = (figure_pixels(&natural, 300.0, 200.0, 72.0, PlotFont::Serif), page_pixels(
            &PageLayout { width: 300.0, height: 200.0, columns: 1, gap: 0.0, letters: PanelLetters::None, ..PageLayout::default() },
            &[1],
            &[Some(&natural)],
            72.0,
        ));
        assert_eq!((a.0, a.1), (b.0, b.1));
        assert_ne!(a.2, b.2, "serif and sans text differ");
    }

    #[test]
    fn transparent_figures_leave_the_background_clear() {
        let natural = chart("", 200.0, 150.0);
        let clear = ExportLook { transparent: true, ..ExportLook::default() };
        let png = look_figure(&clear, VectorFormat::Png, &natural, None);
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[buf.len() - 4..], [0, 0, 0, 0]);
        let svg = String::from_utf8(look_figure(&clear, VectorFormat::Svg, &natural, None)).unwrap();
        assert!(!svg.contains("fill=\"#ffffff\"/>\n<"), "no white page under the drawing");
        let white = String::from_utf8(look_figure(&ExportLook::default(), VectorFormat::Svg, &natural, None)).unwrap();
        assert!(white.contains("<rect width=\"200\" height=\"150\" fill=\"#ffffff\"/>"));
    }
}
