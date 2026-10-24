//! PNG output: the same drawing calls as the SVG/PDF/EPS writers, rendered
//! into pixels with `tiny-skia` (anti-aliased paths, dashes, clipping) and
//! text drawn with `ab_glyph` from the bundled Liberation Sans or Liberation
//! Serif (metric-compatible with the Helvetica and Times that PDF and EPS
//! figures name; DejaVu Sans for glyphs they lack). Coordinates are in points; the image has
//! [`PNG_DPI`] pixels per inch and records that density in the file.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform};

use crate::layer::Rgba;
use crate::plot_style::PlotFont;

/// Pixels per inch of PNG figures.
pub const PNG_DPI: f64 = 300.0;

/// Liberation Sans 2.1 (SIL Open Font License 1.1).
pub static LIBERATION_SANS: &[u8] = include_bytes!("../../app-gui/assets/fonts/LiberationSans-Regular.ttf");
/// Liberation Serif 2.1 (SIL Open Font License 1.1).
pub static LIBERATION_SERIF: &[u8] = include_bytes!("../../app-gui/assets/fonts/LiberationSerif-Regular.ttf");
static DEJAVU: &[u8] = include_bytes!("../../app-gui/assets/fonts/DejaVuSans.ttf");

/// A figure being drawn into pixels.
pub(crate) struct Raster {
    pixmap: Pixmap,
    /// Pixels per point.
    scale: f32,
    /// Clip regions in force (pixel rectangles, innermost last) and the mask
    /// of their intersection.
    clips: Vec<([f32; 4], Mask)>,
    fonts: [FontRef<'static>; 2],
    /// Pixels per inch, recorded in the file.
    dpi: f64,
}

impl Raster {
    /// A white page of `width` × `height` points at [`PNG_DPI`].
    #[cfg(test)]
    pub(crate) fn new(width: f64, height: f64) -> Self {
        Self::with_style(width, height, PNG_DPI, false)
    }

    /// A page of `width` × `height` points at `dpi` pixels per inch (clamped
    /// to 36–1200), white or (`transparent`) clear, with sans-serif text.
    pub(crate) fn with_style(width: f64, height: f64, dpi: f64, transparent: bool) -> Self {
        Self::with_font(width, height, dpi, transparent, PlotFont::Sans)
    }

    /// [`Raster::with_style`] with text in `font`.
    pub(crate) fn with_font(width: f64, height: f64, dpi: f64, transparent: bool, font: PlotFont) -> Self {
        let dpi = if dpi.is_finite() { dpi.clamp(36.0, 1200.0) } else { PNG_DPI };
        let scale = (dpi / 72.0) as f32;
        let px = |v: f64| ((v * f64::from(scale)).ceil() as u32).max(1);
        let mut pixmap = Pixmap::new(px(width), px(height)).expect("a positive page size");
        if !transparent {
            pixmap.fill(tiny_skia::Color::WHITE);
        }
        let face = match font {
            PlotFont::Sans => LIBERATION_SANS,
            PlotFont::Serif => LIBERATION_SERIF,
        };
        let fonts = [
            FontRef::try_from_slice(face).expect("the bundled Liberation font"),
            FontRef::try_from_slice(DEJAVU).expect("the bundled DejaVu Sans font"),
        ];
        Self { pixmap, scale, clips: Vec::new(), fonts, dpi }
    }

    /// The picture as straight (not premultiplied) RGBA bytes, row by row:
    /// `(width, height, bytes)`.
    pub(crate) fn into_rgba(self) -> (u32, u32, Vec<u8>) {
        let (w, h) = (self.pixmap.width(), self.pixmap.height());
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for p in self.pixmap.pixels() {
            let c = p.demultiply();
            rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
        (w, h, rgba)
    }

    fn paint(color: Rgba) -> Paint<'static> {
        let [r, g, b, a] = color.0;
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        paint
    }

