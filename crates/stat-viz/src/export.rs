//! Vector export of a graph: SVG, PDF and EPS, drawn from the same frame and
//! layers as the screen, so a saved figure matches what is shown.
//!
//! The page holds the title, the panel grid (each panel with its frame,
//! light grid lines, tick labels, axis titles on the outer panels and any
//! panel headers), the marks clipped to each panel, and the key below. Text
//! is set in Helvetica, or Times for a serif figure ([`PlotFont`]): PDF and
//! EPS use the standard fonts without embedding (characters outside their
//! Latin-1 set are spelled out, e.g. `≤` → `<=`), SVG names the font and its
//! common stand-ins, and PNG draws the metric-compatible Liberation Sans or
//! Liberation Serif.
//!
//! Greek letters go to the standard Symbol font in PDF and EPS. Labels in the
//! math markup of [`crate::math_text`] are set run by run: scripts smaller
//! and raised or dropped, fractions stacked over a rule.
//!
//! PDF output is PDF 1.4 with transparency through graphics-state
//! dictionaries; EPS (PostScript level 2) has no transparency, so
//! translucent colors are blended with white.

use crate::raster::Raster;
use std::fmt::Write as _;

use crate::category_labels::{CategoryLayout, LabelTurn};
use crate::element::GraphSpec;
use crate::frame::{ColorKey, Frame, Panel};
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape, Rgba, TextAnchor};
use crate::math_text::MathText;
use crate::plot_style::PlotFont;
use crate::scale::AxisScale;
use crate::view::{SharedRanges, format_value, scaled_ticks, tick_label};

/// A figure file format: three vector formats and PNG (drawn by the same
/// code at [`crate::raster::PNG_DPI`] pixels per inch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorFormat {
    /// Scalable Vector Graphics.
    Svg,
    /// Portable Document Format.
    Pdf,
    /// Encapsulated PostScript.
    Eps,
    /// Portable Network Graphics (a 300 ppi picture).
    Png,
    /// The drawing operations themselves, for placing the figure on a
    /// figure page ([`crate::figure_page`]). Not a file format, so not in
    /// [`VectorFormat::ALL`].
    Drawing,
}

impl VectorFormat {
    /// Every format.
    pub const ALL: [VectorFormat; 4] = [VectorFormat::Png, VectorFormat::Svg, VectorFormat::Pdf, VectorFormat::Eps];

    /// File extension (without the dot).
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Pdf => "pdf",
            Self::Eps => "eps",
            Self::Png => "png",
            Self::Drawing => "acdraw",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Svg => "SVG",
            Self::Pdf => "PDF",
            Self::Eps => "EPS",
            Self::Png => "PNG",
            Self::Drawing => "Drawing",
        }
    }
}

/// The visible range of each panel: `([x_min, x_max], [y_min, y_max])`.
pub type PanelView = ([f64; 2], [f64; 2]);

/// Each panel's view from shared ranges (with optional overrides by axis
/// name, as set by dragging the axes on screen).
#[must_use]
pub fn panel_views(
    frame: &Frame,
    ranges: &SharedRanges,
    x_override: &dyn Fn(&str) -> Option<(f64, f64)>,
    y_override: &dyn Fn(&str) -> Option<(f64, f64)>,
) -> Vec<PanelView> {
    frame
        .panels
        .iter()
        .map(|p| {
            let x = x_override(&p.x.name).or_else(|| ranges.x.get(&p.x.name).copied()).unwrap_or((0.0, 1.0));
            let y = y_override(&p.y.name).or_else(|| ranges.y.get(&p.y.name).copied()).unwrap_or((0.0, 1.0));
            ([x.0, x.1], [y.0, y.1])
        })
        .collect()
}

/// `view` widened about its centre along one axis so that a `width` ×
/// `height` area shows one unit the same length across and up.
#[must_use]
pub fn equal_aspect_view(view: PanelView, width: f64, height: f64) -> PanelView {
    let ([x0, x1], [y0, y1]) = view;
    let (w, h) = (x1 - x0, y1 - y0);
    if !(w > 0.0 && h > 0.0 && width > 0.0 && height > 0.0) {
        return view;
    }
    let (per_x, per_y) = (w / width, h / height);
    if per_x > per_y {
        let half = 0.5 * per_x * height;
        let c = 0.5 * (y0 + y1);
        ([x0, x1], [c - half, c + half])
    } else {
        let half = 0.5 * per_y * width;
        let c = 0.5 * (x0 + x1);
        ([c - half, c + half], [y0, y1])
    }
}

// ---------------------------------------------------------------------------
// Drawing backends
// ---------------------------------------------------------------------------

/// Horizontal alignment of text: 0 left, 0.5 centre, 1 right.
pub(crate) type HAlign = f64;

/// What every backend draws, in page points with y downwards.
fn draw_labels(c: &mut dyn Canvas, labels: &[Label], map: &dyn Fn([f64; 2]) -> [f64; 2]) {
    for l in labels {
        let p = map(l.at);
        let baseline = p[1] + l.size * 0.72 * (1.0 - l.valign);
        c.text([p[0], baseline], &l.text, l.size, l.color, l.align, l.bold);
    }
}

/// A y-axis title written upwards beside the axis, left of tick labels
/// `tick_width` wide, centred on the plot's height (shortened to fit it).
/// `left` is the leftmost x the letters may reach.
fn y_axis_title(c: &mut dyn Canvas, title: &MathText, plot: [f64; 4], tick_width: f64, left: f64) {
    let size = 8.5;
    // The baseline is the right edge of the turned letters; capitals reach
    // about 0.72 em left of it.
    let x = (plot[0] - 5.0 - tick_width - 4.0).max(left + size * 0.75);
    if let Some(plain) = title.as_plain() {
        let Some(line) = wrap_text(plain, size, plot[3] - plot[1], 1).into_iter().next() else { return };
        c.vertical_text([x, (plot[1] + plot[3]) / 2.0], &line, size, INK, 0.5);
    } else {
        math_text(c, [x, (plot[1] + plot[3]) / 2.0], title, size, INK, 0.5, false, true);
    }
}

/// The right axis's title, written upwards right of its tick labels.
fn y2_axis_title(c: &mut dyn Canvas, title: &MathText, plot: [f64; 4], tick_width: f64) {
    let size = 8.5;
    let x = plot[2] + 5.0 + tick_width + 4.0 + size * 0.75;
    let at = [x, (plot[1] + plot[3]) / 2.0];
    match title.as_plain() {
        Some(plain) => {
            if let Some(line) = wrap_text(plain, size, plot[3] - plot[1], 1).into_iter().next() {
                c.vertical_text(at, &line, size, INK, 0.5);
            }
        }
        None => math_text(c, at, title, size, INK, 0.5, false, true),
    }
}

/// Draw `text` (math markup already parsed) with its baseline through
/// `at`: a plain label exactly as [`Canvas::text`] (or, `vertical`,
/// [`Canvas::vertical_text`]) draws it; otherwise run by run, scripts and
/// fractions placed by [`MathText::layout`] with the canvas's own text
/// widths, and fraction rules as lines.
#[allow(clippy::too_many_arguments)]
fn math_text(
    c: &mut dyn Canvas,
    at: [f64; 2],
    text: &MathText,
    size: f64,
    color: Rgba,
    align: HAlign,
    bold: bool,
    vertical: bool,
) {
    if let Some(plain) = text.as_plain() {
        if vertical {
            c.vertical_text(at, plain, size, color, align);
        } else {
            c.text(at, plain, size, color, align, bold);
        }
        return;
    }
    let layout = {
        let measure = |t: &str, s: f64| c.measure(t, s) * if bold { 1.08 } else { 1.0 };
        text.layout(size, &measure)
    };
    let start = -align * layout.width;
    // A point `along` the baseline and `up` from it, on the page.
    let place = |along: f64, up: f64| {
        if vertical { [at[0] - up, at[1] - (start + along)] } else { [at[0] + start + along, at[1] - up] }
    };
    for run in &layout.runs {
        let p = place(run.x, run.rise);
        if vertical {
            c.vertical_text(p, &run.text, run.size, color, 0.0);
        } else {
            c.text(p, &run.text, run.size, color, 0.0, bold);
        }
    }
    for rule in &layout.rules {
        c.polyline(&[place(rule.x0, rule.rise), place(rule.x1, rule.rise)], color, rule.thickness, &[], false);
    }
}

/// [`math_text`] for one line set left to right, regular weight.
pub(crate) fn draw_math(c: &mut dyn Canvas, at: [f64; 2], text: &MathText, size: f64, color: Rgba, align: HAlign) {
    math_text(c, at, text, size, color, align, false, false);
}

/// [`math_width`] for the axes of report figures.
pub(crate) fn markup_width(text: &MathText, size: f64) -> f64 {
    math_width(text, size)
}

/// A vertical axis's title left of tick labels `tick_width` wide (see
/// [`y_axis_title`]).
pub(crate) fn left_axis_title(c: &mut dyn Canvas, title: &MathText, plot: [f64; 4], tick_width: f64, left: f64) {
    y_axis_title(c, title, plot, tick_width, left);
}

/// A vertical axis's title right of the plot (see [`y2_axis_title`]).
pub(crate) fn right_axis_title(c: &mut dyn Canvas, title: &MathText, plot: [f64; 4], tick_width: f64) {
    y2_axis_title(c, title, plot, tick_width);
}

/// Width of a label in markup at `size` (approximate Helvetica widths).
fn math_width(text: &MathText, size: f64) -> f64 {
    match text.as_plain() {
        Some(t) => text_width(t, size),
        None => text.layout(size, &text_width).width,
    }
}

/// A blank page in `format`.
fn new_canvas(format: VectorFormat, width: f64, height: f64) -> Box<dyn Canvas> {
    styled_canvas(format, width, height, PageStyle::default())
}

/// How a page is set up beyond its size: used by figure pages
/// ([`crate::figure_page`]); single figures use the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PageStyle {
    /// PNG pixels per inch.
    pub dpi: f64,
    /// PDF and EPS colours written as CMYK (see [`cmyk`]).
    pub cmyk: bool,
    /// Background: `None` keeps each format's own (white, and none for PDF),
    /// `Some(true)` paints white, `Some(false)` leaves the page clear.
    pub white: Option<bool>,
    /// Typeface of the text.
    pub font: PlotFont,
}

impl Default for PageStyle {
    fn default() -> Self {
        Self { dpi: crate::raster::PNG_DPI, cmyk: false, white: None, font: PlotFont::Sans }
    }
}

/// A blank page in `format` set up as `style` says.
pub(crate) fn styled_canvas(format: VectorFormat, width: f64, height: f64, style: PageStyle) -> Box<dyn Canvas> {
    let white = |default: bool| style.white.unwrap_or(default);
    let ps = |pdf: bool| PostScriptLike {
        ops: String::new(),
        height,
        pdf,
        alphas: Vec::new(),
        cmyk: style.cmyk,
        background: white(!pdf),
        symbol: false,
        serif: style.font == PlotFont::Serif,
        images: Vec::new(),
    };
    match format {
        VectorFormat::Svg => {
            Box::new(Svg { body: String::new(), clip: 0, images: false, background: white(true), font: style.font })
        }
        VectorFormat::Pdf => Box::new(ps(true)),
        VectorFormat::Eps => Box::new(ps(false)),
        VectorFormat::Png => Box::new(Raster::with_font(width, height, style.dpi, !white(true), style.font)),
        VectorFormat::Drawing => Box::new(crate::figure_page::Recorder::default()),
    }
}

/// Device CMYK of an sRGB colour by the simple complement rule (no colour
/// profile): K = 1 − max(R, G, B), and C = (1 − R − K) / (1 − K), likewise M
/// and Y, so black is K alone and white is no ink.
#[must_use]
pub fn cmyk(r: f64, g: f64, b: f64) -> [f64; 4] {
    let k = 1.0 - r.max(g).max(b);
    if k >= 1.0 - 1e-12 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let ink = |v: f64| ((1.0 - v - k) / (1.0 - k)).clamp(0.0, 1.0);
    [ink(r), ink(g), ink(b), k]
}

impl Canvas for Raster {
    fn polyline(&mut self, pts: &[[f64; 2]], color: Rgba, width: f64, dash: &[f64], closed: bool) {
        Raster::polyline(self, pts, color, width, dash, closed);
    }

    fn fill(&mut self, pts: &[[f64; 2]], color: Rgba) {
        Raster::fill(self, pts, color);
    }

    fn fill_rings(&mut self, rings: &[Vec<[f64; 2]>], color: Rgba) {
        Raster::fill_rings(self, rings, color);
    }

    fn text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign, bold: bool) {
        Raster::text(self, at, text, size, color, align, bold);
    }

    fn vertical_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign) {
        Raster::vertical_text(self, at, text, size, color, align);
    }

    fn slanted_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign) {
        Raster::turned_text(self, at, text, size, color, align, -45.0);
    }

    fn clip_begin(&mut self, rect: [f64; 4]) {
        Raster::clip_begin(self, rect);
    }

    fn clip_end(&mut self) {
        Raster::clip_end(self);
    }

    fn image(&mut self, rect: [f64; 4], bitmap: &crate::bitmap::Bitmap) {
        Raster::image(self, rect, bitmap);
    }

    fn finish(self: Box<Self>, _width: f64, _height: f64) -> Vec<u8> {
        Raster::finish(*self)
    }

    fn measure(&self, text: &str, size: f64) -> f64 {
        Raster::measure(self, text, size)
    }
}

pub(crate) trait Canvas {
    fn polyline(&mut self, pts: &[[f64; 2]], color: Rgba, width: f64, dash: &[f64], closed: bool);
    fn fill(&mut self, pts: &[[f64; 2]], color: Rgba);
    /// Fill closed rings as one shape by the even–odd rule (rings inside
    /// rings are holes).
    fn fill_rings(&mut self, rings: &[Vec<[f64; 2]>], color: Rgba);
    /// Text with its baseline at `at[1]`.
    fn text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign, bold: bool);
    /// Text turned a quarter turn anticlockwise (reading upwards), its
    /// baseline on the vertical line through `at`; `align` places it along
    /// that line relative to `at[1]` (0 starts there, 0.5 centres).
    fn vertical_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign);
    /// Text turned 45° anticlockwise (reading up and to the right), its
    /// baseline on the slanted line through `at`; `align` places it along
    /// that line as for [`Canvas::vertical_text`].
    fn slanted_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign);
    fn clip_begin(&mut self, rect: [f64; 4]);
    fn clip_end(&mut self);
    /// A bitmap stretched over the page rectangle `[left, top, right,
    /// bottom]`, its first row at the top, each cell drawn as a flat block
    /// (no smoothing between cells).
    fn image(&mut self, rect: [f64; 4], bitmap: &crate::bitmap::Bitmap);
    fn finish(self: Box<Self>, width: f64, height: f64) -> Vec<u8>;
    /// Advance width of `text` at `size` points (for setting math runs).
    fn measure(&self, text: &str, size: f64) -> f64 {
        text_width(text, size)
    }
}

/// `v` as figure files write a length in points (at most two decimals).
#[cfg(test)]
pub(crate) fn fmt_points(v: f64) -> String {
    fmt(v)
}

