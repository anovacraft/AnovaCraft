//! Image plots: a dense grid of values (an image's pixels, a 2-D spectrum,
//! a filter mask) drawn as one picture of coloured cells, with a colour bar
//! beside it. A picture keeps hundreds of thousands of cells cheap to draw
//! on screen (one texture) and in figures (one embedded bitmap, see
//! [`crate::bitmap`]), where drawing a rectangle per cell would not.
//!
//! Values map linearly onto a [`ColorScale`] between two limits (the data's
//! least and greatest finite values unless chosen); values outside them take
//! the end colours and missing values (NaN) are left blank. Colour is never
//! the only way to read a value: hovering a cell reports its position and
//! value ([`RasterSpec::describe_cell`]), and the colour bar labels its band
//! boundaries.
//!
//! Rows run from the top of the picture down. Images count their rows
//! downwards too, so for them the vertical axis increases downwards
//! ([`RasterSpec::y_down`]); plots draw that axis in negated units
//! ([`RasterSpec::plot_y`]) and label it with the values negated back.

use crate::bitmap::Bitmap;
use crate::contour::ColorBar;
use crate::export::{ChartPanel, PanelImage};
use crate::layer::{Mark, MarkShape, Rgba, TextAnchor};
use crate::palette::ColorScale;

/// Bands of the colour bar beside an image plot (the picture itself is
/// coloured continuously).
pub const BAR_BANDS: usize = 10;

/// Marks for cells without a table row in [`RasterSpec::row_index`].
pub const NO_ROW: usize = usize::MAX;

/// A grid of values to draw as an image plot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RasterSpec {
    /// Rows of cells (down the picture).
    pub rows: usize,
    /// Columns of cells (across).
    pub cols: usize,
    /// `rows × cols` values, row by row from the top; NaN leaves a cell
    /// blank.
    pub values: Vec<f64>,
    /// Horizontal extent `(left edge, right edge)` in axis units: column
    /// `j` spans `x_range.0 + j·dx … x_range.0 + (j+1)·dx`.
    pub x_range: (f64, f64),
    /// Vertical extent `(low, high)` in axis units. Row 0 is at the high end,
    /// or at the low end when [`RasterSpec::y_down`] is set.
    pub y_range: (f64, f64),
    /// Horizontal axis title.
    pub x_title: String,
    /// Vertical axis title.
    pub y_title: String,
    /// What the values are (the colour bar's title).
    pub value_title: String,
    /// The colours.
    pub scale: ColorScale,
    /// Run the colours from the high end of the scale to the low end (the
    /// grey scale then goes from black for low values to white for high
    /// ones, as images do).
    pub reversed: bool,
    /// Values at the two ends of the colour scale; `None` uses the least and
    /// greatest finite values.
    pub limits: Option<(f64, f64)>,
    /// The table row of each cell (row-major, as `values`; [`NO_ROW`] for a
    /// cell without one), so that selecting cells selects their rows and
    /// selected rows are marked on the picture. `None` when the cells are
    /// not table rows (a spectrum, say).
    pub row_index: Option<Vec<usize>>,
    /// Labelled marks drawn over the picture (peaks, notch centres).
    pub marks: Vec<RasterMark>,
    /// One axis unit is as long across as up (square pixels stay square).
    pub equal_aspect: bool,
    /// The vertical axis increases downwards, as image rows do.
    pub y_down: bool,
}

/// A labelled point drawn over an image plot.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterMark {
    /// Position in axis units.
    pub at: [f64; 2],
    /// Label written beside it (may be empty).
    pub label: String,
    /// Marker shape.
    pub shape: MarkShape,
}

impl RasterSpec {
    /// Width and height of one cell in axis units.
    #[must_use]
    pub fn cell_size(&self) -> (f64, f64) {
        let size = |(a, b): (f64, f64), n: usize| if n > 0 { (b - a) / n as f64 } else { 1.0 };
        (size(self.x_range, self.cols), size(self.y_range, self.rows))
    }