    fn path(&self, pts: &[[f64; 2]], closed: bool) -> Option<tiny_skia::Path> {
        let mut pb = PathBuilder::new();
        let s = f64::from(self.scale);
        let mut finite = pts.iter().filter(|p| p[0].is_finite() && p[1].is_finite());
        let first = finite.next()?;
        pb.move_to((first[0] * s) as f32, (first[1] * s) as f32);
        for p in finite {
            pb.line_to((p[0] * s) as f32, (p[1] * s) as f32);
        }
        if closed {
            pb.close();
        }
        pb.finish()
    }

    pub(crate) fn polyline(&mut self, pts: &[[f64; 2]], color: Rgba, width: f64, dash: &[f64], closed: bool) {
        let Some(path) = self.path(pts, closed) else { return };
        let mut stroke = Stroke {
            width: (width as f32 * self.scale).max(0.5),
            line_join: LineJoin::Round,
            line_cap: LineCap::Butt,
            ..Stroke::default()
        };
        if !dash.is_empty() {
            let intervals: Vec<f32> = dash.iter().map(|d| *d as f32 * self.scale).collect();
            stroke.dash = StrokeDash::new(intervals, 0.0);
        }
        let mask = self.clips.last().map(|(_, m)| m);
        self.pixmap.stroke_path(&path, &Self::paint(color), &stroke, Transform::identity(), mask);
    }

    pub(crate) fn fill(&mut self, pts: &[[f64; 2]], color: Rgba) {
        if pts.len() < 3 {
            return;
        }
        let Some(path) = self.path(pts, true) else { return };
        let mask = self.clips.last().map(|(_, m)| m);
        self.pixmap.fill_path(&path, &Self::paint(color), FillRule::Winding, Transform::identity(), mask);
    }

    /// A bitmap stretched over the page rectangle `[left, top, right,
    /// bottom]` (points), each cell a flat block: sampled nearest-neighbour,
    /// so cells keep sharp edges however far they are enlarged.
    pub(crate) fn image(&mut self, r: [f64; 4], bitmap: &crate::bitmap::Bitmap) {
        if !bitmap.is_valid() || !r.iter().all(|v| v.is_finite()) || r[2] <= r[0] || r[3] <= r[1] {
            return;
        }
        let Some(mut source) = Pixmap::new(bitmap.width, bitmap.height) else { return };
        for (px, p) in source.pixels_mut().iter_mut().zip(bitmap.rgba.chunks_exact(4)) {
            *px = tiny_skia::ColorU8::from_rgba(p[0], p[1], p[2], p[3]).premultiply();
        }
        let s = self.scale;
        let sx = (r[2] - r[0]) as f32 * s / bitmap.width as f32;
        let sy = (r[3] - r[1]) as f32 * s / bitmap.height as f32;
        let transform = Transform::from_row(sx, 0.0, 0.0, sy, r[0] as f32 * s, r[1] as f32 * s);
        let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Nearest, ..Default::default() };
        let mask = self.clips.last().map(|(_, m)| m);
        self.pixmap.draw_pixmap(0, 0, source.as_ref(), &paint, transform, mask);
    }

    /// Fill closed rings as one shape by the even–odd rule.
    pub(crate) fn fill_rings(&mut self, rings: &[Vec<[f64; 2]>], color: Rgba) {
        let mut pb = PathBuilder::new();
        let s = f64::from(self.scale);
        for ring in rings {
            let mut finite = ring.iter().filter(|p| p[0].is_finite() && p[1].is_finite());
            let Some(first) = finite.next() else { continue };
            pb.move_to((first[0] * s) as f32, (first[1] * s) as f32);
            for p in finite {
                pb.line_to((p[0] * s) as f32, (p[1] * s) as f32);
            }
            pb.close();
        }
        let Some(path) = pb.finish() else { return };
        let mask = self.clips.last().map(|(_, m)| m);
        self.pixmap.fill_path(&path, &Self::paint(color), FillRule::EvenOdd, Transform::identity(), mask);
    }