fn fmt(v: f64) -> String {
    let v = if v.abs() < 1e-9 { 0.0 } else { v };
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

/// The finite points of `pts`: a missing coordinate cannot be written into
/// SVG, PDF or EPS (`NaN` is not a number there and stops a PostScript
/// interpreter), so such vertices are skipped and the path joins across
/// them, as the PNG writer does.
fn finite_points(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    pts.iter().copied().filter(|p| p[0].is_finite() && p[1].is_finite()).collect()
}

/// `true` when a text anchor can be written.
fn finite_at(at: [f64; 2]) -> bool {
    at[0].is_finite() && at[1].is_finite()
}

/// Splits `text` at spaces into at most `max_lines` lines no wider than
/// `max_width`; the last line ends in an ellipsis when text is left over.
fn wrap_text(text: &str, size: f64, max_width: f64, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if current.is_empty() || text_width(&candidate, size) <= max_width {
            current = candidate;
            continue;
        }
        if lines.len() + 1 == max_lines {
            // No room for another line: cut this one short.
            let mut cut = candidate;
            while text_width(&format!("{cut}…"), size) > max_width && cut.pop().is_some() {}
            lines.push(format!("{}…", cut.trim_end()));
            return lines;
        }
        lines.push(std::mem::replace(&mut current, word.to_owned()));
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Approximate Helvetica advance width of `text` at `size` points.
fn text_width(text: &str, size: f64) -> f64 {
    text.chars()
        .map(|c| match c {
            'i' | 'j' | 'l' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' => 0.24,
            'f' | 't' | 'r' | 'I' | ' ' | '(' | ')' | '[' | ']' | '-' => 0.33,
            'm' | 'w' | 'M' | 'W' => 0.83,
            c if c.is_ascii_uppercase() => 0.67,
            c if c.is_ascii_digit() => 0.556,
            _ => 0.52,
        })
        .sum::<f64>()
        * size
}

struct Svg {
    body: String,
    clip: usize,
    /// A bitmap was placed (the root then declares the `xlink` namespace,
    /// which older readers want for image links).
    images: bool,
    /// A white rectangle under the drawing.
    background: bool,
    font: PlotFont,
}

impl Svg {
    fn color(c: Rgba) -> (String, f64) {
        let [r, g, b, a] = c.0;
        (format!("#{r:02x}{g:02x}{b:02x}"), f64::from(a) / 255.0)
    }
}

/// The SVG `font-family` list of `font`: the standard face, its common
/// metric-compatible stand-ins, then the generic family.
fn svg_family(font: PlotFont) -> &'static str {
    match font {
        PlotFont::Sans => "Helvetica, Arial, Liberation Sans, sans-serif",
        PlotFont::Serif => "Times New Roman, Times, Liberation Serif, serif",
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

impl Canvas for Svg {
    fn polyline(&mut self, pts: &[[f64; 2]], color: Rgba, width: f64, dash: &[f64], closed: bool) {
        let pts = &finite_points(pts);
        if pts.len() < 2 {
            return;
        }
        let (c, a) = Svg::color(color);
        let points: Vec<String> = pts.iter().map(|p| format!("{},{}", fmt(p[0]), fmt(p[1]))).collect();
        let tag = if closed { "polygon" } else { "polyline" };
        let dash = if dash.is_empty() {
            String::new()
        } else {
            format!(" stroke-dasharray=\"{}\"", dash.iter().map(|d| fmt(*d)).collect::<Vec<_>>().join(" "))
        };
        let _ = writeln!(
            self.body,
            "<{tag} points=\"{}\" fill=\"none\" stroke=\"{c}\" stroke-opacity=\"{}\" stroke-width=\"{}\" \
             stroke-linejoin=\"round\"{dash}/>",
            points.join(" "),
            fmt(a),
            fmt(width)
        );
    }

    fn fill(&mut self, pts: &[[f64; 2]], color: Rgba) {
        let pts = &finite_points(pts);
        if pts.len() < 3 {
            return;
        }
        let (c, a) = Svg::color(color);
        let points: Vec<String> = pts.iter().map(|p| format!("{},{}", fmt(p[0]), fmt(p[1]))).collect();
        let _ =
            writeln!(self.body, "<polygon points=\"{}\" fill=\"{c}\" fill-opacity=\"{}\"/>", points.join(" "), fmt(a));
    }

    fn fill_rings(&mut self, rings: &[Vec<[f64; 2]>], color: Rgba) {
        let mut d = String::new();
        for ring in rings {
            let ring = finite_points(ring);
            if ring.len() < 3 {
                continue;
            }
            for (i, p) in ring.iter().enumerate() {
                let _ = write!(d, "{}{},{}", if i == 0 { "M" } else { "L" }, fmt(p[0]), fmt(p[1]));
            }
            d.push('Z');
        }
        if d.is_empty() {
            return;
        }
        let (c, a) = Svg::color(color);
        let _ = writeln!(self.body, "<path d=\"{d}\" fill=\"{c}\" fill-opacity=\"{}\" fill-rule=\"evenodd\"/>", fmt(a));
    }

    fn text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign, bold: bool) {
        if !finite_at(at) {
            return;
        }
        let (c, a) = Svg::color(color);
        let anchor = if align < 0.25 {
            "start"
        } else if align > 0.75 {
            "end"
        } else {
            "middle"
        };
        let weight = if bold { " font-weight=\"bold\"" } else { "" };
        let _ = writeln!(
            self.body,
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{c}\" fill-opacity=\"{}\" text-anchor=\"{anchor}\"{weight}>{}</text>",
            fmt(at[0]),
            fmt(at[1]),
            fmt(size),
            fmt(a),
            xml_escape(text)
        );
    }

    fn vertical_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign) {
        if !finite_at(at) {
            return;
        }
        let (c, a) = Svg::color(color);
        let anchor = if align < 0.25 {
            "start"
        } else if align > 0.75 {
            "end"
        } else {
            "middle"
        };
        let _ = writeln!(
            self.body,
            "<text transform=\"translate({},{}) rotate(-90)\" font-size=\"{}\" fill=\"{c}\" fill-opacity=\"{}\" \
             text-anchor=\"{anchor}\">{}</text>",
            fmt(at[0]),
            fmt(at[1]),
            fmt(size),
            fmt(a),
            xml_escape(text)
        );
    }

    fn slanted_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign) {
        if !finite_at(at) {
            return;
        }
        let (c, a) = Svg::color(color);
        let anchor = if align < 0.25 {
            "start"
        } else if align > 0.75 {
            "end"
        } else {
            "middle"
        };
        let _ = writeln!(
            self.body,
            "<text transform=\"translate({},{}) rotate(-45)\" font-size=\"{}\" fill=\"{c}\" fill-opacity=\"{}\" \
             text-anchor=\"{anchor}\">{}</text>",
            fmt(at[0]),
            fmt(at[1]),
            fmt(size),
            fmt(a),
            xml_escape(text)
        );
    }

    fn clip_begin(&mut self, r: [f64; 4]) {
        self.clip += 1;
        let _ = writeln!(
            self.body,
            "<clipPath id=\"c{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath><g clip-path=\"url(#c{})\">",
            self.clip,
            fmt(r[0]),
            fmt(r[1]),
            fmt(r[2] - r[0]),
            fmt(r[3] - r[1]),
            self.clip
        );
    }

    fn clip_end(&mut self) {
        self.body.push_str("</g>\n");
    }

    fn image(&mut self, r: [f64; 4], bitmap: &crate::bitmap::Bitmap) {
        if !bitmap.is_valid() || !r.iter().all(|v| v.is_finite()) || r[2] <= r[0] || r[3] <= r[1] {
            return;
        }
        self.images = true;
        let data = crate::bitmap::base64_encode(&bitmap.to_png());
        // `image-rendering` keeps each cell a sharp block when enlarged.
        let _ = writeln!(
            self.body,
            "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" \
             image-rendering=\"pixelated\" style=\"image-rendering:pixelated;image-rendering:crisp-edges\" \
             xlink:href=\"data:image/png;base64,{data}\"/>",
            fmt(r[0]),
            fmt(r[1]),
            fmt(r[2] - r[0]),
            fmt(r[3] - r[1]),
        );
    }

    fn finish(self: Box<Self>, width: f64, height: f64) -> Vec<u8> {
        let (w, h) = (fmt(width), fmt(height));
        let background = if self.background {
            format!("<rect width=\"{w}\" height=\"{h}\" fill=\"#ffffff\"/>\n")
        } else {
            String::new()
        };
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"{} width=\"{w}pt\" \
             height=\"{h}pt\" viewBox=\"0 0 {w} {h}\" font-family=\"{}\">\n{background}{}</svg>\n",
            if self.images { " xmlns:xlink=\"http://www.w3.org/1999/xlink\"" } else { "" },
            svg_family(self.font),
            self.body,
        )
        .into_bytes()
    }
}

/// Text for the standard Helvetica font: Latin-1 kept, a few symbols spelled
/// out, anything else replaced by `?`. Returns bytes in WinAnsi encoding.
fn latin1_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in text.chars() {
        let spelled: Option<&str> = match c {
            '≤' => Some("<="),
            '≥' => Some(">="),
            '−' | '–' => Some("-"),
            '—' => Some("--"),
            '…' => Some("..."),
            '›' => Some(">"),
            '‹' => Some("<"),
            '₀' => Some("0"),
            '₁' => Some("1"),
            '√' => Some("sqrt"),
            '∈' => Some(" in "),
            '“' | '”' => Some("\""),
            '‘' | '’' => Some("'"),
            _ => None,
        };
        match spelled {
            Some(s) => out.extend_from_slice(s.as_bytes()),
            None => {
                let u = c as u32;
                out.push(if u < 256 { u as u8 } else { b'?' });
            }
        }
    }
    out
}

/// The code of `c` in the standard Symbol font, for Greek letters and the
/// few mathematical signs Latin-1 lacks.
fn symbol_code(c: char) -> Option<u8> {
    const GREEK_LOWER: &str = "abgdezhqiklmnxoprVstufcyw";
    const GREEK_UPPER: &str = "ABGDEZHQIKLMNXOPRVSTUFCYW";
    let u = c as u32;
    let pick = |table: &str, k: u32| table.as_bytes().get(k as usize).copied().filter(|b| *b != b'V');
    match c {
        // α (U+03B1) … ω (U+03C9), with final sigma ς at U+03C2.
        'ς' => Some(b'V'),
        '\u{3b1}'..='\u{3c9}' => pick(GREEK_LOWER, u - 0x3b1),
        '\u{391}'..='\u{3a9}' => pick(GREEK_UPPER, u - 0x391),
        'ϑ' => Some(b'J'),
        'ϕ' => Some(b'j'),
        '≠' => Some(0xB9),
        '≈' => Some(0xBB),
        '∞' => Some(0xA5),
        '∂' => Some(0xB6),
        _ => None,
    }
}

/// `text` split into runs for Helvetica (`false`, Latin-1 bytes) and for the
/// Symbol font (`true`).
fn font_segments(text: &str) -> Vec<(bool, Vec<u8>)> {
    let mut out: Vec<(bool, Vec<u8>)> = Vec::new();
    let mut plain = String::new();
    for c in text.chars() {
        match symbol_code(c) {
            Some(code) => {
                if !plain.is_empty() {
                    out.push((false, latin1_bytes(&std::mem::take(&mut plain))));
                }
                match out.last_mut() {
                    Some((true, bytes)) => bytes.push(code),
                    _ => out.push((true, vec![code])),
                }
            }
            None => plain.push(c),
        }
    }
    if !plain.is_empty() {
        out.push((false, latin1_bytes(&plain)));
    }
    out
}

/// PostScript / PDF string literal of `bytes`.
fn ps_string(bytes: &[u8]) -> String {
    let mut s = String::from("(");
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(b as char);
            }
            32..=126 => s.push(b as char),
            _ => {
                let _ = write!(s, "\\{b:03o}");
            }
        }
    }
    s.push(')');
    s
}

/// Shared by PDF and EPS: page-space operators with y flipped.
struct PostScriptLike {
    ops: String,
    height: f64,
    /// PDF: alpha via graphics states (`/GSk gs`); EPS: blend with white.
    pdf: bool,
    alphas: Vec<u8>,
    /// The Symbol font was used (PDF lists it as a resource only then).
    symbol: bool,
    /// Colours written as device CMYK.
    cmyk: bool,
    /// A white page under the drawing.
    background: bool,
    /// Text in Times instead of Helvetica.
    serif: bool,
    /// PDF: the image XObjects the content draws (`/Im0`, `/Im1`, …).
    images: Vec<PdfImage>,
}

/// A bitmap held as a PDF image XObject: the colour samples, and the alpha
/// samples as a soft mask when some cell is not opaque.
struct PdfImage {
    width: u32,
    height: u32,
    /// `/DeviceRGB` or `/DeviceCMYK`.
    space: &'static str,
    /// Colour samples, zlib-compressed then ASCII85-encoded.
    data: String,
    /// Alpha samples, likewise encoded.
    alpha: Option<String>,
}

impl PostScriptLike {
    /// The standard PostScript font of the text (`bold` for headings).
    fn face(&self, bold: bool) -> &'static str {
        match (self.serif, bold) {
            (false, false) => "Helvetica",
            (false, true) => "Helvetica-Bold",
            (true, false) => "Times-Roman",
            (true, true) => "Times-Bold",
        }
    }

    fn y(&self, y: f64) -> f64 {
        self.height - y
    }

    /// Set the color (stroke or fill) and, for PDF, the alpha.
    fn set_color(&mut self, c: Rgba, stroke: bool) {
        let [r, g, b, a] = c.0;
        let (r, g, b) = if self.pdf {
            (f64::from(r), f64::from(g), f64::from(b))
        } else {
            // No transparency in PostScript: blend with white.
            let t = f64::from(a) / 255.0;
            let mix = |v: u8| t * f64::from(v) + (1.0 - t) * 255.0;
            (mix(r), mix(g), mix(b))
        };
        let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
        if self.cmyk {
            let [c, m, y, k] = cmyk(r, g, b);
            let ink = format!("{} {} {} {}", fmt3(c), fmt3(m), fmt3(y), fmt3(k));
            if self.pdf {
                let n = self.alpha_state(a);
                let _ = writeln!(self.ops, "/GS{n} gs\n{ink} {}", if stroke { "K" } else { "k" });
            } else {
                let _ = writeln!(self.ops, "{ink} setcmykcolor");
            }
            return;
        }
        if self.pdf {
            let k = match self.alphas.iter().position(|&x| x == a) {
                Some(k) => k,
                None => {
                    self.alphas.push(a);
                    self.alphas.len() - 1
                }
            };
            let _ = writeln!(self.ops, "/GS{k} gs");
            let _ = writeln!(self.ops, "{} {} {} {}", fmt3(r), fmt3(g), fmt3(b), if stroke { "RG" } else { "rg" });
        } else {
            let _ = writeln!(self.ops, "{} {} {} setrgbcolor", fmt3(r), fmt3(g), fmt3(b));
        }
    }

    /// Text in Helvetica (`face`) and Symbol runs, starting at the current
    /// point: PDF operators for inside `BT … ET`, or PostScript that first
    /// moves back by `align` of the width and then shows each run.
    fn mixed_runs(&mut self, segments: &[(bool, Vec<u8>)], size: f64, face: (&str, &str), align: f64) {
        self.symbol = true;
        if self.pdf {
            for (symbol, bytes) in segments {
                let font = if *symbol { "F3" } else { face.0 };
                let _ = write!(self.ops, " /{font} {} Tf {} Tj", fmt(size), ps_string(bytes));
            }
            return;
        }
        let set = |symbol: bool| {
            format!("/{} findfont {} scalefont setfont", if symbol { "Symbol" } else { face.1 }, fmt(size))
        };
        let _ = write!(self.ops, " 0");
        for (symbol, bytes) in segments {
            let _ = write!(self.ops, " {} {} stringwidth pop add", set(*symbol), ps_string(bytes));
        }
        let _ = write!(self.ops, " {} mul neg 0 rmoveto", fmt3(align));
        for (symbol, bytes) in segments {
            let _ = write!(self.ops, " {} {} show", set(*symbol), ps_string(bytes));
        }
    }

    /// Index of the PDF graphics state with alpha `a` (added when new).
    fn alpha_state(&mut self, a: u8) -> usize {
        match self.alphas.iter().position(|&x| x == a) {
            Some(k) => k,
            None => {
                self.alphas.push(a);
                self.alphas.len() - 1
            }
        }
    }

    /// A bitmap's colour samples: RGB, or CMYK when the page is in CMYK.
    /// PDF keeps alpha apart (a soft mask); EPS has no transparency, so
    /// translucent cells are blended with white, as colours are.
    fn samples(&self, bitmap: &crate::bitmap::Bitmap) -> Vec<u8> {
        let n = if self.cmyk { 4 } else { 3 };
        let mut out = Vec::with_capacity(bitmap.rgba.len() / 4 * n);
        for p in bitmap.rgba.chunks_exact(4) {
            let [r, g, b] = if self.pdf {
                [p[0], p[1], p[2]]
            } else {
                let t = f64::from(p[3]) / 255.0;
                let mix = |v: u8| (t * f64::from(v) + (1.0 - t) * 255.0).round() as u8;
                [mix(p[0]), mix(p[1]), mix(p[2])]
            };
            if self.cmyk {
                let ink = cmyk(f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
                out.extend(ink.iter().map(|v| (v * 255.0).round() as u8));
            } else {
                out.extend_from_slice(&[r, g, b]);
            }
        }
        out
    }

    fn path(&mut self, pts: &[[f64; 2]]) {
        let (m, l) = if self.pdf { ("m", "l") } else { ("moveto", "lineto") };
        for (i, p) in pts.iter().enumerate() {
            let _ = writeln!(self.ops, "{} {} {}", fmt(p[0]), fmt(self.y(p[1])), if i == 0 { m } else { l });
        }
    }
}