    /// Whether the spec is drawable: cells, matching values and positive
    /// finite extents.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let extent = |(a, b): (f64, f64)| a.is_finite() && b.is_finite() && b > a;
        self.rows > 0
            && self.cols > 0
            && self.values.len() == self.rows * self.cols
            && extent(self.x_range)
            && extent(self.y_range)
            && self.row_index.as_ref().is_none_or(|r| r.len() == self.values.len())
    }

    /// The plot coordinate of axis value `y` (negated when the axis runs
    /// downwards).
    #[must_use]
    pub fn plot_y(&self, y: f64) -> f64 {
        if self.y_down { -y } else { y }
    }

    /// The axis value at plot coordinate `y` (the inverse of
    /// [`RasterSpec::plot_y`]).
    #[must_use]
    pub fn axis_y(&self, plot_y: f64) -> f64 {
        self.plot_y(plot_y)
    }

    /// The picture's extent in plot coordinates: `([left, right], [bottom,
    /// top])`.
    #[must_use]
    pub fn plot_bounds(&self) -> ([f64; 2], [f64; 2]) {
        let (a, b) = (self.plot_y(self.y_range.0), self.plot_y(self.y_range.1));
        ([self.x_range.0, self.x_range.1], [a.min(b), a.max(b)])
    }

    /// The limits of the colour scale: the chosen ones, else the least and
    /// greatest finite values (a single value is centred in a unit range;
    /// no finite value gives `(0, 1)`).
    #[must_use]
    pub fn color_limits(&self) -> (f64, f64) {
        if let Some((lo, hi)) = self.limits.filter(|(a, b)| a.is_finite() && b.is_finite() && b > a) {
            return (lo, hi);
        }
        let (lo, hi) = self
            .values
            .iter()
            .filter(|v| v.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        if !lo.is_finite() {
            (0.0, 1.0)
        } else if hi > lo {
            (lo, hi)
        } else {
            (lo - 0.5, hi + 0.5)
        }
    }

    /// Position of `value` on the colour scale, in [0, 1] (NaN for a missing
    /// value).
    #[must_use]
    pub fn scale_position(&self, value: f64, limits: (f64, f64)) -> f64 {
        if !value.is_finite() {
            return f64::NAN;
        }
        ((value - limits.0) / (limits.1 - limits.0)).clamp(0.0, 1.0)
    }

    /// The colour at position `t` (0–1) along the scale, reversed when
    /// [`RasterSpec::reversed`] is set.
    #[must_use]
    pub fn color_at(&self, t: f64, dark: bool) -> Rgba {
        self.scale.color(if self.reversed { 1.0 - t } else { t }, dark)
    }

    /// The picture: one cell per value, coloured along the scale (the dark
    /// variant when `dark`), missing values transparent.
    #[must_use]
    pub fn bitmap(&self, dark: bool) -> Bitmap {
        let limits = self.color_limits();
        // A lookup table keeps a million cells quick.
        const STEPS: usize = 1024;
        let table: Vec<Rgba> =
            (0..STEPS).map(|k| self.color_at(k as f64 / (STEPS - 1) as f64, dark)).collect();
        let mut rgba = Vec::with_capacity(self.values.len() * 4);
        for &v in &self.values {
            let t = self.scale_position(v, limits);
            if t.is_nan() {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let c = table[((t * (STEPS - 1) as f64).round() as usize).min(STEPS - 1)];
                rgba.extend_from_slice(&c.0);
            }
        }
        Bitmap { width: self.cols as u32, height: self.rows as u32, rgba }
    }

    /// The colour key: [`BAR_BANDS`] boxes along the scale with the values
    /// at their boundaries.
    #[must_use]
    pub fn color_bar(&self, dark: bool) -> ColorBar {
        let (lo, hi) = self.color_limits();
        let step = (hi - lo) / BAR_BANDS as f64;
        // One digit finer than the step between boundaries.
        let decimals = (1 - step.log10().floor() as i32).clamp(0, 12) as usize;
        let label = |v: f64| {
            let v = if v.abs() < step * 1e-9 { 0.0 } else { v };
            format!("{v:.decimals$}")
        };
        ColorBar {
            title: self.value_title.clone(),
            colors: (0..BAR_BANDS).map(|k| self.color_at((k as f64 + 0.5) / BAR_BANDS as f64, dark)).collect(),
            labels: (1..BAR_BANDS).map(|k| label(lo + step * k as f64)).collect(),
        }
    }

    /// The cell `(row, col)` holding axis point `(x, y)`, if any.
    #[must_use]
    pub fn cell_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        if !self.is_valid() {
            return None;
        }
        let (dx, dy) = self.cell_size();
        let fx = (x - self.x_range.0) / dx;
        let fy = if self.y_down { (y - self.y_range.0) / dy } else { (self.y_range.1 - y) / dy };
        if !(fx >= 0.0 && fy >= 0.0 && fx < self.cols as f64 && fy < self.rows as f64) {
            return None;
        }
        Some((fy as usize, fx as usize))
    }

    /// Axis coordinates of the centre of cell `(row, col)`.
    #[must_use]
    pub fn cell_center(&self, row: usize, col: usize) -> [f64; 2] {
        let (dx, dy) = self.cell_size();
        let x = self.x_range.0 + (col as f64 + 0.5) * dx;
        let y = if self.y_down {
            self.y_range.0 + (row as f64 + 0.5) * dy
        } else {
            self.y_range.1 - (row as f64 + 0.5) * dy
        };
        [x, y]
    }

    /// The value of cell `(row, col)` (NaN outside the grid).
    #[must_use]
    pub fn value(&self, row: usize, col: usize) -> f64 {
        if row < self.rows && col < self.cols { self.values[row * self.cols + col] } else { f64::NAN }
    }

    /// The cells whose centres lie in the axis-unit box `x0..=x1` ×
    /// `y0..=y1` (corners in any order), as `(row, col)`.
    #[must_use]
    pub fn cells_in(&self, x: [f64; 2], y: [f64; 2]) -> Vec<(usize, usize)> {
        if !self.is_valid() {
            return Vec::new();
        }
        let (dx, dy) = self.cell_size();
        let (x0, x1) = (x[0].min(x[1]), x[0].max(x[1]));
        let (y0, y1) = (y[0].min(y[1]), y[0].max(y[1]));
        // Index ranges of centres inside the box: centre_j = left + (j + ½)·d.
        let span = |lo: f64, hi: f64, start: f64, d: f64, n: usize| -> Option<(usize, usize)> {
            let first = ((lo - start) / d - 0.5).ceil().max(0.0);
            let last = ((hi - start) / d - 0.5).floor().min(n as f64 - 1.0);
            (first <= last).then_some((first as usize, last as usize))
        };
        let Some((c0, c1)) = span(x0, x1, self.x_range.0, dx, self.cols) else { return Vec::new() };
        // Rows counted from the edge where row 0 sits.
        let rows = if self.y_down {
            span(y0, y1, self.y_range.0, dy, self.rows)
        } else {
            span(self.y_range.1 - y1, self.y_range.1 - y0, 0.0, dy, self.rows)
        };
        let Some((r0, r1)) = rows else { return Vec::new() };
        (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r, c))).collect()
    }

    /// The table rows of the cells whose centres lie in the box (none when
    /// the cells are not table rows).
    #[must_use]
    pub fn rows_in(&self, x: [f64; 2], y: [f64; 2]) -> Vec<usize> {
        let Some(index) = &self.row_index else { return Vec::new() };
        self.cells_in(x, y).into_iter().map(|(r, c)| index[r * self.cols + c]).filter(|&r| r != NO_ROW).collect()
    }

    /// The table row of cell `(row, col)`, if it has one.
    #[must_use]
    pub fn table_row(&self, row: usize, col: usize) -> Option<usize> {
        let index = self.row_index.as_ref()?;
        index.get(row * self.cols + col).copied().filter(|&r| r != NO_ROW)
    }

    /// Hover text of cell `(row, col)`: its centre and value.
    #[must_use]
    pub fn describe_cell(&self, row: usize, col: usize) -> String {
        let [x, y] = self.cell_center(row, col);
        let name = |t: &str, d: &'static str| if t.is_empty() { d.to_owned() } else { t.to_owned() };
        format!(
            "{}: {}\n{}: {}\n{}: {}",
            name(&self.x_title, "x"),
            format_value(x),
            name(&self.y_title, "y"),
            format_value(y),
            name(&self.value_title, "Value"),
            format_value(self.value(row, col))
        )
    }

    /// The selected cells as a picture over the image: `selected(table row)`
    /// cells in `color`, the rest transparent; `None` without table rows or
    /// when nothing is selected.
    #[must_use]
    pub fn selection_bitmap(&self, selected: &dyn Fn(usize) -> bool, color: Rgba) -> Option<Bitmap> {
        let index = self.row_index.as_ref()?;
        let mut any = false;
        let mut rgba = Vec::with_capacity(index.len() * 4);
        for &r in index {
            if r != NO_ROW && selected(r) {
                any = true;
                rgba.extend_from_slice(&color.0);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
        any.then_some(Bitmap { width: self.cols as u32, height: self.rows as u32, rgba })
    }

    /// The plot as a figure panel: the picture, its marks and the colour
    /// bar, in the light palette. `selected` rows (if any) are drawn over the
    /// picture in `selection` colour, as on screen.
    #[must_use]
    pub fn chart_panel(&self, selected: Option<(&dyn Fn(usize) -> bool, Rgba)>) -> ChartPanel {
        let ([x0, x1], [y0, y1]) = self.plot_bounds();
        let mut c = ChartPanel {
            x_title: self.x_title.clone(),
            y_title: self.y_title.clone(),
            x_range: (x0, x1),
            y_range: (y0, y1),
            flip_y: self.y_down,
            color_bar: Some(self.color_bar(false)),
            ..ChartPanel::default()
        };
        if self.is_valid() {
            c.image = Some(PanelImage { bitmap: self.bitmap(false), x: (x0, x1), y: (y0, y1) });
            if let Some(over) = selected.and_then(|(s, color)| self.selection_bitmap(s, color)) {
                // Selected cells as filled runs over the picture.
                c.marks.extend(self.selection_outline(&over));
            }
        }
        for m in &self.marks {
            let at = [m.at[0], self.plot_y(m.at[1])];
            c.marks.push(Mark::Points {
                points: vec![at],
                color: MARK_INK,
                shape: m.shape,
                radius: 4.5,
                filled: false,
                name: m.label.clone(),
            });
            if !m.label.is_empty() {
                // Spaces keep the text clear of the marker.
                c.marks.push(Mark::Text {
                    at,
                    text: format!("   {}", m.label),
                    color: MARK_INK,
                    anchor: TextAnchor::LeftOfText,
                    movable: None,
                });
            }
        }
        c
    }

    /// Selected cells as filled squares (runs of neighbouring selected cells
    /// in a row are merged), for figures.
    fn selection_outline(&self, over: &Bitmap) -> Vec<Mark> {
        let (dx, dy) = self.cell_size();
        let mut marks = Vec::new();
        for r in 0..self.rows {
            let mut c = 0;
            while c < self.cols {
                let on = |c: usize| over.rgba[(r * self.cols + c) * 4 + 3] != 0;
                if !on(c) {
                    c += 1;
                    continue;
                }
                let start = c;
                while c < self.cols && on(c) {
                    c += 1;
                }
                let [cx, cy] = self.cell_center(r, start);
                let (left, right) = (cx - dx / 2.0, cx - dx / 2.0 + (c - start) as f64 * dx);
                let (a, b) = (self.plot_y(cy - dy / 2.0), self.plot_y(cy + dy / 2.0));
                let fill = Rgba(over.rgba[(r * self.cols + start) * 4..][..4].try_into().expect("four bytes"));
                marks.push(Mark::Polygon {
                    points: vec![[left, a], [right, a], [right, b], [left, b]],
                    fill,
                    stroke: fill,
                    name: "Selected".to_owned(),
                });
            }
        }
        marks
    }
}

/// Ink of the labelled marks over a picture: dark, so it reads on the light
/// end of every scale; the marks are hollow so the cells under them show.
const MARK_INK: Rgba = Rgba::rgb(20, 24, 32);

/// A value for a label: up to five significant digits, no trailing zeros.
fn format_value(v: f64) -> String {
    if !v.is_finite() {
        return "—".to_owned();
    }
    if v == 0.0 {
        return "0".to_owned();
    }
    let magnitude = v.abs().log10().floor() as i32;
    if !(-4..6).contains(&magnitude) {
        return format!("{v:.4e}");
    }
    let decimals = (4 - magnitude).max(0) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3-row, 4-column image of the values 0–11, rows counted downwards
    /// from y = 0 with unit pixels.
    fn image() -> RasterSpec {
        RasterSpec {
            rows: 3,
            cols: 4,
            values: (0..12).map(f64::from).collect(),
            x_range: (-0.5, 3.5),
            y_range: (-0.5, 2.5),
            x_title: "x".into(),
            y_title: "y".into(),
            value_title: "intensity".into(),
            row_index: Some((0..12).collect()),
            y_down: true,
            equal_aspect: true,
            ..RasterSpec::default()
        }
    }

    #[test]
    fn reversed_scales_swap_the_ends_in_the_picture_and_the_bar() {
        let plain = image();
        let reversed = RasterSpec { reversed: true, ..image() };
        assert_eq!(reversed.color_at(0.0, false), plain.scale.color(1.0, false));
        let (a, b) = (plain.bitmap(false), reversed.bitmap(false));
        assert_eq!(&a.rgba[..4], &b.rgba[b.rgba.len() - 4..], "the lowest cell takes the highest colour");
        assert_eq!(plain.color_bar(false).colors[0], reversed.color_bar(false).colors[BAR_BANDS - 1]);
    }

    #[test]
    fn cells_are_found_by_position_either_way_up() {
        let s = image();
        assert!(s.is_valid());
        assert_eq!(s.cell_size(), (1.0, 1.0));
        // y runs downwards: y = 0 is row 0.
        assert_eq!(s.cell_at(0.0, 0.0), Some((0, 0)));
        assert_eq!(s.cell_at(3.2, 2.4), Some((2, 3)));
        assert_eq!(s.cell_at(3.6, 0.0), None);
        assert_eq!(s.cell_center(2, 1), [1.0, 2.0]);
        assert_eq!(s.plot_bounds(), ([-0.5, 3.5], [-2.5, 0.5]));
        // Upwards: row 0 is at the top, the high end.
        let up = RasterSpec { y_down: false, ..image() };
        assert_eq!(up.cell_at(0.0, 2.0), Some((0, 0)));
        assert_eq!(up.cell_center(0, 0), [0.0, 2.0]);
        assert_eq!(up.cell_at(0.0, 0.0), Some((2, 0)));
        for (r, c) in [(0, 0), (1, 2), (2, 3)] {
            let [x, y] = s.cell_center(r, c);
            assert_eq!(s.cell_at(x, y), Some((r, c)));
            let [x, y] = up.cell_center(r, c);
            assert_eq!(up.cell_at(x, y), Some((r, c)));
        }
        assert_eq!(s.value(1, 2), 6.0);
        assert!(s.value(3, 0).is_nan());
        assert!(s.describe_cell(1, 2).contains("intensity: 6"), "{}", s.describe_cell(1, 2));
    }

    #[test]
    fn a_box_selects_the_rows_of_the_cells_whose_centres_it_holds() {
        let s = image();
        // Centres x ∈ {1, 2}, y ∈ {0, 1}: cells (0,1),(0,2),(1,1),(1,2).
        assert_eq!(s.rows_in([0.6, 2.2], [1.4, -0.3]), vec![1, 2, 5, 6]);
        assert!(s.rows_in([10.0, 11.0], [0.0, 1.0]).is_empty());
        let up = RasterSpec { y_down: false, ..image() };
        // Upwards, y ∈ [1.6, 2.4] holds row 0's centre (y = 2).
        assert_eq!(up.rows_in([-1.0, 0.4], [1.6, 2.4]), vec![0]);
        let unrowed = RasterSpec { row_index: None, ..image() };
        assert!(unrowed.rows_in([0.0, 3.0], [0.0, 2.0]).is_empty());
        let mut sparse = image();
        sparse.row_index.as_mut().unwrap()[1] = NO_ROW;
        assert_eq!(sparse.rows_in([0.6, 1.4], [-0.4, 0.4]), Vec::<usize>::new());
        assert_eq!(sparse.table_row(0, 1), None);
        assert_eq!(sparse.table_row(0, 2), Some(2));
    }

    #[test]
    fn colours_follow_the_limits_and_missing_values_are_blank() {
        let mut s = image();
        assert_eq!(s.color_limits(), (0.0, 11.0));
        s.values[5] = f64::NAN;
        let b = s.bitmap(false);
        assert!(b.is_valid());
        assert_eq!(&b.rgba[0..4], &ColorScale::Sequential.color(0.0, false).0);
        assert_eq!(&b.rgba[44..48], &ColorScale::Sequential.color(1.0, false).0);
        assert_eq!(b.rgba[5 * 4 + 3], 0, "missing value is transparent");
        // Chosen limits clamp.
        s.limits = Some((2.0, 4.0));
        s.scale = ColorScale::Grey;
        let b = s.bitmap(false);
        assert_eq!(&b.rgba[0..4], &ColorScale::Grey.color(0.0, false).0);
        assert_eq!(&b.rgba[3 * 4..3 * 4 + 4], &ColorScale::Grey.color(0.5, false).0);
        assert_eq!(&b.rgba[11 * 4..11 * 4 + 4], &ColorScale::Grey.color(1.0, false).0);
        // Unusable limits fall back to the data; a constant image is centred.
        s.limits = Some((3.0, 3.0));
        assert_eq!(s.color_limits(), (0.0, 11.0));
        let flat = RasterSpec { values: vec![7.0; 12], ..image() };
        assert_eq!(flat.color_limits(), (6.5, 7.5));
        let bar = s.color_bar(false);
        assert_eq!((bar.colors.len(), bar.labels.len()), (BAR_BANDS, BAR_BANDS - 1));
        assert_eq!(bar.labels[4], "5.5");
        let wide = RasterSpec { limits: Some((-1600.0, 1600.0)), ..image() };
        assert_eq!(wide.color_bar(false).labels[..5], ["-1280", "-960", "-640", "-320", "0"]);
        assert_eq!(bar.title, "intensity");
    }

    #[test]
    fn figure_panels_carry_the_picture_and_flip_the_axis() {
        let mut s = image();
        s.marks.push(RasterMark { at: [1.0, 2.0], label: "peak".into(), shape: MarkShape::Diamond });
        let selected = |r: usize| r == 5 || r == 6;
        let panel = s.chart_panel(Some((&selected, Rgba::rgb(200, 0, 0))));
        let image = panel.image.as_ref().unwrap();
        assert_eq!((image.x, image.y), ((-0.5, 3.5), (-2.5, 0.5)));
        assert!(panel.flip_y && panel.color_bar.is_some());
        // One merged run of two selected cells, then the mark and its label.
        let polygons = panel.marks.iter().filter(|m| matches!(m, Mark::Polygon { .. })).count();
        assert_eq!(polygons, 1);
        assert!(panel.marks.iter().any(|m| matches!(m, Mark::Points { points, .. } if points[0] == [1.0, -2.0])));
        assert!(panel.marks.iter().any(|m| matches!(m, Mark::Text { text, .. } if text.trim() == "peak")));
    }

    /// A 2 × 2 picture of four distinct colours fills its plot in every
    /// figure format: drawn into PNG pixels, embedded as PNG data in SVG, as
    /// an image XObject in PDF and as an inline image in EPS.
    #[test]
    fn figures_embed_the_picture_in_every_format() {
        use crate::export::{Figure, VectorFormat, export_chart};
        let spec = RasterSpec {
            rows: 2,
            cols: 2,
            values: vec![0.0, 1.0, 2.0, 3.0],
            x_range: (0.0, 2.0),
            y_range: (0.0, 2.0),
            scale: ColorScale::Grey,
            y_down: true,
            ..RasterSpec::default()
        };
        let panel = spec.chart_panel(None);
        let figure = |format| Figure { title: "", format, width: 300.0, height: 300.0 };
        // PNG: the picture's top-left cell (value 0) is the lightest grey and
        // its bottom-right cell (value 3) the darkest.
        let png = export_chart(std::slice::from_ref(&panel), figure(VectorFormat::Png));
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        let (w, h) = (info.width as usize, info.height as usize);
        let grey = |c: Rgba| c.0[0];
        let light = grey(ColorScale::Grey.color(0.0, false));
        let dark = grey(ColorScale::Grey.color(1.0, false));
        let count = |v: u8| buf.chunks_exact(4).filter(|p| p[0] == v && p[1] == v && p[3] == 255).count();
        assert!(count(light) > w * h / 40, "light cell drawn");
        assert!(count(dark) > w * h / 40, "dark cell drawn");
        let first = |v: u8| buf.chunks_exact(4).position(|p| p[0] == v && p[1] == v).unwrap();
        let (l, d) = (first(light), first(dark));
        assert!(l / w < d / w && l % w < (d % w), "light cell up and left of the dark one");
        // SVG: a PNG data URI holding the 2 × 2 cells.
        let svg = String::from_utf8(export_chart(std::slice::from_ref(&panel), figure(VectorFormat::Svg))).unwrap();
        let start = svg.find("base64,").unwrap() + 7;
        let data = &svg[start..start + svg[start..].find('"').unwrap()];
        let png = crate::bitmap::base64_decode(data).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut cells = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut cells).unwrap();
        assert_eq!(cells, spec.bitmap(false).rgba);
        assert!(svg.contains("xmlns:xlink") && svg.contains("pixelated"));
        // PDF: an image XObject with its samples compressed.
        let pdf_bytes = export_chart(std::slice::from_ref(&panel), figure(VectorFormat::Pdf));
        // The binary marker in the header is the only non-ASCII; blank it.
        let pdf: String = pdf_bytes.iter().map(|&b| if b < 128 { b as char } else { '#' }).collect();
        assert!(pdf.contains("/Im0 Do") && pdf.contains("/XObject << /Im0"), "image drawn");
        assert!(pdf.contains("/Width 2 /Height 2 /ColorSpace /DeviceRGB"));
        assert!(!pdf.contains("/SMask"), "opaque cells need no mask");
        // The xref offsets still point at each object.
        let xref = pdf.rfind("startxref\n").unwrap();
        let offset: usize = pdf[xref + 10..].lines().next().unwrap().parse().unwrap();
        assert!(pdf[offset..].starts_with("xref"));
        // EPS: an inline image with the same samples.
        let eps = String::from_utf8(export_chart(std::slice::from_ref(&panel), figure(VectorFormat::Eps))).unwrap();
        assert!(eps.contains("/Width 2 /Height 2") && eps.contains("/DataSource currentfile /ASCII85Decode filter"));
        // A figure drawing keeps the picture for figure pages.
        let drawing = export_chart(std::slice::from_ref(&panel), figure(VectorFormat::Drawing));
        let back = crate::figure_page::Drawing::from_bytes(&drawing).unwrap();
        assert_eq!(back.to_bytes(), drawing);
    }

    #[test]
    fn values_are_written_briefly() {
        assert_eq!(format_value(1234.5678), "1234.6");
        assert_eq!(format_value(0.5), "0.5");
        assert_eq!(format_value(2.0), "2");
        assert_eq!(format_value(-0.000_012_34), "-1.2340e-5");
        assert_eq!(format_value(f64::NAN), "—");
    }
}