    /// The font that has `c` (the text font first).
    fn font_for(&self, c: char) -> &FontRef<'static> {
        self.fonts.iter().find(|f| f.glyph_id(c).0 != 0).unwrap_or(&self.fonts[0])
    }

    /// Width of `text` in pixels at `px` pixels per em.
    fn width_px(&self, text: &str, px: f32) -> f32 {
        text.chars()
            .map(|c| self.font_for(c).as_scaled(PxScale::from(px)).h_advance(self.font_for(c).glyph_id(c)))
            .sum()
    }

    /// Advance width of `text` at `size` points, in points.
    pub(crate) fn measure(&self, text: &str, size: f64) -> f64 {
        f64::from(self.width_px(text, size as f32 * self.scale) / self.scale)
    }

    /// Text with its baseline at `at[1]`; `align` 0 = start, 0.5 = centre, 1 = end.
    pub(crate) fn text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: f64, bold: bool) {
        if !(at[0].is_finite() && at[1].is_finite()) {
            return;
        }
        self.draw_text(at, text, size, color, align, bold, false);
    }

    /// Text turned a quarter turn anticlockwise (reading upwards) with its
    /// baseline on the vertical line through `at`; `align` places it along
    /// that line relative to `at[1]`.
    pub(crate) fn vertical_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: f64) {
        if !(at[0].is_finite() && at[1].is_finite()) {
            return;
        }
        self.draw_text(at, text, size, color, align, false, true);
    }

    /// Text turned `degrees` about `at` (negative turns anticlockwise on
    /// the page, so −45 reads up and to the right), its baseline through
    /// `at`; `align` as for [`Raster::text`]. The glyph outlines are filled as
    /// paths under the rotation, so any angle renders without resampling.
    pub(crate) fn turned_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: f64, degrees: f32) {
        use ab_glyph::OutlineCurve;
        if !(at[0].is_finite() && at[1].is_finite()) {
            return;
        }
        let px = size as f32 * self.scale;
        let mut x = -self.width_px(text, px) * align as f32;
        let mut pb = PathBuilder::new();
        for c in text.chars() {
            let font = self.font_for(c).clone();
            let scaled = font.as_scaled(PxScale::from(px));
            let id = font.glyph_id(c);
            if let Some(outline) = font.outline(id) {
                let k = scaled.scale_factor();
                // Font units have y upward; the page has it downward.
                let to = |p: ab_glyph::Point| (x + p.x * k.horizontal, -p.y * k.vertical);
                let mut last: Option<ab_glyph::Point> = None;
                for curve in &outline.curves {
                    let (start, end) = match *curve {
                        OutlineCurve::Line(a, b) | OutlineCurve::Quad(a, _, b) | OutlineCurve::Cubic(a, _, _, b) => {
                            (a, b)
                        }
                    };
                    if last != Some(start) {
                        if last.is_some() {
                            pb.close();
                        }
                        let (sx, sy) = to(start);
                        pb.move_to(sx, sy);
                    }
                    match *curve {
                        OutlineCurve::Line(_, b) => {
                            let (bx, by) = to(b);
                            pb.line_to(bx, by);
                        }
                        OutlineCurve::Quad(_, c1, b) => {
                            let ((cx, cy), (bx, by)) = (to(c1), to(b));
                            pb.quad_to(cx, cy, bx, by);
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let ((ax, ay), (cx, cy), (bx, by)) = (to(c1), to(c2), to(b));
                            pb.cubic_to(ax, ay, cx, cy, bx, by);
                        }
                    }
                    last = Some(end);
                }
                if last.is_some() {
                    pb.close();
                }
            }
            x += scaled.h_advance(id);
        }
        let Some(path) = pb.finish() else { return };
        let transform =
            Transform::from_rotate(degrees).post_translate(at[0] as f32 * self.scale, at[1] as f32 * self.scale);
        let mask = self.clips.last().map(|(_, m)| m);
        self.pixmap.fill_path(&path, &Self::paint(color), FillRule::Winding, transform, mask);
    }

    /// Lay the text out horizontally about the anchor; when `turned`, each
    /// glyph pixel is moved a quarter turn about the anchor (an exact
    /// permutation of the pixel grid, so no resampling).
    #[allow(clippy::too_many_arguments)]
    fn draw_text(&mut self, at: [f64; 2], text: &str, size: f64, color: Rgba, align: f64, bold: bool, turned: bool) {
        let px = size as f32 * self.scale;
        let anchor = ((at[0] as f32 * self.scale).round(), (at[1] as f32 * self.scale).round());
        let width = self.width_px(text, px);
        let (start, baseline) = if turned { anchor } else { (at[0] as f32 * self.scale, at[1] as f32 * self.scale) };
        let mut x = start - width * align as f32;
        let clip = self.clips.last().map(|(r, _)| *r);
        let [r, g, b, a] = color.0;
        // A bold weight is drawn as the regular glyph twice, a hair apart.
        let offsets: &[f32] = if bold { &[0.0, px * 0.045] } else { &[0.0] };
        let (w, h) = (self.pixmap.width(), self.pixmap.height());
        for c in text.chars() {
            let font = self.font_for(c).clone();
            let scaled = font.as_scaled(PxScale::from(px));
            let id = font.glyph_id(c);
            for &dx in offsets {
                let glyph = id.with_scale_and_position(px, point(x + dx, baseline));
                let Some(outlined) = font.outline_glyph(glyph) else { continue };
                let bounds = outlined.px_bounds();
                let pixels = self.pixmap.pixels_mut();
                outlined.draw(|gx, gy, coverage| {
                    let (mut px_x, mut px_y) = (bounds.min.x + gx as f32, bounds.min.y + gy as f32);
                    if turned {
                        // Along the text becomes up the page; below the
                        // baseline becomes right of it.
                        (px_x, px_y) = (anchor.0 + (px_y - anchor.1), anchor.1 - (px_x - anchor.0) - 1.0);
                    }
                    if px_x < 0.0 || px_y < 0.0 || px_x >= w as f32 || px_y >= h as f32 {
                        return;
                    }
                    if let Some([x0, y0, x1, y1]) = clip {
                        if px_x < x0 || px_x >= x1 || px_y < y0 || px_y >= y1 {
                            return;
                        }
                    }
                    let alpha = coverage.clamp(0.0, 1.0) * f32::from(a) / 255.0;
                    let i = px_y as usize * w as usize + px_x as usize;
                    let d = pixels[i];
                    // Source-over on premultiplied pixels.
                    let over =
                        |src: u8, dst: u8| (f32::from(src) * alpha + f32::from(dst) * (1.0 - alpha)).round() as u8;
                    let out_a = over(255, d.alpha());
                    let channel = |src: u8, dst: u8| over(src, dst).min(out_a);
                    if let Some(p) = tiny_skia::PremultipliedColorU8::from_rgba(
                        channel(r, d.red()),
                        channel(g, d.green()),
                        channel(b, d.blue()),
                        out_a,
                    ) {
                        pixels[i] = p;
                    }
                });
            }
            x += scaled.h_advance(id);
        }
    }

    pub(crate) fn clip_begin(&mut self, r: [f64; 4]) {
        let s = f64::from(self.scale);
        let mut rect = [(r[0] * s) as f32, (r[1] * s) as f32, (r[2] * s) as f32, (r[3] * s) as f32];
        if let Some((outer, _)) = self.clips.last() {
            rect = [rect[0].max(outer[0]), rect[1].max(outer[1]), rect[2].min(outer[2]), rect[3].min(outer[3])];
        }
        let mut mask = Mask::new(self.pixmap.width(), self.pixmap.height()).expect("a positive page size");
        if let Some(area) =
            tiny_skia::Rect::from_ltrb(rect[0], rect[1], rect[2].max(rect[0] + 0.01), rect[3].max(rect[1] + 0.01))
        {
            mask.fill_path(&PathBuilder::from_rect(area), FillRule::Winding, false, Transform::identity());
        }
        self.clips.push((rect, mask));
    }

    pub(crate) fn clip_end(&mut self) {
        self.clips.pop();
    }

    /// The PNG file.
    pub(crate) fn finish(self) -> Vec<u8> {
        let dpi = self.dpi;
        let (w, h, rgba) = self.into_rgba();
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let per_metre = (dpi / 0.0254).round() as u32;
            encoder.set_pixel_dims(Some(png::PixelDimensions {
                xppu: per_metre,
                yppu: per_metre,
                unit: png::Unit::Meter,
            }));
            let mut writer = encoder.write_header().expect("PNG header into memory");
            writer.write_image_data(&rgba).expect("PNG data into memory");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a PNG: (width, height, pixels per metre, RGBA bytes).
    fn decode(bytes: &[u8]) -> (u32, u32, Option<u32>, Vec<u8>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        let dims = reader.info().pixel_dims.map(|d| d.xppu);
        (info.width, info.height, dims, buf[..info.buffer_size()].to_vec())
    }

    /// Pixels come out at 300 ppi with the density recorded; fills, strokes,
    /// text and clipping all reach the pixels.
    #[test]
    fn png_pages_draw_at_300_ppi() {
        let mut r = Raster::new(72.0, 36.0);
        // A red square in the left half, clipped to its top half.
        r.clip_begin([0.0, 0.0, 72.0, 18.0]);
        r.fill(&[[4.0, 4.0], [32.0, 4.0], [32.0, 32.0], [4.0, 32.0]], Rgba::rgb(220, 0, 0));
        r.clip_end();
        r.polyline(&[[40.0, 30.0], [68.0, 30.0]], Rgba::rgb(0, 0, 200), 2.0, &[], false);
        r.text([54.0, 14.0], "Ag", 12.0, Rgba::rgb(0, 0, 0), 0.5, false);
        let (w, h, ppm, px) = decode(&r.finish());
        assert_eq!((w, h), (300, 150), "one inch by half an inch");
        assert_eq!(ppm, Some(11811), "300 pixels per inch");
        let at = |x: f64, y: f64| {
            let (i, j) = ((x * 300.0 / 72.0) as usize, (y * 300.0 / 72.0) as usize);
            let k = (j * w as usize + i) * 4;
            [px[k], px[k + 1], px[k + 2], px[k + 3]]
        };
        assert_eq!(at(18.0, 10.0), [220, 0, 0, 255], "inside the fill");
        assert_eq!(at(18.0, 26.0), [255, 255, 255, 255], "clipped away");
        assert_eq!(at(54.0, 30.0), [0, 0, 200, 255], "on the line");
        assert_eq!(at(2.0, 34.0), [255, 255, 255, 255], "background stays white");
        // The text inks some pixels around its anchor.
        let dark = (40..68)
            .flat_map(|x| (2..16).map(move |y| (x, y)))
            .filter(|&(x, y)| at(f64::from(x), f64::from(y))[0] < 128)
            .count();
        assert!(dark > 5, "{dark} dark points of text");
    }

    /// Turned text runs up the page, its letters left of the baseline.
    #[test]
    fn turned_text_reads_upwards() {
        let mut r = Raster::new(36.0, 72.0);
        r.vertical_text([20.0, 36.0], "Yield of the process", 9.0, Rgba::rgb(0, 0, 0), 0.5);
        let (w, h, _, px) = decode(&r.finish());
        let ink: Vec<(usize, usize)> = (0..h as usize)
            .flat_map(|y| (0..w as usize).map(move |x| (x, y)))
            .filter(|&(x, y)| px[(y * w as usize + x) * 4] < 128)
            .collect();
        let (xs, ys): (Vec<usize>, Vec<usize>) = ink.iter().copied().unzip();
        let (x_lo, x_hi) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        let (y_lo, y_hi) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
        let s = 300.0 / 72.0;
        // Tall and narrow, centred on the anchor's height, ink left of the
        // baseline (descenders aside).
        assert!(y_hi - y_lo > 4 * (x_hi - x_lo), "{x_lo}..{x_hi} by {y_lo}..{y_hi}");
        assert!(((y_lo + y_hi) as f64 / 2.0 - 36.0 * s).abs() < 6.0);
        assert!((x_hi as f64) < 20.0 * s + 3.0 * s && (x_lo as f64) > 20.0 * s - 9.0 * s);
    }
}