fn fmt3(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl Canvas for PostScriptLike {
    fn polyline(&mut self, pts: &[[f64; 2]], color: Rgba, width: f64, dash: &[f64], closed: bool) {
        let pts = &finite_points(pts);
        if pts.len() < 2 {
            return;
        }
        self.ops.push_str(if self.pdf { "q\n" } else { "gsave\n" });
        self.set_color(color, true);
        let dash_text = dash.iter().map(|d| fmt(*d)).collect::<Vec<_>>().join(" ");
        if self.pdf {
            let _ = writeln!(self.ops, "{} w 1 j [{dash_text}] 0 d", fmt(width));
        } else {
            let _ = writeln!(self.ops, "{} setlinewidth 1 setlinejoin [{dash_text}] 0 setdash", fmt(width));
        }
        self.path(pts);
        if closed {
            self.ops.push_str(if self.pdf { "h\n" } else { "closepath\n" });
        }
        self.ops.push_str(if self.pdf { "S\nQ\n" } else { "stroke\ngrestore\n" });
    }

    fn fill(&mut self, pts: &[[f64; 2]], color: Rgba) {
        let pts = &finite_points(pts);
        if pts.len() < 3 {
            return;
        }
        self.ops.push_str(if self.pdf { "q\n" } else { "gsave\n" });
        self.set_color(color, false);
        self.path(pts);
        self.ops.push_str(if self.pdf { "h f\nQ\n" } else { "closepath fill\ngrestore\n" });
    }

    fn fill_rings(&mut self, rings: &[Vec<[f64; 2]>], color: Rgba) {
        let rings: Vec<Vec<[f64; 2]>> = rings.iter().map(|r| finite_points(r)).filter(|r| r.len() >= 3).collect();
        if rings.is_empty() {
            return;
        }
        self.ops.push_str(if self.pdf { "q\n" } else { "gsave\n" });
        self.set_color(color, false);
        if !self.pdf {
            self.ops.push_str("newpath\n");
        }
        for ring in &rings {
            self.path(ring);
            self.ops.push_str(if self.pdf { "h\n" } else { "closepath\n" });
        }
        self.ops.push_str(if self.pdf { "f*\nQ\n" } else { "eofill\ngrestore\n" });
    }

    fn text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign, bold: bool) {
        if !finite_at(at) {
            return;
        }
        let bytes = latin1_bytes(text);
        let font = if bold { "F2" } else { "F1" };
        self.ops.push_str(if self.pdf { "q\n" } else { "gsave\n" });
        self.set_color(color, false);
        let (x, y) = (at[0], self.y(at[1]));
        let segments = font_segments(text);
        if segments.iter().any(|(symbol, _)| *symbol) {
            let face = (font, self.face(bold));
            if self.pdf {
                let shift = align * text_width(text, size);
                let _ = write!(self.ops, "BT {} {} Td", fmt(x - shift), fmt(y));
                self.mixed_runs(&segments, size, face, align);
                self.ops.push_str(" ET\n");
            } else {
                let _ = write!(self.ops, "{} {} moveto", fmt(x), fmt(y));
                self.mixed_runs(&segments, size, face, align);
                self.ops.push('\n');
            }
        } else if self.pdf {
            let shift = align * text_width(text, size);
            let _ = writeln!(
                self.ops,
                "BT /{font} {} Tf {} {} Td {} Tj ET",
                fmt(size),
                fmt(x - shift),
                fmt(y),
                ps_string(&bytes)
            );
        } else {
            let face = self.face(bold);
            let s = ps_string(&bytes);
            let _ = writeln!(
                self.ops,
                "/{face} findfont {} scalefont setfont {} {} moveto {s} dup stringwidth pop {} mul neg 0 rmoveto show",
                fmt(size),
                fmt(x),
                fmt(y),
                fmt3(align)
            );
        }
        self.ops.push_str(if self.pdf { "Q\n" } else { "grestore\n" });
    }

    fn vertical_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign) {
        if !finite_at(at) {
            return;
        }
        let bytes = latin1_bytes(text);
        self.ops.push_str(if self.pdf { "q\n" } else { "gsave\n" });
        self.set_color(color, false);
        let (x, y) = (at[0], self.y(at[1]));
        let segments = font_segments(text);
        if segments.iter().any(|(symbol, _)| *symbol) {
            if self.pdf {
                let shift = align * text_width(text, size);
                let _ = write!(self.ops, "BT 0 1 -1 0 {} {} Tm", fmt(x), fmt(y - shift));
                self.mixed_runs(&segments, size, ("F1", self.face(false)), align);
                self.ops.push_str(" ET\n");
            } else {
                let _ = write!(self.ops, "{} {} translate 90 rotate 0 0 moveto", fmt(x), fmt(y));
                self.mixed_runs(&segments, size, ("F1", self.face(false)), align);
                self.ops.push('\n');
            }
        } else if self.pdf {
            // The text matrix [0 1 -1 0 x y] turns the text a quarter turn.
            let shift = align * text_width(text, size);
            let _ = writeln!(
                self.ops,
                "BT /F1 {} Tf 0 1 -1 0 {} {} Tm {} Tj ET",
                fmt(size),
                fmt(x),
                fmt(y - shift),
                ps_string(&bytes)
            );
        } else {
            let _ = writeln!(
                self.ops,
                "/{} findfont {} scalefont setfont {} {} translate 90 rotate 0 0 moveto {} dup stringwidth pop \
                 {} mul neg 0 rmoveto show",
                self.face(false),
                fmt(size),
                fmt(x),
                fmt(y),
                ps_string(&bytes),
                fmt3(align)
            );
        }
        self.ops.push_str(if self.pdf { "Q\n" } else { "grestore\n" });
    }

    fn slanted_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: HAlign) {
        if !finite_at(at) {
            return;
        }
        let bytes = latin1_bytes(text);
        self.ops.push_str(if self.pdf { "q\n" } else { "gsave\n" });
        self.set_color(color, false);
        let (x, y) = (at[0], self.y(at[1]));
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let segments = font_segments(text);
        let symbols = segments.iter().any(|(symbol, _)| *symbol);
        if self.pdf {
            // The text matrix [c s −s c x y] turns the text 45°; the start
            // moves back along the slant by the aligned share of its width.
            let shift = align * text_width(text, size);
            let matrix = format!(
                "{} {} {} {} {} {} Tm",
                fmt3(k),
                fmt3(k),
                fmt3(-k),
                fmt3(k),
                fmt(x - shift * k),
                fmt(y - shift * k)
            );
            if symbols {
                let _ = write!(self.ops, "BT {matrix}");
                self.mixed_runs(&segments, size, ("F1", self.face(false)), 0.0);
                self.ops.push_str(" ET\n");
            } else {
                let _ = writeln!(self.ops, "BT /F1 {} Tf {matrix} {} Tj ET", fmt(size), ps_string(&bytes));
            }
        } else if symbols {
            let _ = write!(self.ops, "{} {} translate 45 rotate 0 0 moveto", fmt(x), fmt(y));
            self.mixed_runs(&segments, size, ("F1", self.face(false)), align);
            self.ops.push('\n');
        } else {
            let _ = writeln!(
                self.ops,
                "/{} findfont {} scalefont setfont {} {} translate 45 rotate 0 0 moveto {} dup stringwidth pop \
                 {} mul neg 0 rmoveto show",
                self.face(false),
                fmt(size),
                fmt(x),
                fmt(y),
                ps_string(&bytes),
                fmt3(align)
            );
        }
        self.ops.push_str(if self.pdf { "Q\n" } else { "grestore\n" });
    }

    fn clip_begin(&mut self, r: [f64; 4]) {
        let (x, y, w, h) = (r[0], self.y(r[3]), r[2] - r[0], r[3] - r[1]);
        if self.pdf {
            let _ = writeln!(self.ops, "q {} {} {} {} re W n", fmt(x), fmt(y), fmt(w), fmt(h));
        } else {
            let _ = writeln!(self.ops, "gsave {} {} {} {} rectclip", fmt(x), fmt(y), fmt(w), fmt(h));
        }
    }

    fn clip_end(&mut self) {
        self.ops.push_str(if self.pdf { "Q\n" } else { "grestore\n" });
    }

    fn image(&mut self, r: [f64; 4], bitmap: &crate::bitmap::Bitmap) {
        use crate::bitmap::{ascii85_encode, zlib_compress};
        if !bitmap.is_valid() || !r.iter().all(|v| v.is_finite()) || r[2] <= r[0] || r[3] <= r[1] {
            return;
        }
        let (x, y, w, h) = (r[0], self.y(r[3]), r[2] - r[0], r[3] - r[1]);
        let samples = self.samples(bitmap);
        let space = if self.cmyk { "/DeviceCMYK" } else { "/DeviceRGB" };
        if self.pdf {
            let alpha = bitmap.has_alpha().then(|| {
                let a: Vec<u8> = bitmap.rgba.chunks_exact(4).map(|p| p[3]).collect();
                ascii85_encode(&zlib_compress(&a))
            });
            let k = self.images.len();
            self.images.push(PdfImage {
                width: bitmap.width,
                height: bitmap.height,
                space,
                data: ascii85_encode(&zlib_compress(&samples)),
                alpha,
            });
            // The unit square of image space, scaled onto the rectangle; PDF
            // puts an image's first row at the top of that square.
            let _ = writeln!(self.ops, "q {} 0 0 {} {} {} cm /Im{k} Do Q", fmt(w), fmt(h), fmt(x), fmt(y));
        } else {
            let (cols, rows) = (bitmap.width, bitmap.height);
            let decode = if self.cmyk { "0 1 0 1 0 1 0 1" } else { "0 1 0 1 0 1" };
            let _ = writeln!(
                self.ops,
                "gsave {} {} translate {} {} scale {space} setcolorspace\n<< /ImageType 1 /Width {cols} /Height {rows} \
                 /BitsPerComponent 8 /Decode [{decode}] /Interpolate false /ImageMatrix [{cols} 0 0 -{rows} 0 {rows}] \
                 /DataSource currentfile /ASCII85Decode filter >> image",
                fmt(x),
                fmt(y),
                fmt(w),
                fmt(h)
            );
            self.ops.push_str(&ascii85_encode(&samples));
            self.ops.push_str("\ngrestore\n");
        }
    }

    fn finish(self: Box<Self>, width: f64, height: f64) -> Vec<u8> {
        let faces = [self.face(false), self.face(true)];
        if self.pdf {
            let mut ops = self.ops;
            if self.background {
                let white = if self.cmyk { "0 0 0 0 k" } else { "1 1 1 rg" };
                ops = format!("q {white} 0 0 {} {} re f Q\n{ops}", fmt(width), fmt(height));
            }
            pdf_document(&ops, &self.alphas, width, height, self.symbol, faces, &self.images)
        } else {
            eps_document(&self.ops, width, height, self.background.then_some(self.cmyk), faces)
        }
    }
}

/// A one-page PDF around a content stream, its text fonts `/F1` and `/F2`
/// being the standard fonts `faces` (regular and bold), and the image
/// XObjects it draws.
fn pdf_document(
    content: &str,
    alphas: &[u8],
    width: f64,
    height: f64,
    symbol: bool,
    faces: [&str; 2],
    images: &[PdfImage],
) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    // Images follow the fonts: object numbers from 7 (8 after the Symbol font).
    let mut next = 7 + usize::from(symbol);
    let mut image_objects = Vec::new();
    let mut xobjects = String::new();
    for (k, im) in images.iter().enumerate() {
        let (number, mask) = (next, im.alpha.as_ref().map(|_| next + 1));
        next += 1 + usize::from(mask.is_some());
        let _ = write!(xobjects, "/Im{k} {number} 0 R ");
        let smask = mask.map_or_else(String::new, |m| format!(" /SMask {m} 0 R"));
        image_objects.push(format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} /BitsPerComponent 8 \
             /Interpolate false /Filter [/ASCII85Decode /FlateDecode]{smask} /Length {} >>\nstream\n{}\nendstream",
            im.width,
            im.height,
            im.space,
            im.data.len(),
            im.data
        ));
        if let Some(alpha) = &im.alpha {
            image_objects.push(format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 \
                 /Filter [/ASCII85Decode /FlateDecode] /Length {} >>\nstream\n{alpha}\nendstream",
                im.width,
                im.height,
                alpha.len(),
            ));
        }
    }
    let xobjects = if xobjects.is_empty() { String::new() } else { format!("/XObject << {xobjects}>> ") };
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_owned());
    objects.push("<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned());
    let gs: String = alphas
        .iter()
        .enumerate()
        .map(|(k, a)| {
            let v = fmt3(f64::from(*a) / 255.0);
            format!("/GS{k} << /Type /ExtGState /ca {v} /CA {v} >> ")
        })
        .collect();
    objects.push(format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R \
         {}>> /ExtGState << {gs}>> {xobjects}>> >>",
        fmt(width),
        fmt(height),
        if symbol { "/F3 7 0 R " } else { "" }
    ));
    objects.push(format!("<< /Length {} >>\nstream\n{content}endstream", content.len()));
    for face in faces {
        objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{face} /Encoding /WinAnsiEncoding >>"));
    }
    if symbol {
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Symbol >>".to_owned());
    }
    objects.extend(image_objects);
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for off in offsets {
        let _ = writeln!(table, "{off:010} 00000 n ");
    }
    let _ = write!(table, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    out.extend_from_slice(table.as_bytes());
    out
}

/// An EPS file around PostScript drawing operators; `background` paints the
/// page white first (`Some(true)` in CMYK); `faces` are the text fonts
/// (regular and bold).
fn eps_document(content: &str, width: f64, height: f64, background: Option<bool>, faces: [&str; 2]) -> Vec<u8> {
    let white = match background {
        None => String::new(),
        Some(cmyk) => {
            let ink = if cmyk { "0 0 0 0 setcmykcolor" } else { "1 1 1 setrgbcolor" };
            format!("{ink} 0 0 {} {} rectfill\n", fmt(width), fmt(height))
        }
    };
    // The text fonts re-encoded to Latin-1, so accented letters print.
    let header = format!(
        "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 {} {}\n%%HiResBoundingBox: 0 0 {} {}\n%%Creator: AnovaCraft\n\
         %%LanguageLevel: 2\n%%Pages: 1\n%%EndComments\n\
         /reencode {{ findfont dup length dict begin {{ 1 index /FID ne {{ def }} {{ pop pop }} ifelse }} forall \
         /Encoding ISOLatin1Encoding def currentdict end definefont pop }} bind def\n\
         /{r} /{r} reencode /{b} /{b} reencode\n{white}",
        width.ceil(),
        height.ceil(),
        fmt(width),
        fmt(height),
        r = faces[0],
        b = faces[1],
    );
    let mut out = header.into_bytes();
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"showpage\n%%EOF\n");
    out
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// About `target` round tick values (steps of 1, 2, 2.5 or 5 × 10ⁿ, as on
/// screen) in `lo ..= hi`.
#[must_use]
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo {
        return Vec::new();
    }
    let step = nice_step(lo, hi, target);
    let mut out = Vec::new();
    let mut k = (lo / step).ceil();
    while k * step <= hi + 1e-9 * step && out.len() < 100 {
        out.push(k * step);
        k += 1.0;
    }
    out
}

/// The step of [`nice_ticks`]: the first of 1, 2, 2.5, 5, 10 × 10ⁿ at least
/// `(hi − lo) / target`.
fn nice_step(lo: f64, hi: f64, target: usize) -> f64 {
    let raw = (hi - lo) / target.max(1) as f64;
    let p = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].into_iter().map(|m| m * p).find(|s| *s >= raw).unwrap_or(10.0 * p)
}

/// [`nice_ticks`] with their labels: [`format_value`]'s compact form when it
/// reads back as the tick, else as many digits as the step needs (ticks
/// 2.5 apart from 1000 read 1002.5, not a rounded 1002).
#[must_use]
pub fn nice_tick_labels(lo: f64, hi: f64, target: usize) -> Vec<(f64, String)> {
    let ticks = nice_ticks(lo, hi, target);
    let step = nice_step(lo, hi, target);
    ticks.into_iter().map(|t| (t, tick_text(t, step))).collect()
}

/// `t` written precisely enough for ticks `step` apart.
fn tick_text(t: f64, step: f64) -> String {
    let compact = format_value(t);
    if compact.parse::<f64>().is_ok_and(|v| (v - t).abs() <= 1e-6 * step) {
        return compact;
    }
    // The decimal place of the step's last significant digit (2.5 × 10ⁿ has
    // one more than 1, 2 or 5 × 10ⁿ).
    let e = step.log10().floor();
    let mantissa = step / 10f64.powf(e);
    let last = e as i32 - i32::from((mantissa - 2.5).abs() < 1e-9);
    let a = t.abs();
    if (1e-4..1e7).contains(&a) {
        let decimals = (-last).clamp(0, 17) as usize;
        let s = format!("{t:.decimals$}");
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s };
        if s == "-0" { "0".to_owned() } else { s }
    } else {
        let digits = (a.log10().floor() as i32 - last).clamp(3, 16) as usize;
        format!("{t:.digits$e}")
    }
}

/// Ticks of an axis in drawn units within `lo..=hi`, with their labels.
fn axis_ticks(axis: &crate::frame::AxisSpec, lo: f64, hi: f64) -> Vec<(f64, String)> {
    axis_ticks_n(axis, lo, hi, 6)
}

/// Ticks of a broken axis at their broken positions within the shown range
/// `lo..=hi`: each piece between breaks gets nice ticks of its own on its
/// own scale (steps of 1, 2, 2.5 or 5 × 10ⁿ on a linear piece), about
/// `target` in all shared by length; ticks just above a cut that would
/// crowd the piece below are left out ([`crate::BrokenAxis::ticks`]).
#[must_use]
pub fn broken_axis_ticks(
    axis: &crate::frame::AxisSpec,
    lo: f64,
    hi: f64,
    brk: &crate::BrokenAxis,
    target: usize,
) -> Vec<(f64, String)> {
    brk.ticks(axis, lo, hi, target)
}

/// Ticks of `axis` within `lo..=hi` (drawn units) with their labels: about
/// `target` nice values on linear and date axes, level positions on a
/// categorical axis, nice data values on a transformed scale.
#[must_use]
pub fn axis_tick_labels(axis: &crate::frame::AxisSpec, lo: f64, hi: f64, target: usize) -> Vec<(f64, String)> {
    axis_ticks_n(axis, lo, hi, target)
}

/// [`axis_ticks`] with about `target` ticks on linear and date axes.
fn axis_ticks_n(axis: &crate::frame::AxisSpec, lo: f64, hi: f64, target: usize) -> Vec<(f64, String)> {
    if hi <= lo {
        return Vec::new();
    }
    if axis.is_categorical() {
        return (1..=axis.levels().len())
            .map(|k| k as f64)
            .filter(|t| *t >= lo - 1e-9 && *t <= hi + 1e-9)
            .map(|t| (t, tick_label(axis, t)))
            .collect();
    }
    if let Some(display) = axis.time {
        let step = crate::time_scale::time_step((hi - lo) / target.max(1) as f64, display.format);
        let ticks = crate::time_scale::time_ticks(lo, hi, step, &display);
        let labels = crate::time_scale::time_labels(&ticks, step, &display);
        return ticks.into_iter().zip(labels).collect();
    }
    if axis.scale == AxisScale::Linear {
        return nice_tick_labels(lo, hi, target);
    }
    scaled_ticks(axis.scale, lo, hi).into_iter().map(|t| (t, tick_label(axis, t))).collect()
}

const GRID: Rgba = Rgba([220, 222, 228, 255]);
const FRAME: Rgba = Rgba([140, 144, 156, 255]);
const INK: Rgba = Rgba([30, 32, 40, 255]);
const MUTED: Rgba = Rgba([90, 94, 106, 255]);

/// The mark where an axis is broken (a pair of slashes or a zigzag, as
/// `mark` says), across the spine through `at`: along a vertical spine
/// (`vertical`) or a horizontal one. The spine under the mark is painted
/// over in the page colour.
pub(crate) fn break_marks(c: &mut dyn Canvas, at: [f64; 2], vertical: bool, mark: crate::BreakMark) {
    let (half, gap, slant) = (4.5, 1.6, 1.8);
    c.fill(&mark.cover(at, vertical, half, gap, slant), Rgba::rgb(255, 255, 255));
    for stroke in mark.strokes(at, vertical, half, gap, slant) {
        c.polyline(&stroke, FRAME, 0.8, &[], false);
    }
}

/// Draw a marker centred at `c` (page points).
fn marker(canvas: &mut dyn Canvas, c: [f64; 2], r: f64, shape: MarkShape, color: Rgba, filled: bool) {
    let poly = |pts: Vec<[f64; 2]>| pts.into_iter().map(|p| [c[0] + p[0], c[1] + p[1]]).collect::<Vec<_>>();
    let outline: Option<Vec<[f64; 2]>> = match shape {
        MarkShape::Circle => Some(poly(
            (0..16)
                .map(|k| {
                    let a = std::f64::consts::PI * f64::from(k) / 8.0;
                    [r * a.cos(), r * a.sin()]
                })
                .collect(),
        )),
        MarkShape::Square => Some(poly(vec![[-r, -r], [r, -r], [r, r], [-r, r]])),
        MarkShape::Diamond => Some(poly(vec![[0.0, -r], [r, 0.0], [0.0, r], [-r, 0.0]])),
        MarkShape::Up => Some(poly(vec![[0.0, -r], [r, r], [-r, r]])),
        MarkShape::Down => Some(poly(vec![[-r, -r], [r, -r], [0.0, r]])),
        _ => None,
    };
    match outline {
        Some(o) if filled => canvas.fill(&o, color),
        Some(o) => canvas.polyline(&o, color, 1.2, &[], true),
        None => {
            let strokes: Vec<[[f64; 2]; 2]> = match shape {
                MarkShape::Plus => vec![[[-r, 0.0], [r, 0.0]], [[0.0, -r], [0.0, r]]],
                MarkShape::Cross => vec![[[-r, -r], [r, r]], [[-r, r], [r, -r]]],
                _ => vec![[[-r, 0.0], [r, 0.0]], [[-0.5 * r, -r], [0.5 * r, r]], [[-0.5 * r, r], [0.5 * r, -r]]],
            };
            for [a, b] in strokes {
                canvas.polyline(&[[c[0] + a[0], c[1] + a[1]], [c[0] + b[0], c[1] + b[1]]], color, 1.2, &[], false);
            }
        }
    }
}

fn dash(p: LinePattern) -> &'static [f64] {
    p.dashes()
}

/// The visible ranges of a panel, `([x0, x1], [y0, y1])` in drawn units,
/// which reference lines and bands ([`Mark::Rule`], [`Mark::Span`]) cross.
type View = ([f64; 2], [f64; 2]);

/// Draw one layer into a panel's plot rectangle showing `view`; with
/// `markup`, text labels are read as math markup.
fn draw_layer(canvas: &mut dyn Canvas, layer: &Layer, map: &dyn Fn([f64; 2]) -> [f64; 2], view: View, markup: bool) {
    let ([x0, x1], [y0, y1]) = view;
    for mark in &layer.marks {
        match mark {
            Mark::Rule { at, vertical, color, width, pattern, label, .. } => {
                let (a, b) = if *vertical { (map([*at, y0]), map([*at, y1])) } else { (map([x0, *at]), map([x1, *at])) };
                if !(a.iter().chain(&b).all(|v| v.is_finite())) {
                    continue;
                }
                if *width > 0.0 {
                    canvas.polyline(&[a, b], *color, f64::from(*width) * 0.8, dash(*pattern), false);
                }
                if !label.is_empty() {
                    let size = 7.0;
                    if *vertical {
                        // Top end, just right of the line.
                        let top = a[1].min(b[1]);
                        canvas.text([a[0] + 3.0, top + size + 2.0], label, size, *color, 0.0, false);
                    } else {
                        let right = a[0].max(b[0]);
                        canvas.text([right - 3.0, a[1] - 2.5], label, size, *color, 1.0, false);
                    }
                }
            }
            Mark::Span { from, to, vertical, color, .. } => {
                let ring = if *vertical {
                    [map([*from, y0]), map([*to, y0]), map([*to, y1]), map([*from, y1])]
                } else {
                    [map([x0, *from]), map([x1, *from]), map([x1, *to]), map([x0, *to])]
                };
                if ring.iter().flatten().all(|v| v.is_finite()) {
                    canvas.fill(&ring, *color);
                }
            }
            Mark::Points { points, color, shape, radius, filled, .. } => {
                for p in points {
                    marker(canvas, map(*p), f64::from(*radius) * 0.8, *shape, *color, *filled);
                }
            }
            Mark::Line { points, color, width, pattern, .. } => {
                let pts: Vec<[f64; 2]> = points.iter().map(|p| map(*p)).collect();
                canvas.polyline(&pts, *color, f64::from(*width) * 0.8, dash(*pattern), false);
            }
            Mark::Band { lower, upper, color, .. } => {
                let ring: Vec<[f64; 2]> = lower.iter().chain(upper.iter().rev()).map(|p| map(*p)).collect();
                canvas.fill(&ring, *color);
            }
            Mark::Polygon { points, fill, stroke, .. } => {
                let pts: Vec<[f64; 2]> = points.iter().map(|p| map(*p)).collect();
                canvas.fill(&pts, *fill);
                canvas.polyline(&pts, *stroke, 0.8, &[], true);
            }
            Mark::Area { rings, color, .. } => {
                let rings: Vec<Vec<[f64; 2]>> = rings.iter().map(|r| r.iter().map(|p| map(*p)).collect()).collect();
                canvas.fill_rings(&rings, *color);
            }
            Mark::Segments { segments, color, width, .. } => {
                for [a, b] in segments {
                    canvas.polyline(&[map(*a), map(*b)], *color, f64::from(*width) * 0.8, &[], false);
                }
            }
            Mark::Arrow { from, to, color, width } => {
                let (a, b) = (map(*from), map(*to));
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len > 1e-6 {
                    // The head: 7 points long (the screen's 9 at 0.8 scale).
                    let (ux, uy) = (dx / len, dy / len);
                    let head = 7.2_f64.min(len);
                    let base = [b[0] - ux * head, b[1] - uy * head];
                    let (nx, ny) = (-uy * head * 0.45, ux * head * 0.45);
                    canvas.polyline(&[a, base], *color, f64::from(*width) * 0.8, &[], false);
                    canvas.fill(&[b, [base[0] + nx, base[1] + ny], [base[0] - nx, base[1] - ny]], *color);
                }
            }
            Mark::Text { at, text, color, anchor, .. } => {
                let p = map(*at);
                let size = 7.5;
                let lines: Vec<&str> = text.lines().collect();
                let n = lines.len() as f64;
                // First baseline so the block sits as the anchor asks.
                let (align, first) = match anchor {
                    TextAnchor::RightOfText => (1.0, p[1] - 2.0 - (n - 1.0) * size * 1.2),
                    TextAnchor::LeftOfText => (0.0, p[1] + size * 0.35 - (n - 1.0) * size * 0.6),
                    TextAnchor::Centre => (0.5, p[1] + size * 0.35 - (n - 1.0) * size * 0.6),
                    TextAnchor::Above => (0.5, p[1] - 3.0 - (n - 1.0) * size * 1.2),
                };
                for (i, line) in lines.iter().enumerate() {
                    let at = [p[0], first + i as f64 * size * 1.2];
                    math_text(canvas, at, &MathText::new(line, markup), size, *color, align, false, false);
                }
            }
        }
    }
}

/// Width of the swatch column of a colour bar, in points.
const BAR_SWATCH: f64 = 10.0;

/// Room a colour bar takes at the right of a plot: gap, swatches, labels
/// and the vertical title.
fn color_bar_width(bar: &crate::contour::ColorBar) -> f64 {
    let labels = bar.labels.iter().map(|l| text_width(l, 7.0)).fold(0.0, f64::max);
    12.0 + BAR_SWATCH + 4.0 + labels + if bar.title.is_empty() { 4.0 } else { 16.0 }
}

/// A colour bar with its top-left corner at `(at[0], at[1])` reaching down
/// to `at[2]`: one equal box per band, low at the bottom, the level values
/// beside the boundaries between boxes, and the title reading upwards at
/// the right.
fn draw_color_bar(c: &mut dyn Canvas, bar: &crate::contour::ColorBar, at: [f64; 3]) {
    let n = bar.colors.len();
    if n == 0 {
        return;
    }
    let (x, top, bottom) = (at[0], at[1], at[2]);
    // Keep the boxes a readable size: at most 28 points each, centred.
    let h = ((bottom - top) / n as f64).min(28.0);
    let y0 = top + 0.5 * ((bottom - top) - h * n as f64);
    let box_top = |k: usize| y0 + (n - 1 - k) as f64 * h;
    for (k, &color) in bar.colors.iter().enumerate() {
        let y = box_top(k);
        c.fill(&[[x, y], [x + BAR_SWATCH, y], [x + BAR_SWATCH, y + h], [x, y + h]], color);
    }
    let frame = [[x, y0], [x + BAR_SWATCH, y0], [x + BAR_SWATCH, y0 + h * n as f64], [x, y0 + h * n as f64]];
    c.polyline(&frame, FRAME, 0.6, &[], true);
    let mut widest: f64 = 0.0;
    for (k, label) in bar.labels.iter().enumerate() {
        // Level k separates band k (below) from band k + 1 (above).
        let y = box_top(k);
        c.polyline(&[[x + BAR_SWATCH, y], [x + BAR_SWATCH + 2.5, y]], FRAME, 0.6, &[], false);
        c.text([x + BAR_SWATCH + 4.0, y + 2.5], label, 7.0, MUTED, 0.0, false);
        widest = widest.max(text_width(label, 7.0));
    }
    if !bar.title.is_empty() {
        let tx = x + BAR_SWATCH + 4.0 + widest + 11.0;
        c.vertical_text([tx, 0.5 * (top + bottom)], &bar.title, 8.0, INK, 0.5);
    }
}

/// A key entry: label, color, glyph.
type KeyItem = (String, Rgba, KeyGlyph);
/// A ramp in the key: title, low, high, reversed.
type RampItem = (String, f64, f64, bool);

/// Key entries for the page, plus ramps.
fn key_entries(frame: &Frame, layers: &[Vec<Layer>]) -> (Vec<KeyItem>, Vec<RampItem>) {
    let mut entries: Vec<(String, Rgba, KeyGlyph)> = Vec::new();
    let legend = &frame.legend;
    if let Some(ColorKey::Levels { title, entries: e }) = &legend.color {
        for (l, c, s) in e {
            entries.push((format!("{title} {l}"), *c, KeyGlyph::Marker(*s, true)));
        }
    }
    if let Some((title, e)) = &legend.shape {
        for (l, s) in e {
            entries.push((format!("{title} {l}"), INK, KeyGlyph::Marker(*s, false)));
        }
    }
    if let Some((title, groups)) = &legend.groups {
        for g in groups {
            entries.push((format!("{title} {}", g.label), g.color, KeyGlyph::Marker(g.shape, true)));
        }
    }
    for e in layers.iter().flatten().flat_map(|l| l.key.iter()) {
        if !entries.iter().any(|x| x.0 == e.label) {
            entries.push((e.label.clone(), e.color, e.glyph));
        }
    }
    let mut ramps: Vec<(String, f64, f64, bool)> = Vec::new();
    if let Some(ColorKey::Gradient { title, min, max }) = &legend.color {
        ramps.push((title.clone(), *min, *max, false));
    }
    for r in layers.iter().flatten().filter_map(|l| l.ramp.as_ref()) {
        match ramps.iter_mut().find(|x| x.0 == r.title) {
            Some(x) => {
                x.1 = x.1.min(r.min);
                x.2 = x.2.max(r.max);
            }
            None => ramps.push((r.title.clone(), r.min, r.max, r.reversed)),
        }
    }
    (entries, ramps)
}

/// Page settings of an exported figure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Figure<'a> {
    /// Title written above the panels (empty for none).
    pub title: &'a str,
    /// File format.
    pub format: VectorFormat,
    /// Page width in points (1/72 inch).
    pub width: f64,
    /// Page height in points.
    pub height: f64,
}

/// Which grid rules an exported graph draws at the major ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridRules {
    /// Vertical rules at the horizontal axis's ticks (numeric axes only).
    pub vertical: bool,
    /// Horizontal rules at the vertical axis's ticks.
    pub horizontal: bool,
}

impl Default for GridRules {
    /// The on-screen default: horizontal rules only.
    fn default() -> Self {
        Self { vertical: false, horizontal: true }
    }
}

/// Draw the whole graph with the default grid (horizontal rules only) and
/// return the file's bytes. `views` gives each panel's visible range (see
/// [`panel_views`]).
#[must_use]
pub fn export_graph(
    spec: &GraphSpec,
    frame: &Frame,
    layers: &[Vec<Layer>],
    views: &[PanelView],
    figure: Figure<'_>,
) -> Vec<u8> {
    export_graph_with_grid(spec, frame, layers, views, figure, GridRules::default())
}

/// [`export_graph`] with the grid rules the graph shows on screen.
#[must_use]
pub fn export_graph_with_grid(
    spec: &GraphSpec,
    frame: &Frame,
    layers: &[Vec<Layer>],
    views: &[PanelView],
    figure: Figure<'_>,
    grid: GridRules,
) -> Vec<u8> {
    export_graph_full(spec, frame, layers, views, figure, grid, None)
}

/// The right axis of an exported graph with the range it shows.
#[derive(Debug, Clone, Copy)]
pub struct RightView<'a> {
    /// What the right axis draws ([`GraphSpec::right_axis`]).
    pub axis: &'a crate::axes::RightAxis,
    /// Its visible range (drawn units).
    pub range: (f64, f64),
}

/// The whole graph as drawn on screen: grid rules, the right axis (when the
/// graph has one), the user's titles and axis breaks from
/// [`GraphSpec::axes`], with math markup set as on screen.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn export_graph_full(
    spec: &GraphSpec,
    frame: &Frame,
    layers: &[Vec<Layer>],
    views: &[PanelView],
    figure: Figure<'_>,
    grid: GridRules,
    right: Option<RightView<'_>>,
) -> Vec<u8> {
    let Figure { title, format, width, height } = figure;
    let (width, height) = (width.max(200.0), height.max(150.0));
    let mut canvas = new_canvas(format, width, height);
    let c = canvas.as_mut();
    let margin = 18.0;
    let mut top = margin;
    let axes = &spec.axes;
    let markup = axes.markup_labels;
    let title = axes.title_text(axes.title.as_deref(), title);
    if let Some(plain) = title.as_plain() {
        // Bold text runs about 8% wider than the regular-weight estimate.
        for line in wrap_text(plain, 13.0, (width - 2.0 * margin) / 1.08, 2) {
            c.text([width / 2.0, top + 12.0], &line, 13.0, INK, 0.5, true);
            top += 16.0;
        }
        if !plain.is_empty() {
            top += 6.0;
        }
    } else {
        let size = 13.0;
        let l = title.layout(size, &text_width);
        let baseline = top + l.ascent.max(size * 0.72);
        math_text(c, [width / 2.0, baseline], &title, size, INK, 0.5, true, false);
        top = baseline + l.descent.max(size * 0.25) + 6.0;
    }
    if let Some(page) = frame.page_title(&spec.encoding) {
        c.text([width / 2.0, top + 9.0], &page, 9.5, MUTED, 0.5, false);
        top += 14.0;
    }
    // Key size: wrap entries into rows.
    let (entries, ramps) = if spec.legend == crate::element::LegendPlacement::Hidden {
        (Vec::new(), Vec::new())
    } else {
        let mut all: Vec<Vec<Layer>> = layers.to_vec();
        if let Some(r) = right {
            all.extend(r.axis.layers.iter().flatten().cloned());
        }
        let (_, ramps) = key_entries(frame, &all);
        // The entries as the user edited them.
        let e: Vec<KeyItem> = spec
            .key_shown(frame, layers, right.map(|r| r.axis))
            .into_iter()
            .map(|(_, label, color, glyph)| (label, color, glyph))
            .collect();
        (e, ramps)
    };
    let key_texts: Vec<MathText> = entries.iter().map(|e| MathText::new(&e.0, markup)).collect();
    let key_font = 7.5;
    let mut rows: Vec<Vec<(f64, usize)>> = vec![Vec::new()];
    let mut x = margin;
    for (i, label) in key_texts.iter().enumerate() {
        let w = 22.0 + math_width(label, key_font) + 12.0;
        if x + w > width - margin && x > margin {
            rows.push(Vec::new());
            x = margin;
        }
        rows.last_mut().expect("a row").push((x, i));
        x += w;
    }
    let key_rows = rows.iter().filter(|r| !r.is_empty()).count();
    let key_height =
        key_rows as f64 * 12.0 + ramps.len() as f64 * 14.0 + if key_rows + ramps.len() > 0 { 8.0 } else { 0.0 };
    let bottom = height - margin - key_height;
    // Panel grid.
    let (n_rows, n_cols) = (frame.n_rows.max(1), frame.n_cols.max(1));
    let has_headers =
        frame.column_headers.iter().any(|h| !h.is_empty()) || frame.panels.iter().any(|p| p.title.is_some());
    let row_header_w = if frame.row_headers.iter().any(|h| !h.is_empty()) { 70.0 } else { 0.0 };
    // The graph's own axis settings (Plot settings), drawn as on screen.
    let look = axes.style.as_ref().map(|s| &s.axes);
    let (gap_x, gap_y) = look.map_or((0.0, 0.0), |a| {
        (crate::export_axes::label_gap(&a.x), crate::export_axes::label_gap(&a.y))
    });
    let (left_pad, bottom_pad, header_h) = (48.0 + gap_y, 34.0 + gap_x, if has_headers { 14.0 } else { 0.0 });
    // Room for the right axis's ticks and title.
    let right_pad = if right.is_some() { 44.0 } else { 0.0 };
    let cell_w = (width - 2.0 * margin - row_header_w) / n_cols as f64;
    let cell_h = (bottom - top) / n_rows as f64;
    let axes_hidden = spec.form.draws_own_axes();
    for (i, panel) in frame.panels.iter().enumerate() {
        let Some(&([x0, x1], [y0, y1])) = views.get(i) else { continue };
        let cx = margin + panel.grid_col as f64 * cell_w;
        let cy = top + panel.grid_row as f64 * cell_h;
        let plot = [cx + left_pad, cy + header_h + 6.0, cx + cell_w - 8.0 - right_pad, cy + cell_h - bottom_pad];
        if plot[2] <= plot[0] || plot[3] <= plot[1] || x1 <= x0 || y1 <= y0 {
            continue;
        }
        // Round forms keep one unit the same length across and up.
        let ([x0, x1], [y0, y1]) = if spec.form.equal_aspect() {
            equal_aspect_view(([x0, x1], [y0, y1]), plot[2] - plot[0], plot[3] - plot[1])
        } else {
            ([x0, x1], [y0, y1])
        };
        let map = |p: [f64; 2]| -> [f64; 2] {
            [
                plot[0] + (p[0] - x0) / (x1 - x0) * (plot[2] - plot[0]),
                plot[3] - (p[1] - y0) / (y1 - y0) * (plot[3] - plot[1]),
            ]
        };
        let (x_break, y_break) = spec.panel_breaks(panel);
        // Header.
        let header = panel.title.clone().or_else(|| {
            (panel.grid_row == 0).then(|| frame.column_headers.get(panel.grid_col).cloned().unwrap_or_default())
        });
        if let Some(h) = header.filter(|h| !h.is_empty() && *h != panel.x.name) {
            c.text([(plot[0] + plot[2]) / 2.0, cy + header_h], &h, 8.5, INK, 0.5, true);
        }
        if !axes_hidden {
            let rect = [[plot[0], plot[1]], [plot[2], plot[1]], [plot[2], plot[3]], [plot[0], plot[3]]];
            // Grid lines and ticks.
            let x_ticks = match &x_break {
                Some(b) => broken_axis_ticks(&panel.x, x0, x1, b, 6),
                None => axis_ticks(&panel.x, x0, x1),
            };
            let plain = |a: &crate::frame::AxisSpec, b: Option<&crate::BrokenAxis>| {
                b.is_none() && !a.is_categorical() && a.scale == AxisScale::Linear
            };
            let x_ticks = crate::export_axes::graph_ticks(
                look.map(|a| &a.x),
                panel.x.time,
                plain(&panel.x, x_break.as_ref()),
                x_ticks,
                (x0, x1),
                plot[2] - plot[0],
                false,
            );
            let minor_grid = |s: Option<&crate::axis_settings::AxisSettings>| {
                s.is_some_and(|s| s.grid == crate::axis_settings::GridLines::MajorAndMinor)
            };
            for t in x_ticks {
                let px = map([t.value, y0])[0];
                if grid.vertical && !panel.x.is_categorical() && (!t.minor || minor_grid(look.map(|a| &a.x))) {
                    let color = if t.minor { Rgba([236, 237, 241, 255]) } else { GRID };
                    c.polyline(&[[px, plot[1]], [px, plot[3]]], color, 0.5, &[], false);
                }
                match look {
                    Some(a) => crate::export_axes::mark(c, &a.x, px, plot, false, t.minor),
                    None => c.polyline(&[[px, plot[3]], [px, plot[3] + 3.0]], FRAME, 0.6, &[], false),
                }
                if let Some(label) = &t.label {
                    math_text(c, [px, plot[3] + gap_x + 11.0], label, 7.0, MUTED, 0.5, false, false);
                }
            }
            let parallel = matches!(spec.form, crate::forms::GraphForm::Parallel);
            let mut tick_width: f64 = 0.0;
            if !parallel {
                let y_ticks = match &y_break {
                    Some(b) => broken_axis_ticks(&panel.y, y0, y1, b, 6),
                    None => axis_ticks(&panel.y, y0, y1),
                };
                let y_ticks = crate::export_axes::graph_ticks(
                    look.map(|a| &a.y),
                    panel.y.time,
                    plain(&panel.y, y_break.as_ref()),
                    y_ticks,
                    (y0, y1),
                    plot[3] - plot[1],
                    true,
                );
                for t in y_ticks {
                    let py = map([x0, t.value])[1];
                    if grid.horizontal && (!t.minor || minor_grid(look.map(|a| &a.y))) {
                        let color = if t.minor { Rgba([236, 237, 241, 255]) } else { GRID };
                        c.polyline(&[[plot[0], py], [plot[2], py]], color, 0.5, &[], false);
                    }
                    // Graph exports keep the vertical axis on the left.
                    match look {
                        Some(a) => {
                            let left = crate::axis_settings::AxisSettings { side: crate::axis_settings::AxisSide::Standard, ..a.y.clone() };
                            crate::export_axes::mark(c, &left, py, plot, true, t.minor);
                        }
                        None => c.polyline(&[[plot[0] - 3.0, py], [plot[0], py]], FRAME, 0.6, &[], false),
                    }
                    if let Some(label) = &t.label {
                        tick_width = tick_width.max(math_width(label, 7.0));
                        math_text(c, [plot[0] - gap_y - 5.0, py + 2.5], label, 7.0, MUTED, 1.0, false, false);
                    }
                }
            }
            let (ox, oy) = look.map_or((0.0, 0.0), |a| {
                (crate::export_axes::offset(&a.x), crate::export_axes::offset(&a.y))
            });
            if ox > 0.0 || oy > 0.0 {
                // Axis lines moved away from the data area.
                c.polyline(&[[plot[0], plot[3] + ox], [plot[2], plot[3] + ox]], FRAME, 0.8, &[], false);
                c.polyline(&[[plot[0] - oy, plot[1]], [plot[0] - oy, plot[3]]], FRAME, 0.8, &[], false);
            } else {
                c.polyline(&rect, FRAME, 0.8, &[], true);
            }
            // Break marks on both spines of a broken axis.
            if let Some(b) = &x_break {
                for a in b.cuts().into_iter().filter(|a| *a > x0 && *a < x1) {
                    let px = map([a, y0])[0];
                    break_marks(c, [px, plot[3]], false, b.mark);
                    break_marks(c, [px, plot[1]], false, b.mark);
                }
            }
            if let Some(b) = &y_break {
                for a in b.cuts().into_iter().filter(|a| *a > y0 && *a < y1) {
                    let py = map([x0, a])[1];
                    break_marks(c, [plot[0], py], true, b.mark);
                    break_marks(c, [plot[2], py], true, b.mark);
                }
            }
            // The right axis: ticks and labels outside the right edge.
            if let Some(r) = right {
                let to_left = crate::axes::RightAxis::to_left((y0, y1), r.range);
                let mut right_width: f64 = 0.0;
                for (t, label) in axis_ticks(&r.axis.axis, r.range.0, r.range.1) {
                    right_width = right_width.max(text_width(&label, 7.0));
                    let py = map([x0, to_left(t)])[1];
                    c.polyline(&[[plot[2], py], [plot[2] + 3.0, py]], FRAME, 0.6, &[], false);
                    c.text([plot[2] + 5.0, py + 2.5], &label, 7.0, MUTED, 0.0, false);
                }
                let last_col =
                    !frame.panels.iter().any(|q| q.grid_row == panel.grid_row && q.grid_col > panel.grid_col);
                if last_col {
                    let title = axes.title_text(axes.y2_title.as_deref(), &r.axis.title);
                    y2_axis_title(c, &title, plot, right_width);
                }
            }
            // Axis titles on the outer panels.
            let bottom_row =
                !frame.panels.iter().any(|q| q.grid_row == panel.grid_row + 1 && q.grid_col == panel.grid_col);
            let x_title = axes.title_text(axes.x_title.as_deref(), &panel.x.title());
            if bottom_row && !x_title.is_empty() {
                let at = [(plot[0] + plot[2]) / 2.0, plot[3] + gap_x + 24.0];
                math_text(c, at, &x_title, 8.5, INK, 0.5, false, false);
            }
            let y_title = axes.title_text(axes.y_title.as_deref(), &spec.y_title(&panel.y));
            if panel.grid_col == 0 && !y_title.is_empty() && !parallel {
                y_axis_title(c, &y_title, plot, tick_width + gap_y, 4.0);
            }
        }
        if panel.grid_col + 1 == n_cols {
            if let Some(h) = frame.row_headers.get(panel.grid_row).filter(|h| !h.is_empty() && **h != panel.y.name) {
                c.text([plot[2] + 12.0 + right_pad, (plot[1] + plot[3]) / 2.0], h, 8.0, INK, 0.0, true);
            }
        }
        c.clip_begin(plot);
        let placed = crate::view::place_labels(
            layers.get(i).map_or(&[][..], Vec::as_slice),
            &spec.label_positions,
            i,
            ([x0, x1], [y0, y1]),
        );
        for layer in &placed {
            draw_layer(c, layer, &map, ([x0, x1], [y0, y1]), markup);
        }
        if let Some(r) = right {
            for layer in &r.axis.panel_layers(i, (y0, y1), r.range) {
                draw_layer(c, layer, &map, ([x0, x1], [y0, y1]), markup);
            }
        }
        // The user's annotations on top (their text is always markup).
        draw_layer(c, &spec.annotation_layer(i, panel, ([x0, x1], [y0, y1])), &map, ([x0, x1], [y0, y1]), true);
        c.clip_end();
    }
    // Key.
    let mut y = bottom + 14.0;
    for row in rows.iter().filter(|r| !r.is_empty()) {
        for &(x, i) in row {
            let (_, color, glyph) = &entries[i];
            let g = [x + 9.0, y - 3.0];
            match glyph {
                KeyGlyph::Line(p) => c.polyline(&[[x, g[1]], [x + 18.0, g[1]]], *color, 1.5, dash(*p), false),
                KeyGlyph::Area(fill) => {
                    let r =
                        [[x + 1.0, g[1] - 4.0], [x + 17.0, g[1] - 4.0], [x + 17.0, g[1] + 4.0], [x + 1.0, g[1] + 4.0]];
                    c.fill(&r, *fill);
                    c.polyline(&r, *color, 0.8, &[], true);
                }
                KeyGlyph::Marker(s, filled) => marker(c, g, 3.5, *s, *color, *filled),
            }
            math_text(c, [x + 22.0, y], &key_texts[i], key_font, INK, 0.0, false, false);
        }
        y += 12.0;
    }
    for (title, lo, hi, reversed) in &ramps {
        c.text([margin, y], &format!("{title}: {}", format_value(*lo)), key_font, INK, 0.0, false);
        let x0 = margin + text_width(&format!("{title}: {}", format_value(*lo)), key_font) + 6.0;
        let steps = 24;
        for k in 0..steps {
            let t = (k as f64 + 0.5) / steps as f64;
            let t = if *reversed { 1.0 - t } else { t };
            let xa = x0 + k as f64 * 5.0;
            c.fill(&[[xa, y - 7.0], [xa + 5.2, y - 7.0], [xa + 5.2, y], [xa, y]], crate::palette::ramp(t));
        }
        c.text([x0 + steps as f64 * 5.0 + 6.0, y], &format_value(*hi), key_font, INK, 0.0, false);
        y += 14.0;
    }
    canvas.finish(width, height)
}

/// One plot of a chart figure ([`export_chart`]): marks in data
/// coordinates with their axes. Used for report plots, which are not graph
/// canvases.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartPanel {
    /// Heading above the plot (empty for none).
    pub title: String,
    /// Horizontal axis title.
    pub x_title: String,
    /// Vertical axis title.
    pub y_title: String,
    /// Visible horizontal range (see [`ChartPanel::fit_ranges`]).
    pub x_range: (f64, f64),
    /// Visible vertical range.
    pub y_range: (f64, f64),
    /// Labelled ticks replacing the numeric horizontal axis.
    pub x_ticks: Option<Vec<(f64, String)>>,
    /// Labelled ticks replacing the numeric vertical axis.
    pub y_ticks: Option<Vec<(f64, String)>>,
    /// Date-time display of the horizontal axis (seconds since 1970).
    pub x_time: Option<stat_data::TemporalDisplay>,
    /// Date-time display of the vertical axis.
    pub y_time: Option<stat_data::TemporalDisplay>,
    /// Marks, back to front.
    pub marks: Vec<Mark>,
    /// Key entries.
    pub key: Vec<crate::layer::KeyEntry>,
    /// Draw the marks only, with no axes, ticks, grid or frame (for
    /// pictures already in page coordinates, such as a projected 3-D
    /// surface).
    pub bare: bool,
    /// Text placed in data coordinates, drawn over the marks.
    pub labels: Vec<Label>,
    /// How labelled horizontal ticks (categories) are turned.
    pub x_labels: crate::CategoryLabels,
    /// A colour-bar key drawn at the right of the plot (filled contour maps).
    pub color_bar: Option<crate::contour::ColorBar>,
    /// The plot's own axis settings (ticks, grid, tick marks, offsets,
    /// tick labels, sides), so the figure draws its axes as the screen
    /// does; `None` draws the standard axes.
    pub axes: Option<crate::axis_settings::PlotAxes>,
    /// Broken horizontal and vertical axes (`[x, y]`): the marks are
    /// already in broken drawn units; these give the ticks of each piece and
    /// the break marks ([`crate::axis_breaks`]).
    pub breaks: [Option<crate::BrokenAxis>; 2],
    /// A picture drawn behind the marks (an image plot's cells).
    pub image: Option<PanelImage>,
    /// The vertical axis runs downwards (image rows): marks, the picture and
    /// `y_range` are in negated units, so the tick labels show the values
    /// negated back.
    pub flip_y: bool,
}

/// A bitmap placed in a [`ChartPanel`]'s data coordinates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PanelImage {
    /// The cells, first row at the top.
    pub bitmap: crate::bitmap::Bitmap,
    /// Left and right edges.
    pub x: (f64, f64),
    /// Bottom and top edges.
    pub y: (f64, f64),
}

impl PanelImage {
    /// Draw the picture clipped as the canvas is, mapping data to the page
    /// with `map`.
    fn draw(&self, c: &mut dyn Canvas, map: &dyn Fn([f64; 2]) -> [f64; 2]) {
        let (a, b) = (map([self.x.0, self.y.1]), map([self.x.1, self.y.0]));
        c.image([a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])], &self.bitmap);
    }
}

/// Text at a point of a [`ChartPanel`], with its own size and alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// Anchor point (data coordinates).
    pub at: [f64; 2],
    /// Text (one line).
    pub text: String,
    /// Size in points.
    pub size: f64,
    /// Colour.
    pub color: Rgba,
    /// Horizontal alignment: 0 starts at the point, 0.5 centres, 1 ends there.
    pub align: f64,
    /// Vertical alignment: 0 hangs below the point, 0.5 centres, 1 sits on it.
    pub valign: f64,
    /// Bold weight.
    pub bold: bool,
}

impl ChartPanel {
    /// Set both ranges to fit every mark and labelled tick, with the
    /// default padding of [`crate::auto_range`].
    pub fn fit_ranges(&mut self) {
        let mut x = (f64::INFINITY, f64::NEG_INFINITY);
        let mut y = (f64::INFINITY, f64::NEG_INFINITY);
        let corners: Vec<[f64; 2]> =
            self.image.iter().flat_map(|i| [[i.x.0, i.y.0], [i.x.1, i.y.1]]).collect();
        let points = self.marks.iter().flat_map(Mark::coordinates).chain(corners);
        let ticks_x = self.x_ticks.iter().flatten().map(|t| [t.0, f64::NAN]);
        let ticks_y = self.y_ticks.iter().flatten().map(|t| [f64::NAN, t.0]);
        for [a, b] in points.chain(ticks_x).chain(ticks_y) {
            if a.is_finite() {
                x = (x.0.min(a), x.1.max(a));
            }
            if b.is_finite() {
                y = (y.0.min(b), y.1.max(b));
            }
        }
        self.x_range = crate::view::auto_range(x.0, x.1);
        self.y_range = crate::view::auto_range(y.0, y.1);
    }

    fn ticks(
        explicit: Option<&Vec<(f64, String)>>,
        time: Option<&stat_data::TemporalDisplay>,
        lo: f64,
        hi: f64,
    ) -> Vec<(f64, String)> {
        if let Some(t) = explicit {
            return t.iter().filter(|(v, _)| *v >= lo - 1e-9 && *v <= hi + 1e-9).cloned().collect();
        }
        if let Some(display) = time {
            let step = crate::time_scale::time_step((hi - lo) / 6.0, display.format);
            let ticks = crate::time_scale::time_ticks(lo, hi, step, display);
            let labels = crate::time_scale::time_labels(&ticks, step, display);
            return ticks.into_iter().zip(labels).collect();
        }
        nice_tick_labels(lo, hi, 6)
    }
}

/// Draw chart panels stacked top to bottom on one page, with a shared key
/// below, and return the file's bytes. Drawn like [`export_graph`]: light
/// grid, framed plots, Helvetica text.
#[must_use]
pub fn export_chart(panels: &[ChartPanel], figure: Figure<'_>) -> Vec<u8> {
    export_chart_grid(panels, 1, figure)
}

/// Draw chart panels in a grid of `columns` columns (filled row by row),
/// with a shared key below; otherwise as [`export_chart`].
#[must_use]
pub fn export_chart_grid(panels: &[ChartPanel], columns: usize, figure: Figure<'_>) -> Vec<u8> {
    let Figure { title, format, width, height } = figure;
    let (width, height) = (width.max(200.0), height.max(150.0));
    let mut canvas = new_canvas(format, width, height);
    let c = canvas.as_mut();
    let margin = 18.0;
    let mut top = margin;
    // Bold text runs about 8% wider than the regular-weight estimate.
    for line in wrap_text(title, 13.0, (width - 2.0 * margin) / 1.08, 2) {
        c.text([width / 2.0, top + 12.0], &line, 13.0, INK, 0.5, true);
        top += 16.0;
    }
    if !title.is_empty() {
        top += 6.0;
    }
    // Key entries, each label once, wrapped into rows.
    let mut entries: Vec<&crate::layer::KeyEntry> = Vec::new();
    for e in panels.iter().flat_map(|p| &p.key) {
        if !entries.iter().any(|x| x.label == e.label) {
            entries.push(e);
        }
    }
    let key_font = 7.5;
    let mut rows: Vec<Vec<(f64, usize)>> = vec![Vec::new()];
    let mut x = margin;
    for (i, e) in entries.iter().enumerate() {
        let w = 22.0 + text_width(&e.label, key_font) + 12.0;
        if x + w > width - margin && x > margin {
            rows.push(Vec::new());
            x = margin;
        }
        rows.last_mut().expect("a row").push((x, i));
        x += w;
    }
    let key_rows = rows.iter().filter(|r| !r.is_empty()).count();
    let bottom = height - margin - if key_rows > 0 { key_rows as f64 * 12.0 + 8.0 } else { 0.0 };
    let columns = columns.clamp(1, panels.len().max(1));
    let grid_rows = panels.len().div_ceil(columns).max(1);
    let cell_w = (width - 2.0 * margin) / columns as f64;
    let cell_h = (bottom - top) / grid_rows as f64;
    for (i, panel) in panels.iter().enumerate() {
        let flipped;
        let panel = if panel.flip_y && panel.y_ticks.is_none() {
            // Ticks of the values the downward axis shows, at their negated places.
            let (y0, y1) = panel.y_range;
            let ticks = nice_tick_labels(-y1, -y0, 6).into_iter().map(|(t, label)| (-t, label)).collect();
            flipped = ChartPanel { y_ticks: Some(ticks), ..panel.clone() };
            &flipped
        } else {
            panel
        };
        let cy = top + (i / columns) as f64 * cell_h;
        let cx = margin + (i % columns) as f64 * cell_w;
        let centre = cx + cell_w / 2.0;
        let title_lines = wrap_text(&panel.title, 8.5, cell_w / 1.08, 2);
        let header_h = title_lines.len() as f64 * 11.0 + if title_lines.is_empty() { 0.0 } else { 3.0 };
        let bar_w = panel.color_bar.as_ref().map_or(0.0, color_bar_width);
        let ((x0, x1), (y0, y1)) = (panel.x_range, panel.y_range);
        let x_ticks = match &panel.breaks[0] {
            Some(b) => b.ticks(&plain_axis(), x0, x1, 6),
            None => ChartPanel::ticks(panel.x_ticks.as_ref(), panel.x_time.as_ref(), x0, x1),
        };
        let (x_layout, strip) = x_label_layout(panel, &x_ticks, cell_w - 56.0 - bar_w);
        let look = panel.axes.as_ref().filter(|_| !panel.bare);
        let plot = if panel.bare {
            [cx + 4.0, cy + header_h + 4.0, cx + cell_w - 4.0 - bar_w, cy + cell_h - 4.0]
        } else if let Some(axes) = look {
            crate::export_axes::plot_rect(axes, [cx, cy, cx + cell_w, cy + cell_h], header_h, bar_w, strip)
        } else {
            [cx + 48.0, cy + header_h + 6.0, cx + cell_w - 8.0 - bar_w, cy + cell_h - strip - 23.0]
        };
        if plot[2] <= plot[0]
            || plot[3] <= plot[1]
            || x1.partial_cmp(&x0) != Some(std::cmp::Ordering::Greater)
            || y1.partial_cmp(&y0) != Some(std::cmp::Ordering::Greater)
        {
            continue;
        }
        let map = |p: [f64; 2]| -> [f64; 2] {
            [
                plot[0] + (p[0] - x0) / (x1 - x0) * (plot[2] - plot[0]),
                plot[3] - (p[1] - y0) / (y1 - y0) * (plot[3] - plot[1]),
            ]
        };
        for (k, line) in title_lines.iter().enumerate() {
            c.text([centre, cy + 11.0 * (k + 1) as f64], line, 8.5, INK, 0.5, true);
        }
        if panel.bare {
            c.clip_begin(plot);
            if let Some(image) = &panel.image {
                image.draw(c, &map);
            }
            let layer = Layer { marks: panel.marks.clone(), ..Layer::default() };
            draw_layer(c, &layer, &map, ([x0, x1], [y0, y1]), false);
            draw_labels(c, &panel.labels, &map);
            c.clip_end();
            continue;
        }
        if let Some(axes) = look {
            // The first column may use the page margin; others stop short of
            // the plot to their left.
            let left = if i % columns == 0 { 4.0 } else { cx - 4.0 };
            crate::export_axes::draw_chart_axes(c, panel, axes, plot, &x_ticks, x_layout, strip, &map, left);
        } else {
            for (k, (t, label)) in x_ticks.iter().enumerate() {
                let px = map([*t, y0])[0];
                c.polyline(&[[px, plot[3]], [px, plot[3] + 3.0]], FRAME, 0.6, &[], false);
                if k % x_layout.step != 0 {
                    continue;
                }
                // Turned labels end at their tick, centred on it across the text.
                let across = 0.35 * TICK_FONT;
                match x_layout.turn {
                    LabelTurn::Horizontal => c.text([px, plot[3] + 11.0], label, TICK_FONT, MUTED, 0.5, false),
                    LabelTurn::Slanted => {
                        let d = across * std::f64::consts::FRAC_1_SQRT_2;
                        c.slanted_text([px + d, plot[3] + 5.0 + d], label, TICK_FONT, MUTED, 1.0);
                    }
                    LabelTurn::Vertical => c.vertical_text([px + across, plot[3] + 5.0], label, TICK_FONT, MUTED, 1.0),
                }
            }
            let mut tick_width: f64 = 0.0;
            let y_ticks = match &panel.breaks[1] {
                Some(b) => b.ticks(&plain_axis(), y0, y1, 6),
                None => ChartPanel::ticks(panel.y_ticks.as_ref(), panel.y_time.as_ref(), y0, y1),
            };
            for (t, label) in y_ticks {
                tick_width = tick_width.max(text_width(&label, 7.0));
                let py = map([x0, t])[1];
                c.polyline(&[[plot[0], py], [plot[2], py]], GRID, 0.5, &[], false);
                c.polyline(&[[plot[0] - 3.0, py], [plot[0], py]], FRAME, 0.6, &[], false);
                c.text([plot[0] - 5.0, py + 2.5], &label, 7.0, MUTED, 1.0, false);
            }
            let rect = [[plot[0], plot[1]], [plot[2], plot[1]], [plot[2], plot[3]], [plot[0], plot[3]]];
            c.polyline(&rect, FRAME, 0.8, &[], true);
            if !panel.x_title.is_empty() {
                c.text([(plot[0] + plot[2]) / 2.0, plot[3] + strip + 13.0], &panel.x_title, 8.5, INK, 0.5, false);
            }
            if !panel.y_title.is_empty() {
                // The first column may use the page margin; others stop short of
                // the plot to their left.
                let left = if i % columns == 0 { 4.0 } else { cx - 4.0 };
                y_axis_title(c, &MathText::literal(&panel.y_title), plot, tick_width, left);
            }
        }
        chart_break_marks(c, panel, plot, &map);
        c.clip_begin(plot);
        if let Some(image) = &panel.image {
            image.draw(c, &map);
        }
        let layer = Layer { marks: panel.marks.clone(), ..Layer::default() };
        draw_layer(c, &layer, &map, ([x0, x1], [y0, y1]), false);
        draw_labels(c, &panel.labels, &map);
        c.clip_end();
        if let Some(bar) = &panel.color_bar {
            draw_color_bar(c, bar, [plot[2] + 12.0, plot[1], plot[3]]);
        }
    }
    let mut y = bottom + 14.0;
    for row in rows.iter().filter(|r| !r.is_empty()) {
        for &(x, i) in row {
            let e = entries[i];
            let g = [x + 9.0, y - 3.0];
            match e.glyph {
                KeyGlyph::Line(p) => c.polyline(&[[x, g[1]], [x + 18.0, g[1]]], e.color, 1.5, dash(p), false),
                KeyGlyph::Area(fill) => {
                    let r =
                        [[x + 1.0, g[1] - 4.0], [x + 17.0, g[1] - 4.0], [x + 17.0, g[1] + 4.0], [x + 1.0, g[1] + 4.0]];
                    c.fill(&r, fill);
                    c.polyline(&r, e.color, 0.8, &[], true);
                }
                KeyGlyph::Marker(s, filled) => marker(c, g, 3.5, s, e.color, filled),
            }
            c.text([x + 22.0, y], &e.label, key_font, INK, 0.0, false);
        }
        y += 12.0;
    }
    canvas.finish(width, height)
}

/// Size of tick labels on chart figures, in points.
const TICK_FONT: f64 = 7.0;

/// A plain continuous axis, for ticks of a chart panel's broken axis.
pub(crate) fn plain_axis() -> crate::frame::AxisSpec {
    crate::frame::AxisSpec {
        name: String::new(),
        kind: crate::frame::AxisKind::Continuous,
        scale: AxisScale::Linear,
        time: None,
    }
}

/// Break marks of a chart panel's broken axes on both spines of each.
fn chart_break_marks(c: &mut dyn Canvas, panel: &ChartPanel, plot: [f64; 4], map: &dyn Fn([f64; 2]) -> [f64; 2]) {
    let ((x0, x1), (y0, y1)) = (panel.x_range, panel.y_range);
    if let Some(b) = &panel.breaks[0] {
        for a in b.cuts().into_iter().filter(|a| *a > x0 && *a < x1) {
            let px = map([a, y0])[0];
            break_marks(c, [px, plot[3]], false, b.mark);
            break_marks(c, [px, plot[1]], false, b.mark);
        }
    }
    if let Some(b) = &panel.breaks[1] {
        for a in b.cuts().into_iter().filter(|a| *a > y0 && *a < y1) {
            let py = map([x0, a])[1];
            break_marks(c, [plot[0], py], true, b.mark);
            break_marks(c, [plot[2], py], true, b.mark);
        }
    }
}

/// How a chart panel's horizontal tick labels are set (turn and thinning,
/// see [`crate::category_labels`]) for a plot `plot_width` points wide, and
/// how far below the plot their strip reaches (11 points for level labels,
/// whose baseline sits there). Numeric and date axes stay level.
fn x_label_layout(panel: &ChartPanel, ticks: &[(f64, String)], plot_width: f64) -> (CategoryLayout, f64) {
    let level = CategoryLayout { turn: LabelTurn::Horizontal, step: 1 };
    let (x0, x1) = panel.x_range;
    if panel.x_ticks.is_none() || panel.bare || ticks.is_empty() || x1 <= x0 || plot_width <= 0.0 {
        return (level, 11.0);
    }
    let positions: Vec<f64> = ticks.iter().map(|t| t.0).collect();
    let spacing = crate::category_labels::least_spacing(&positions) * plot_width / (x1 - x0);
    let widest = ticks.iter().map(|t| text_width(&t.1, TICK_FONT)).fold(0.0, f64::max);
    let line = TICK_FONT * 1.15;
    let layout = crate::category_labels::category_layout(panel.x_labels, spacing, widest, line);
    let strip = match layout.turn {
        LabelTurn::Horizontal => 11.0,
        turn => 5.0 + crate::category_labels::label_depth(turn, widest, line),
    };
    (layout, strip)
}

/// The graph's underlying numbers as a table (header row first): for each
/// panel, horizontal position and subgroup, the number of rows and the
/// count-weighted mean, standard deviation, minimum, median and maximum of
/// the vertical values in data units (counts only without a vertical
/// column). Rows excluded or outside the filter are left out, as in the
/// graph's statistics. An estimation plot adds a header row and one row per
/// mean difference (difference, interval, level and method). Cells are plain
/// text, ready to paste as tab-separated values.
#[must_use]
pub fn summary_table(frame: &Frame) -> Vec<Vec<String>> {
    use crate::summary::{SummaryStat, summarize};
    let mut out = vec![
        ["Panel", "Horizontal", "Subgroup", "N", "Mean", "Std dev", "Minimum", "Median", "Maximum"]
            .map(String::from)
            .to_vec(),
    ];
    let mut estimation_rows: Vec<crate::estimation::EstimationGeometry> = Vec::new();
    for (i, panel) in frame.panels.iter().enumerate() {
        let name = panel_name(frame, i, panel);
        let mut slots: std::collections::BTreeMap<(i64, usize), (String, Vec<crate::summary::Obs>)> =
            std::collections::BTreeMap::new();
        for p in panel.points.iter().filter(|p| p.in_analysis()) {
            let position = if panel.x.is_categorical() { p.x.round() } else { p.x };
            let label = if panel.x.is_categorical() {
                tick_label(&panel.x, position)
            } else if let Some(display) = panel.x.time {
                display.text(position)
            } else {
                format_value(panel.x.scale.inverse(position))
            };
            let entry = slots
                .entry((crate::element::position_key(position, 1e9), p.group))
                .or_insert_with(|| (label, Vec::new()));
            entry.1.push((panel.y.scale.inverse(p.y), p.freq, p.weight));
        }
        if let crate::frame::PanelExtra::Estimation(g) = &panel.extra {
            if !g.lower_panel && !g.differences.is_empty() {
                estimation_rows.push(g.clone());
            }
        }
        for ((_, g), (label, obs)) in slots {
            let group = panel.groups.get(g).map(|s| s.label.clone()).unwrap_or_default();
            let n = summarize(&obs, SummaryStat::Count).map_or_else(String::new, format_value);
            let stat = |s: SummaryStat| {
                if panel.y.name.is_empty() {
                    String::new()
                } else {
                    summarize(&obs, s).map_or_else(String::new, format_value)
                }
            };
            out.push(vec![
                name.clone(),
                label,
                group,
                n,
                stat(SummaryStat::Mean),
                stat(SummaryStat::StdDev),
                stat(SummaryStat::Min),
                stat(SummaryStat::Median),
                stat(SummaryStat::Max),
            ]);
        }
    }
    // An estimation plot adds its mean differences after a header of their own.
    for g in estimation_rows {
        out.push(
            ["Comparison", "Mean difference", "Lower limit", "Upper limit", "Level", "Interval", "", "", ""]
                .map(String::from)
                .to_vec(),
        );
        let method = match g.interval {
            crate::estimation::EstimationInterval::Bca => format!("BCa bootstrap ({} resamples)", g.resamples),
            crate::estimation::EstimationInterval::Welch => "Welch t".to_owned(),
        };
        for d in &g.differences {
            out.push(vec![
                format!("{} − {}", g.groups[d.group].label, g.groups[g.reference].label),
                format_value(d.difference),
                format_value(d.lower),
                format_value(d.upper),
                format!("{}%", format_value(g.level * 100.0)),
                method.clone(),
                String::new(),
                String::new(),
                String::new(),
            ]);
        }
    }
    out
}

/// A name for panel `i`: its title or headers, or its axes.
fn panel_name(frame: &Frame, i: usize, panel: &Panel) -> String {
    if let Some(t) = &panel.title {
        return t.clone();
    }
    let col = frame.column_headers.get(panel.grid_col).filter(|h| !h.is_empty());
    let row = frame.row_headers.get(panel.grid_row).filter(|h| !h.is_empty());
    match (col, row) {
        (Some(c), Some(r)) => format!("{c} / {r}"),
        (Some(c), None) => c.clone(),
        (None, Some(r)) => r.clone(),
        (None, None) if frame.panels.len() > 1 => format!("Panel {}", i + 1),
        _ => format!("{} by {}", if panel.y.name.is_empty() { "Count" } else { &panel.y.name }, panel.x.name),
    }
}

/// `table` as tab-separated text (tabs and line breaks in cells become spaces).
#[must_use]
pub fn to_tsv(table: &[Vec<String>]) -> String {
    table
        .iter()
        .map(|row| row.iter().map(|c| c.replace(['\t', '\n', '\r'], " ")).collect::<Vec<_>>().join("\t"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_titles_wrap_then_shorten() {
        assert_eq!(wrap_text("Short", 8.5, 200.0, 2), ["Short"]);
        let long = "word ".repeat(60);
        let lines = wrap_text(&long, 8.5, 200.0, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'));
        assert!(lines.iter().all(|l| text_width(l, 8.5) <= 200.0));
    }
    use crate::encoding::EncodingPlan;
    use crate::{BarElement, FitLineElement};
    use stat_data::{Column, DataTable, RowStateManager};

    fn setup() -> (GraphSpec, Frame, Vec<Vec<Layer>>, Vec<PanelView>) {
        let table = DataTable::new(
            "t",
            vec![
                Column::numeric("x", (0..20).map(f64::from).collect()),
                Column::numeric("y", (0..20).map(|i| f64::from(i) * 1.5 + f64::from(i % 3)).collect()),
                Column::text("g", (0..20).map(|i| ["a", "b"][i % 2].to_owned()).collect()),
            ],
        )
        .unwrap();
        let state = RowStateManager::new(20);
        let spec = GraphSpec {
            encoding: EncodingPlan {
                x: vec!["x".into()],
                y: vec!["y".into()],
                color: Some("g".into()),
                ..Default::default()
            },
            fit: Some(FitLineElement::default()),
            ..GraphSpec::default()
        };
        let frame = spec.frame(&table, &state).unwrap();
        let layers = spec.layers(&frame, &state);
        let ranges = SharedRanges::of(&frame, &layers);
        let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
        (spec, frame, layers, views)
    }

    #[test]
    fn date_axes_use_calendar_ticks() {
        use stat_data::{TemporalDisplay, TemporalFormat};
        // Daily values through January 2026.
        let start = stat_data::temporal::seconds_from_civil(2026, 1, 1, 0, 0, 0.0);
        let days: Vec<f64> = (0..31).map(|d| start + f64::from(d) * 86_400.0).collect();
        let table = DataTable::new(
            "t",
            vec![
                Column::date_time("day", days, TemporalDisplay::new(TemporalFormat::Date)),
                Column::numeric("y", (0..31).map(f64::from).collect()),
            ],
        )
        .unwrap();
        let state = RowStateManager::new(31);
        let spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["day".into()], y: vec!["y".into()], ..Default::default() },
            options: crate::FrameOptions { x_scale: AxisScale::Log10, ..Default::default() },
            ..GraphSpec::default()
        };
        let frame = spec.frame(&table, &state).unwrap();
        let x = &frame.panels[0].x;
        assert_eq!(x.time.map(|t| t.format), Some(TemporalFormat::Date));
        assert_eq!(x.scale, AxisScale::Linear, "dates ignore a log scale");
        let layers = spec.layers(&frame, &state);
        let ranges = SharedRanges::of(&frame, &layers);
        let (lo, hi) = ranges.x["day"];
        assert!(lo > start - 2.0 * 86_400.0 && hi < start + 33.0 * 86_400.0, "no zero floor: {lo} {hi}");
        let ticks = axis_ticks(x, lo, hi);
        assert_eq!(ticks.first().map(|t| t.1.as_str()), Some("5 Jan 2026"), "{ticks:?}");
        assert!(ticks.iter().skip(1).all(|t| t.1.ends_with(" Jan")), "{ticks:?}");
        let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
        let svg = String::from_utf8(export_graph(
            &spec,
            &frame,
            &layers,
            &views,
            Figure { title: "y by day", format: VectorFormat::Svg, width: 500.0, height: 360.0 },
        ))
        .unwrap();
        assert!(svg.contains("12 Jan"), "date labels in the exported axis");
    }

    #[test]
    fn exported_grid_follows_the_on_screen_rules() {
        let (spec, frame, layers, views) = setup();
        let figure = Figure { title: "", format: VectorFormat::Svg, width: 500.0, height: 360.0 };
        let rules = |grid| {
            let svg = String::from_utf8(export_graph_with_grid(&spec, &frame, &layers, &views, figure, grid)).unwrap();
            svg.matches("stroke=\"#dcdee4\"").count()
        };
        let none = rules(GridRules { vertical: false, horizontal: false });
        let horizontal = rules(GridRules::default());
        let both = rules(GridRules { vertical: true, horizontal: true });
        assert_eq!(none, 0);
        assert!(horizontal > 0 && both > horizontal, "{horizontal} {both}");
        let default = String::from_utf8(export_graph(&spec, &frame, &layers, &views, figure)).unwrap();
        assert_eq!(default.matches("stroke=\"#dcdee4\"").count(), horizontal, "no vertical rules by default");
    }

    #[test]
    fn svg_is_well_formed() {
        let (spec, frame, layers, views) = setup();
        let svg = String::from_utf8(export_graph(
            &spec,
            &frame,
            &layers,
            &views,
            Figure { title: "y by x <test>", format: VectorFormat::Svg, width: 500.0, height: 360.0 },
        ))
        .unwrap();
        assert!(svg.starts_with("<?xml") && svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("y by x &lt;test&gt;"), "text is escaped");
        assert!(svg.contains("<clipPath") && svg.contains("<polygon") && svg.contains("<polyline"));
        // Balanced groups.
        assert_eq!(svg.matches("<g ").count(), svg.matches("</g>").count());
        assert!(!svg.contains("NaN") && !svg.contains("inf"));
    }

    #[test]
    fn moved_labels_are_exported_where_placed() {
        let (mut spec, frame, layers, views) = setup();
        let svg = |spec: &GraphSpec| {
            let figure = Figure { title: "", format: VectorFormat::Svg, width: 500.0, height: 360.0 };
            String::from_utf8(export_graph(spec, &frame, &layers, &views, figure)).unwrap()
        };
        // The text element holding the first fit's R² label.
        let label_line = |svg: &str| svg.lines().find(|l| l.contains("a: R²")).map(str::to_owned).unwrap();
        let before = label_line(&svg(&spec));
        spec.place_label(0, "fit/0", [0.0, 1.0]);
        let after = label_line(&svg(&spec));
        assert_ne!(before, after);
        // Near the panel's top left now: smaller x than at the line's right end.
        let x = |l: &str| -> f64 { l.split("x=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap() };
        assert!(x(&after) < x(&before), "{before} → {after}");
    }

    #[test]
    fn pdf_has_a_valid_cross_reference_table() {
        let (spec, frame, layers, views) = setup();
        let pdf = export_graph(
            &spec,
            &frame,
            &layers,
            &views,
            Figure { title: "R² ≤ 1", format: VectorFormat::Pdf, width: 500.0, height: 360.0 },
        );
        assert!(pdf.starts_with(b"%PDF-1.4"));
        // One character per byte, so byte offsets index the text.
        let text: String = pdf.iter().map(|&b| if b < 128 { b as char } else { '?' }).collect();
        assert!(text.trim_end().ends_with("%%EOF"));
        // Each xref offset points at "k 0 obj".
        let xref_at: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        let table = &text[xref_at..];
        let entries: Vec<usize> = table.lines().skip(3).take(6).map(|l| l[..10].parse().unwrap()).collect();
        for (k, off) in entries.iter().enumerate() {
            assert!(text[*off..].starts_with(&format!("{} 0 obj", k + 1)), "object {}", k + 1);
        }
        // Stream length matches.
        let start = text.find("stream\n").unwrap() + 7;
        let end = text.find("endstream").unwrap();
        let declared: usize =
            text[..start].rsplit("/Length ").next().unwrap().split_whitespace().next().unwrap().parse().unwrap();
        assert_eq!(end - start, declared);
        assert!(text.contains("/ExtGState") && text.contains("(R\\262 <= 1)"), "Latin-1 superscript and spelled ≤");
    }

    #[test]
    fn missing_coordinates_are_never_written() {
        // A report plot whose marks carry a missing value: vector files must
        // not contain `NaN` (invalid in SVG and PDF, an error in PostScript).
        let mark_line = Mark::Line {
            points: vec![[0.0, 1.0], [f64::NAN, 2.0], [2.0, 3.0]],
            color: Rgba::rgb(0, 0, 0),
            width: 1.0,
            pattern: LinePattern::Solid,
            name: String::new(),
        };
        let mark_points = Mark::Points {
            points: vec![[1.0, f64::NAN], [1.0, 1.0]],
            color: Rgba::rgb(0, 0, 0),
            shape: MarkShape::Circle,
            radius: 3.0,
            filled: true,
            name: String::new(),
        };
        let mark_text = Mark::Text {
            at: [f64::INFINITY, 1.0],
            text: "far".to_owned(),
            color: Rgba::rgb(0, 0, 0),
            anchor: TextAnchor::Centre,
            movable: None,
        };
        let mut panel = ChartPanel { marks: vec![mark_line, mark_points, mark_text], ..ChartPanel::default() };
        panel.fit_ranges();
        for format in [VectorFormat::Svg, VectorFormat::Pdf, VectorFormat::Eps] {
            let bytes = export_chart(&[panel.clone()], Figure { title: "t", format, width: 400.0, height: 300.0 });
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("NaN") && !text.contains("inf"), "{format:?}");
            // The finite vertices are still drawn.
            let drawn = match format {
                VectorFormat::Svg => "<polyline",
                VectorFormat::Pdf => " l\n",
                _ => " lineto\n",
            };
            assert!(text.contains(drawn), "{format:?}");
        }
        assert!(
            !export_chart(&[panel], Figure { title: "", format: VectorFormat::Png, width: 400.0, height: 300.0 })
                .is_empty()
        );
    }

    #[test]
    fn eps_has_bounding_box_and_no_transparency() {
        let (spec, frame, layers, views) = setup();
        let eps = String::from_utf8(export_graph(
            &spec,
            &frame,
            &layers,
            &views,
            Figure { title: "", format: VectorFormat::Eps, width: 400.0, height: 300.0 },
        ))
        .unwrap();
        assert!(eps.starts_with("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 400 300"));
        assert!(eps.trim_end().ends_with("%%EOF") && eps.contains("rectclip") && !eps.contains(" gs\n"));
        assert_eq!(eps.matches("gsave").count(), eps.matches("grestore").count());
    }

    #[test]
    fn summary_tables_and_ticks() {
        let (_, frame, _, _) = setup();
        let t = summary_table(&frame);
        assert_eq!(t[0][3], "N");
        assert_eq!(t.len(), 21, "one row per distinct x and subgroup");
        assert!(t[1..].iter().all(|r| r[3] == "1"));
        let tsv = to_tsv(&t);
        assert_eq!(tsv.lines().count(), 21);
        assert!(tsv.lines().next().unwrap().split('\t').count() == 9);
        assert_eq!(nice_ticks(0.0, 10.0, 5), [0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        assert!(nice_ticks(1.0, 1.0, 5).is_empty());
        // A raw step of 2.2 rounds to 2.5 (the on-screen ladder), not 5.
        assert_eq!(nice_ticks(0.0, 11.0, 5), [0.0, 2.5, 5.0, 7.5, 10.0]);
        // Labels carry the digits the step needs: ticks 2.5 apart from 1000
        // read 1002.5, not a rounded 1002 (four significant digits).
        let labels: Vec<String> = nice_tick_labels(1000.0, 1015.0, 6).into_iter().map(|(_, l)| l).collect();
        assert_eq!(labels, ["1000", "1002.5", "1005", "1007.5", "1010", "1012.5", "1015"]);
        for (lo, hi) in [(1e7, 1.015e7), (100.0, 100.15), (-1015.0, -1000.0), (0.0, 1.5e-3)] {
            for (t, label) in nice_tick_labels(lo, hi, 6) {
                let back: f64 = label.parse().unwrap();
                assert!((back - t).abs() <= 1e-9 * t.abs().max(1e-300), "{t} labelled {label}");
            }
        }
        assert_eq!(
            nice_tick_labels(0.0, 1.0, 5).into_iter().map(|(_, l)| l).collect::<Vec<_>>(),
            ["0", "0.2", "0.4", "0.6", "0.8", "1"]
        );
        // Bars over categories: one row per category.
        let table = DataTable::new("t", vec![Column::text("g", ["a", "b", "b"].map(String::from).to_vec())]).unwrap();
        let spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["g".into()], ..Default::default() },
            bars: Some(BarElement::default()),
            ..GraphSpec::default()
        };
        let f = spec.frame(&table, &RowStateManager::new(3)).unwrap();
        let t = summary_table(&f);
        assert_eq!(
            t[1..].iter().map(|r| (r[1].as_str(), r[3].as_str(), r[4].as_str())).collect::<Vec<_>>(),
            [("a", "1", ""), ("b", "2", "")]
        );
    }

    fn figure(format: VectorFormat) -> Figure<'static> {
        Figure { title: "y by x", format, width: 500.0, height: 360.0 }
    }

    fn text_of(bytes: Vec<u8>) -> String {
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// A bar-like chart panel with `n` categories named `name k`.
    fn categories(n: usize, name: &str, x_labels: crate::CategoryLabels) -> ChartPanel {
        let ticks: Vec<(f64, String)> = (1..=n).map(|k| (k as f64, format!("{name} {k}"))).collect();
        let points = (1..=n).map(|k| [k as f64, k as f64]).collect();
        let marks = vec![Mark::Points {
            points,
            color: INK,
            shape: MarkShape::Circle,
            radius: 2.0,
            filled: true,
            name: String::new(),
        }];
        let mut panel =
            ChartPanel { x_ticks: Some(ticks), marks, x_labels, x_title: "Term".into(), ..ChartPanel::default() };
        panel.fit_ranges();
        panel
    }

    /// Crowded category labels are slanted or stood upright in every format
    /// (Automatic), each one written; a few short ones stay level.
    #[test]
    fn crowded_category_labels_turn_in_every_format() {
        let figure = |format| Figure { title: "", format, width: 400.0, height: 300.0 };
        let level =
            text_of(export_chart(&[categories(4, "a", crate::CategoryLabels::Automatic)], figure(VectorFormat::Svg)));
        assert!(!level.contains("rotate(-45)") && level.contains(">a 4</text>"));

        // 40 categories across a 600-point page: upright labels, every one written.
        let crowded = categories(40, "category", crate::CategoryLabels::Automatic);
        let wide = |format| Figure { title: "", format, width: 600.0, height: 300.0 };
        let svg = text_of(export_chart(std::slice::from_ref(&crowded), wide(VectorFormat::Svg)));
        for k in 1..=40 {
            let line = svg.lines().find(|l| l.ends_with(&format!(">category {k}</text>"))).expect("every label");
            assert!(line.contains("rotate(-90)"), "{line}");
        }
        let eps = text_of(export_chart(std::slice::from_ref(&crowded), wide(VectorFormat::Eps)));
        assert!(eps.contains("(category 40)") && eps.contains("90 rotate"));
        // On a 400-point page even upright labels would overlap: every other one.
        let narrow = text_of(export_chart(std::slice::from_ref(&crowded), figure(VectorFormat::Svg)));
        assert_eq!(narrow.matches("rotate(-90)\" font-size=\"7\"").count(), 20);
        // Twelve fit slanted.
        let slanted = categories(12, "cat", crate::CategoryLabels::Automatic);
        let svg = text_of(export_chart(std::slice::from_ref(&slanted), figure(VectorFormat::Svg)));
        assert_eq!(svg.matches("rotate(-45)").count(), 12, "{svg}");
        let pdf = text_of(export_chart(std::slice::from_ref(&slanted), figure(VectorFormat::Pdf)));
        assert_eq!(pdf.matches("0.707 0.707 -0.707 0.707").count(), 12);
        let eps = text_of(export_chart(std::slice::from_ref(&slanted), figure(VectorFormat::Eps)));
        assert_eq!(eps.matches("45 rotate").count(), 12);
        // The PNG draws the slanted glyphs: ink below the plot, left of the last tick.
        let png = export_chart(std::slice::from_ref(&slanted), figure(VectorFormat::Png));
        assert!(png.starts_with(b"\x89PNG"));
        // Horizontal keeps them level.
        let fixed = categories(12, "cat", crate::CategoryLabels::Horizontal);
        assert!(!text_of(export_chart(&[fixed], figure(VectorFormat::Svg))).contains("rotate(-45)"));
    }

    /// Slanted text in the PNG is drawn from the glyph outlines at 45°: its
    /// ink spans about as far across as down.
    #[test]
    fn slanted_png_text_is_drawn_at_45_degrees() {
        let mut r = Raster::new(200.0, 200.0);
        r.turned_text([100.0, 100.0], "slanted text here", 12.0, INK, 1.0, -45.0);
        let (w, h, rgba) = r.into_rgba();
        let (mut x0, mut x1, mut y0, mut y1) = (u32::MAX, 0, u32::MAX, 0);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                if rgba[i] < 128 {
                    (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
                }
            }
        }
        assert!(x1 > x0 && y1 > y0, "ink drawn");
        let (dx, dy) = (f64::from(x1 - x0), f64::from(y1 - y0));
        assert!((dx / dy - 1.0).abs() < 0.25, "{dx} × {dy}");
        // It ends at the anchor (centre of the page) and runs down and left of it.
        let centre = f64::from(w) / 2.0;
        assert!(f64::from(x1) < centre + 0.1 * dx && f64::from(y0) > centre - 0.2 * dy, "{x0}..{x1} × {y0}..{y1}");
    }

    /// Typed titles in markup are set run by run: a subscript is its own,
    /// smaller text; Greek letters use the Symbol font in PDF and EPS; a
    /// fraction draws a rule. Plain titles stay one text element.
    #[test]
    fn markup_titles_in_every_format() {
        let (mut spec, frame, layers, views) = setup();
        spec.axes.x_title = Some("Dose x_1 (\\mu g)".into());
        spec.axes.y_title = Some("$dy/dx$".into());
        spec.axes.title = Some("Growth \\alpha^2".into());
        let svg = text_of(export_graph(&spec, &frame, &layers, &views, figure(VectorFormat::Svg)));
        assert!(svg.contains(">Dose x</text>") && svg.contains(">1</text>"), "runs: {svg}");
        assert!(svg.contains("font-size=\"5.95\""), "the subscript is 70% of 8.5 points");
        assert!(svg.contains(" (μ g)</text>") && svg.contains(">Growth α</text>"), "{svg}");
        assert!(svg.contains("rotate(-90)\" font-size=\"6.8\""), "the fraction on the vertical axis is 80% size");
        let pdf = text_of(export_graph(&spec, &frame, &layers, &views, figure(VectorFormat::Pdf)));
        assert!(pdf.contains("/BaseFont /Symbol") && pdf.contains("/F3 7 0 R"));
        assert!(pdf.contains("/F3 8.5 Tf (m) Tj"), "μ is m in the Symbol font");
        let eps = text_of(export_graph(&spec, &frame, &layers, &views, figure(VectorFormat::Eps)));
        assert!(eps.contains("/Symbol findfont"));
        let png = export_graph(&spec, &frame, &layers, &views, figure(VectorFormat::Png));
        assert!(png.starts_with(b"\x89PNG"));
        // Without typed titles nothing is read as markup and no Symbol font is listed.
        let (spec, ..) = setup();
        let pdf = text_of(export_graph(&spec, &frame, &layers, &views, figure(VectorFormat::Pdf)));
        assert!(!pdf.contains("Symbol"));
        // Key entries are read as markup only when asked.
        let (mut spec, frame, layers, views) = setup();
        spec.axes.markup_labels = true;
        let svg = text_of(export_graph(&spec, &frame, &layers, &views, figure(VectorFormat::Svg)));
        assert!(svg.contains(">g a</text>"));
    }

    /// A broken axis: tick labels from both sides of the break, the upper
    /// data moved down, break marks on both spines.
    #[test]
    fn broken_axes_are_exported() {
        let table = DataTable::new(
            "t",
            vec![Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0]), Column::numeric("y", vec![1.0, 2.0, 3.0, 1000.0])],
        )
        .unwrap();
        let state = RowStateManager::new(4);
        let mut spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() },
            ..GraphSpec::default()
        };
        spec.axes.y_breaks = crate::AxisBreaks::single(4.0, 995.0);
        let frame = spec.frame(&table, &state).unwrap();
        let layers = spec.layers(&frame, &state);
        let ranges = SharedRanges::of(&frame, &layers);
        let (lo, hi) = ranges.y["y"];
        assert!(hi < 20.0, "the range covers the broken data: {lo}..{hi}");
        let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
        let svg = text_of(export_graph(&spec, &frame, &layers, &views, figure(VectorFormat::Svg)));
        assert!(svg.contains(">1000</text>") && svg.contains(">2</text>"), "{svg}");
        // White parallelograms over the spine: two per break.
        assert_eq!(svg.matches("fill=\"#ffffff\" fill-opacity=\"1\"").count(), 2);
        let ticks = broken_axis_ticks(&frame.panels[0].y, lo, hi, &crate::BrokenAxis::gap(AxisScale::Linear, (4.0, 995.0)), 6);
        let labels: Vec<&str> = ticks.iter().map(|t| t.1.as_str()).collect();
        assert!(labels.contains(&"1000") && labels.contains(&"2"), "{labels:?}");
        assert!(ticks.windows(2).all(|w| w[0].0 < w[1].0), "ascending positions");
        assert!(ticks.iter().all(|(t, _)| *t >= lo - 1e-9 && *t <= hi + 1e-9));
        // Two breaks marked with zigzags: two covers per break, and each
        // zigzag one stroke of five vertices.
        spec.axes.y_breaks = crate::AxisBreaks {
            breaks: vec![
                crate::axes::AxisBreak { from: 3.5, to: 500.0 },
                crate::axes::AxisBreak { from: 520.0, to: 995.0 },
            ],
            mark: crate::BreakMark::Zigzag,
            ..crate::AxisBreaks::default()
        };
        let layers = spec.layers(&frame, &state);
        let ranges = SharedRanges::of(&frame, &layers);
        let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
        let svg = text_of(export_graph(&spec, &frame, &layers, &views, figure(VectorFormat::Svg)));
        assert_eq!(svg.matches("fill=\"#ffffff\" fill-opacity=\"1\"").count(), 4, "{svg}");
        assert!(svg.contains(">1000</text>"));
        for format in VectorFormat::ALL {
            assert!(!export_graph(&spec, &frame, &layers, &views, figure(format)).is_empty());
        }
    }

    /// A report-style chart panel with a broken vertical axis: ticks from
    /// both pieces and break marks on both spines.
    #[test]
    fn chart_panels_draw_broken_axes() {
        let b = crate::AxisBreaks::single(10.0, 900.0).map(AxisScale::Linear, None).unwrap();
        let points: Vec<[f64; 2]> = [[1.0, 2.0], [2.0, 5.0], [3.0, b.map(1000.0)]].to_vec();
        let mut panel = ChartPanel {
            marks: vec![Mark::Points {
                points,
                color: crate::palette::DEFAULT_POINT,
                shape: MarkShape::Circle,
                radius: 3.0,
                filled: true,
                name: String::new(),
            }],
            breaks: [None, Some(b)],
            ..ChartPanel::default()
        };
        panel.fit_ranges();
        for axes in [None, Some(crate::axis_settings::PlotAxes::default())] {
            panel.axes = axes;
            let fig = Figure { title: "t", format: VectorFormat::Svg, width: 400.0, height: 300.0 };
            let svg = text_of(export_chart(std::slice::from_ref(&panel), fig));
            assert!(svg.contains(">1000</text>") && svg.contains(">5</text>"), "{svg}");
            assert_eq!(svg.matches("fill=\"#ffffff\" fill-opacity=\"1\"").count(), 2);
        }
    }

    /// The right axis: its own ticks and title on the right, its series
    /// keyed "(right axis)" with their own marker, drawn mapped into the
    /// panel.
    #[test]
    fn right_axis_is_exported() {
        let table = DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("y", vec![1.0, 2.0, 3.0, 4.0]),
                Column::numeric("z", vec![1000.0, 3000.0, 2000.0, 4000.0]),
            ],
        )
        .unwrap();
        let state = RowStateManager::new(4);
        let spec = GraphSpec {
            encoding: EncodingPlan {
                x: vec!["x".into()],
                y: vec!["y".into()],
                y2: vec!["z".into()],
                ..Default::default()
            },
            ..GraphSpec::default()
        };
        let frame = spec.frame(&table, &state).unwrap();
        let layers = spec.layers(&frame, &state);
        let ranges = SharedRanges::of(&frame, &layers);
        let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
        let right = spec.right_axis(&table, &state).unwrap().unwrap();
        let view = RightView { axis: &right, range: right.range };
        for format in VectorFormat::ALL {
            let bytes =
                export_graph_full(&spec, &frame, &layers, &views, figure(format), GridRules::default(), Some(view));
            assert!(!bytes.is_empty());
            if format == VectorFormat::Svg {
                let svg = text_of(bytes);
                assert!(svg.contains(">z (right axis)</text>") && svg.contains(">y (left axis)</text>"));
                assert!(svg.contains(">4000</text>"), "right-axis tick labels");
                assert!(svg.contains(
                    "rotate(-90)\" font-size=\"8.5\" fill=\"#1e2028\" fill-opacity=\"1\" text-anchor=\"middle\">z<"
                ));
            }
        }
    }

    /// The summary table kept one row for every position from 9.22·10⁹ up
    /// (its slot key saturated).
    #[test]
    fn summary_table_keeps_large_positions_apart() {
        let table = DataTable::new(
            "t",
            vec![Column::numeric("x", vec![1e10, 2e10, 3e10, 3e10]), Column::numeric("y", vec![1.0, 2.0, 3.0, 5.0])],
        )
        .unwrap();
        let state = RowStateManager::new(4);
        let spec = GraphSpec {
            encoding: EncodingPlan { x: vec!["x".into()], y: vec!["y".into()], ..Default::default() },
            ..GraphSpec::default()
        };
        let frame = spec.frame(&table, &state).unwrap();
        let rows = summary_table(&frame);
        let data: Vec<(&str, &str)> = rows[1..].iter().map(|r| (r[3].as_str(), r[4].as_str())).collect();
        assert_eq!(data, [("1", "1"), ("1", "2"), ("2", "4")], "{rows:?}");
    }
}
