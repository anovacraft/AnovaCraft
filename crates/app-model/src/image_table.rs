//! Images as tables of pixels, and back.
//!
//! **Opening.** A PNG, JPEG, TIFF (8- or 16-bit, or 32-bit floating point)
//! or BMP image opens as a long table with one row per pixel, row by row
//! from the top of the picture: columns `x` and `y` (the pixel's centre,
//! counted in pixels from the centre of the top-left pixel, `y` increasing
//! downwards, times the pixel size) and either `intensity` (a grey image, or
//! a colour image read as grey) or `red`, `green`, `blue` and `luminance`.
//! Values keep the file's own units: 0–255 for 8-bit images, 0–65535 for
//! 16-bit ones, the stored numbers for floating-point ones. Luminance (and
//! the grey reading of a colour image) is the Rec. 709 weighting
//! `Y = 0.2126 R + 0.7152 G + 0.0722 B` of the stored values (ITU-R BT.709-6,
//! item 3.2), with no gamma correction. Transparency is not read.
//!
//! An image wider or taller than [`MAX_SIDE`] pixels (about a million rows)
//! is opened one of three ways ([`SizeChoice`]):
//!
//! * **Reduce** (the default): averaged over blocks of `k × k` pixels, the
//!   smallest whole `k` that brings both sides within [`MAX_SIDE`]. Pixels
//!   left over at the right and bottom edges (fewer than `k`) are left out.
//!   `x` and `y` stay in the original pixel units (each cell at the centre
//!   of its block), so distances and spatial frequencies keep their units.
//! * **Crop**: a rectangle of the image at full resolution, its pixels keeping
//!   their original coordinates.
//! * **Keep full size**: every pixel, up to [`MAX_FULL_SIDE`] on each side.
//!
//! [`ImageInfo::note`] describes the choice; it is shown when the table
//! opens and kept in the project's notes.
//!
//! **Saving.** [`PixelGrid`] puts a long table's `x`, `y` and value columns
//! back on a grid (the inverse of opening; any evenly spaced grid works, with
//! missing cells left blank), which [`encode_image`] writes as an 8-bit PNG or
//! a 16-bit TIFF grey image, mapping two chosen limits linearly onto black and
//! white. The same grid is drawn by the image plot ([`crate::image_plot`]).

use std::io::Cursor;

use image::{DynamicImage, GenericImageView, ImageFormat};
use stat_core::{Result, StatError};
use stat_data::{Column, DataTable};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Largest side opened without a choice (an image up to 1024 × 1024 pixels
/// opens as it is).
pub const MAX_SIDE: usize = 1024;
/// Largest side "Keep full size" accepts.
pub const MAX_FULL_SIDE: usize = 4096;
/// Extensions of the image files that open as tables.
pub const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "tif", "tiff", "bmp", "dib"];

/// Rec. 709 luminance weights of red, green and blue.
pub const LUMINANCE_WEIGHTS: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// Whether `path` names an image file that opens as a table.
#[must_use]
pub fn is_image_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// A decoded image file.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    image: DynamicImage,
    /// File format name (`PNG`, `JPEG`, `TIFF`, `BMP`).
    pub format: String,
}

/// How a colour image's channels become columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelChoice {
    /// One `intensity` column (grey images; colour images read as their
    /// luminance).
    #[default]
    Grey,
    /// `red`, `green`, `blue` and `luminance` columns.
    Rgb,
}

/// How an image larger than [`MAX_SIDE`] pixels opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeChoice {
    /// Average blocks of pixels to fit (see [`reduction_factor`]).
    #[default]
    Reduce,
    /// A rectangle at full resolution: left and top pixel (from 0), width and
    /// height in pixels.
    Crop {
        /// First column.
        left: usize,
        /// First row.
        top: usize,
        /// Columns.
        width: usize,
        /// Rows.
        height: usize,
    },
    /// Every pixel.
    Full,
}

/// Choices for opening an image as a table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageOpenOptions {
    /// Columns for colour images.
    pub channels: ChannelChoice,
    /// What to do with a large image (ignored for one within [`MAX_SIDE`]).
    pub size: SizeChoice,
    /// Length of one pixel in the units `x` and `y` are given in (1 counts
    /// pixels).
    pub pixel_size: f64,
}

impl Default for ImageOpenOptions {
    fn default() -> Self {
        Self { channels: ChannelChoice::Grey, size: SizeChoice::Reduce, pixel_size: 1.0 }
    }
}

/// What an image table was made from, for its note.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageInfo {
    /// The file's name.
    pub file: String,
    /// Width and height of the image in pixels.
    pub original: (usize, usize),
    /// Description of the pixels, e.g. `8-bit RGB`.
    pub pixels: String,
    /// Width and height of the table's grid.
    pub grid: (usize, usize),
    /// Block size of a reduced image (1 when not reduced).
    pub reduction: usize,
    /// The crop rectangle (left, top, width, height), when cropped.
    pub crop: Option<[usize; 4]>,
    /// Columns and rows of pixels left out at the right and bottom edges
    /// when reducing.
    pub left_out: (usize, usize),
    /// Length of a pixel in `x` and `y` units.
    pub pixel_size: f64,
    /// Whether the file's transparency was ignored.
    pub alpha_ignored: bool,
}

impl ImageInfo {
    /// The one-paragraph description shown when the table opens and kept in
    /// the project notes.
    #[must_use]
    pub fn note(&self) -> String {
        let (w, h) = self.original;
        let (gw, gh) = self.grid;
        let mut s = format!("Image {}: {w} × {h} pixels, {}", self.file, self.pixels);
        if self.reduction > 1 {
            let k = self.reduction;
            s.push_str(&format!("; reduced by averaging blocks of {k} × {k} pixels to {gw} × {gh}"));
            match self.left_out {
                (0, 0) => {}
                (c, 0) => s.push_str(&format!(" ({c} columns at the right edge left out)")),
                (0, r) => s.push_str(&format!(" ({r} rows at the bottom edge left out)")),
                (c, r) => s.push_str(&format!(" ({c} columns at the right edge and {r} rows at the bottom left out)")),
            }
        } else if let Some([left, top, cw, ch]) = self.crop {
            s.push_str(&format!(
                "; cropped to columns {left}–{} and rows {top}–{} ({cw} × {ch} pixels)",
                left + cw - 1,
                top + ch - 1
            ));
        } else if w > MAX_SIDE || h > MAX_SIDE {
            s.push_str("; kept at full size");
        }
        if self.pixel_size == 1.0 {
            s.push_str(". x and y count pixels from the centre of the top-left pixel, y downwards.");
        } else {
            s.push_str(&format!(
                ". x and y are pixel positions times the pixel size {}, from the centre of the top-left pixel, y downwards.",
                crate::format::format_number(self.pixel_size, 6)
            ));
        }
        if self.alpha_ignored {
            s.push_str(" Transparency was not read.");
        }
        s
    }
}

impl DecodedImage {
    /// Decode image file bytes (PNG, JPEG, TIFF or BMP, recognised by their
    /// content).
    ///
    /// # Errors
    /// When the bytes are not an image in one of these formats, or the image
    /// is damaged.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let reader = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| invalid(format!("could not read the image: {e}")))?;
        let format = match reader.format() {
            Some(ImageFormat::Png) => "PNG",
            Some(ImageFormat::Jpeg) => "JPEG",
            Some(ImageFormat::Tiff) => "TIFF",
            Some(ImageFormat::Bmp) => "BMP",
            _ => return Err(invalid("not a PNG, JPEG, TIFF or BMP image")),
        };
        let mut reader = reader;
        // Large scans are allowed; the table size is limited separately.
        reader.no_limits();
        let image = reader.decode().map_err(|e| invalid(format!("could not decode the {format} image: {e}")))?;
        Ok(Self { image, format: format.to_owned() })
    }

    /// Read the image file at `path`.
    ///
    /// # Errors
    /// When the file cannot be read or decoded.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| invalid(format!("could not read {}: {e}", path.display())))?;
        Self::decode(&bytes)
    }

    /// Width and height in pixels.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        let (w, h) = self.image.dimensions();
        (w as usize, h as usize)
    }

    /// Whether the image has colour channels.
    #[must_use]
    pub fn is_colour(&self) -> bool {
        self.image.color().has_color()
    }

    /// Whether the image has an alpha channel.
    #[must_use]
    pub fn has_alpha(&self) -> bool {
        self.image.color().has_alpha()
    }

    /// Bits per channel and channel names, e.g. `16-bit grey`.
    #[must_use]
    pub fn describe(&self) -> String {
        let c = self.image.color();
        let bits = c.bits_per_pixel() / u16::from(c.channel_count());
        let kind = match (c.has_color(), c.has_alpha()) {
            (true, true) => "RGB with transparency",
            (true, false) => "RGB",
            (false, true) => "grey with transparency",
            (false, false) => "grey",
        };
        let float = matches!(self.image, DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_));
        if float { format!("{bits}-bit floating-point {kind}") } else { format!("{bits}-bit {kind}") }
    }

    /// Red, green and blue of pixel `(x, y)` in the file's units (grey images
    /// give the grey level three times).
    fn rgb(&self, x: u32, y: u32) -> [f64; 3] {
        use DynamicImage as D;
        match &self.image {
            D::ImageLuma8(b) => [f64::from(b.get_pixel(x, y).0[0]); 3],
            D::ImageLumaA8(b) => [f64::from(b.get_pixel(x, y).0[0]); 3],
            D::ImageLuma16(b) => [f64::from(b.get_pixel(x, y).0[0]); 3],
            D::ImageLumaA16(b) => [f64::from(b.get_pixel(x, y).0[0]); 3],
            D::ImageRgb8(b) => b.get_pixel(x, y).0.map(f64::from),
            D::ImageRgba8(b) => {
                let p = b.get_pixel(x, y).0;
                [p[0], p[1], p[2]].map(f64::from)
            }
            D::ImageRgb16(b) => b.get_pixel(x, y).0.map(f64::from),
            D::ImageRgba16(b) => {
                let p = b.get_pixel(x, y).0;
                [p[0], p[1], p[2]].map(f64::from)
            }
            D::ImageRgb32F(b) => b.get_pixel(x, y).0.map(f64::from),
            D::ImageRgba32F(b) => {
                let p = b.get_pixel(x, y).0;
                [p[0], p[1], p[2]].map(f64::from)
            }
            // Formats added to the image crate later: read through 16 bits.
            other => {
                let p = other.get_pixel(x, y).0;
                [p[0], p[1], p[2]].map(|v| f64::from(v) * 257.0)
            }
        }
    }

    /// A small RGBA picture of the image for a preview, at most `side`
    /// pixels on its longer side, with its width and height.
    #[must_use]
    pub fn thumbnail(&self, side: u32) -> (u32, u32, Vec<u8>) {
        let t = self.image.thumbnail(side, side).to_rgba8();
        (t.width(), t.height(), t.into_raw())
    }

    /// The grid a table opened with `size` would have: `(columns, rows)`,
    /// block size and the edge pixels left out.
    #[must_use]
    pub fn plan(&self, size: SizeChoice) -> GridPlan {
        let (w, h) = self.size();
        if w <= MAX_SIDE && h <= MAX_SIDE {
            return GridPlan { origin: (0, 0), grid: (w, h), block: 1, left_out: (0, 0) };
        }
        match size {
            SizeChoice::Reduce => {
                let k = reduction_factor(w, h);
                let (cols, rows) = ((w / k).max(1), (h / k).max(1));
                GridPlan {
                    origin: (0, 0),
                    grid: (cols, rows),
                    block: k,
                    left_out: (w.saturating_sub(cols * k), h.saturating_sub(rows * k)),
                }
            }
            SizeChoice::Crop { left, top, width, height } => {
                let left = left.min(w - 1);
                let top = top.min(h - 1);
                let width = width.clamp(1, w - left);
                let height = height.clamp(1, h - top);
                GridPlan { origin: (left, top), grid: (width, height), block: 1, left_out: (0, 0) }
            }
            SizeChoice::Full => GridPlan { origin: (0, 0), grid: (w, h), block: 1, left_out: (0, 0) },
        }
    }
}

/// The grid an image opens to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPlan {
    /// Top-left pixel read (a crop's corner; `(0, 0)` otherwise).
    pub origin: (usize, usize),
    /// Columns and rows of the table's grid.
    pub grid: (usize, usize),
    /// Pixels per block side (1 unless reduced).
    pub block: usize,
    /// Columns and rows of pixels left out at the right and bottom.
    pub left_out: (usize, usize),
}

impl GridPlan {
    /// Rows the table will have.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.grid.0 * self.grid.1
    }
}

/// The block size that brings a `width` × `height` image within
/// [`MAX_SIDE`] on both sides: `⌈max(width, height) / MAX_SIDE⌉`.
#[must_use]
pub fn reduction_factor(width: usize, height: usize) -> usize {
    width.max(height).div_ceil(MAX_SIDE).max(1)
}

/// A centred crop of at most [`MAX_SIDE`] × [`MAX_SIDE`] pixels.
#[must_use]
pub fn centred_crop(width: usize, height: usize) -> SizeChoice {
    let (cw, ch) = (width.min(MAX_SIDE), height.min(MAX_SIDE));
    SizeChoice::Crop { left: (width - cw) / 2, top: (height - ch) / 2, width: cw, height: ch }
}

/// Approximate memory a table of `cells` rows and `columns` numeric
/// columns takes, in bytes (8 bytes a value plus row states).
#[must_use]
pub fn table_bytes(cells: usize, columns: usize) -> usize {
    cells * (columns * 8 + 8)
}

/// Whether "Keep full size" is allowed for a `width` × `height` image.
#[must_use]
pub fn full_size_allowed(width: usize, height: usize) -> bool {
    width <= MAX_FULL_SIDE && height <= MAX_FULL_SIDE
}

/// The table of an image's pixels named `name`, and what it was made from.
///
/// # Errors
/// When "Keep full size" is chosen beyond [`MAX_FULL_SIDE`], or the pixel
/// size is not positive.
pub fn image_table(
    image: &DecodedImage,
    name: &str,
    file: &str,
    options: &ImageOpenOptions,
) -> Result<(DataTable, ImageInfo)> {
    let (w, h) = image.size();
    if !(options.pixel_size.is_finite() && options.pixel_size > 0.0) {
        return Err(invalid("the pixel size must be a positive number"));
    }
    if options.size == SizeChoice::Full && !full_size_allowed(w, h) {
        return Err(invalid(format!(
            "the image is {w} × {h} pixels; full size is limited to {MAX_FULL_SIDE} × {MAX_FULL_SIDE}. Reduce or crop it."
        )));
    }
    let plan = image.plan(options.size);
    let (cols, rows) = plan.grid;
    let k = plan.block;
    let n = cols * rows;
    let colour = image.is_colour() && options.channels == ChannelChoice::Rgb;
    let channels = if colour { 4 } else { 1 };
    let mut values: Vec<Vec<f64>> = vec![Vec::with_capacity(n); channels];
    let (mut xs, mut ys) = (Vec::with_capacity(n), Vec::with_capacity(n));
    let s = options.pixel_size;
    // A block's centre in original pixel units: first pixel + (k − 1) / 2.
    let centre = |start: usize| start as f64 + (k as f64 - 1.0) / 2.0;
    let area = (k * k) as f64;
    for r in 0..rows {
        let y0 = plan.origin.1 + r * k;
        for c in 0..cols {
            let x0 = plan.origin.0 + c * k;
            let mut sum = [0.0; 3];
            for yy in y0..y0 + k {
                for xx in x0..x0 + k {
                    let p = image.rgb(xx as u32, yy as u32);
                    for (a, v) in sum.iter_mut().zip(p) {
                        *a += v;
                    }
                }
            }
            let [red, green, blue] = sum.map(|v| v / area);
            let luminance = LUMINANCE_WEIGHTS[0] * red + LUMINANCE_WEIGHTS[1] * green + LUMINANCE_WEIGHTS[2] * blue;
            xs.push(centre(x0) * s);
            ys.push(centre(y0) * s);
            if colour {
                values[0].push(red);
                values[1].push(green);
                values[2].push(blue);
                values[3].push(luminance);
            } else if image.is_colour() {
                values[0].push(luminance);
            } else {
                values[0].push(red);
            }
        }
    }
    let names: &[&str] = if colour { &["red", "green", "blue", "luminance"] } else { &["intensity"] };
    let mut columns = vec![Column::numeric("x", xs), Column::numeric("y", ys)];
    columns.extend(names.iter().zip(values).map(|(n, v)| Column::numeric(*n, v)));
    let table = DataTable::new(name, columns)?;
    let info = ImageInfo {
        file: file.to_owned(),
        original: (w, h),
        pixels: image.describe(),
        grid: (cols, rows),
        reduction: k,
        crop: match options.size {
            SizeChoice::Crop { .. } if w > MAX_SIDE || h > MAX_SIDE => {
                Some([plan.origin.0, plan.origin.1, cols, rows])
            }
            _ => None,
        },
        left_out: plan.left_out,
        pixel_size: s,
        alpha_ignored: image.has_alpha(),
    };
    Ok((table, info))
}

/// Values of a long table put back on an evenly spaced grid of cells.
#[derive(Debug, Clone, PartialEq)]
pub struct PixelGrid {
    /// Cells across.
    pub cols: usize,
    /// Cells down.
    pub rows: usize,
    /// Centre of the first column and the spacing of columns.
    pub x: (f64, f64),
    /// Centre of the first row and the spacing of rows; row 0 has the least
    /// `y`.
    pub y: (f64, f64),
    /// `rows × cols` values, row by row from the least `y`; NaN for cells
    /// no row fills.
    pub values: Vec<f64>,
    /// The table row of each cell ([`stat_viz::image_plot::NO_ROW`] for none).
    pub row_index: Vec<usize>,
}

/// Largest grid [`PixelGrid::from_columns`] builds (cells).
pub const MAX_GRID_CELLS: usize = MAX_FULL_SIDE * MAX_FULL_SIDE;

impl PixelGrid {
    /// The grid of `values` placed by `x` and `y`, over the table rows for
    /// which `use_row` is true and all three are finite. The spacing on each
    /// axis is the smallest gap between distinct values; every value must
    /// lie on that grid (to within 10⁻⁶ of a spacing). Cells no row fills are
    /// NaN.
    ///
    /// # Errors
    /// When there are no usable rows, a value is off the grid, two rows give
    /// the same cell, or the grid would exceed [`MAX_GRID_CELLS`].
    pub fn from_columns(x: &[f64], y: &[f64], values: &[f64], use_row: &dyn Fn(usize) -> bool) -> Result<Self> {
        let rows: Vec<usize> = (0..values.len().min(x.len()).min(y.len()))
            .filter(|&r| use_row(r) && x[r].is_finite() && y[r].is_finite() && values[r].is_finite())
            .collect();
        if rows.is_empty() {
            return Err(invalid("no rows with x, y and a value"));
        }
        let axis = |v: &[f64], name: &str| -> Result<(f64, f64, usize)> {
            let mut u: Vec<f64> = rows.iter().map(|&r| v[r]).collect();
            u.sort_by(f64::total_cmp);
            u.dedup();
            let (first, last) = (u[0], u[u.len() - 1]);
            let step = u.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
            if !step.is_finite() {
                return Ok((first, 1.0, 1));
            }
            let n = ((last - first) / step).round() + 1.0;
            if n > MAX_FULL_SIDE as f64 * 64.0 {
                return Err(invalid(format!("{name} values are too finely or unevenly spaced to form a grid")));
            }
            Ok((first, step, n as usize))
        };
        let (x0, dx, cols) = axis(x, "x")?;
        let (y0, dy, nrows) = axis(y, "y")?;
        if cols.saturating_mul(nrows) > MAX_GRID_CELLS {
            return Err(invalid(format!(
                "the grid would be {cols} × {nrows} cells; at most {MAX_FULL_SIDE} × {MAX_FULL_SIDE} are drawn"
            )));
        }
        let no_row = stat_viz::image_plot::NO_ROW;
        let mut grid = vec![f64::NAN; cols * nrows];
        let mut index = vec![no_row; cols * nrows];
        let place = |v: f64, start: f64, step: f64, name: &str, r: usize| -> Result<usize> {
            let f = (v - start) / step;
            if (f - f.round()).abs() > 1e-6 {
                return Err(invalid(format!(
                    "row {}: {name} = {v} is not on the grid of spacing {step} (the values must be evenly spaced)",
                    r + 1
                )));
            }
            Ok(f.round() as usize)
        };
        for &r in &rows {
            let (c, rr) = (place(x[r], x0, dx, "x", r)?, place(y[r], y0, dy, "y", r)?);
            let k = rr * cols + c;
            if index[k] != no_row {
                return Err(invalid(format!("rows {} and {} fall in the same cell", index[k] + 1, r + 1)));
            }
            index[k] = r;
            grid[k] = values[r];
        }
        Ok(Self { cols, rows: nrows, x: (x0, dx), y: (y0, dy), values: grid, row_index: index })
    }

    /// Cells holding a value.
    #[must_use]
    pub fn filled(&self) -> usize {
        self.values.iter().filter(|v| v.is_finite()).count()
    }

    /// The grid as an image plot. With `y_down`, row 0 (the least `y`) is at
    /// the top and the axis increases downwards (images); otherwise the
    /// greatest `y` is at the top.
    #[must_use]
    pub fn raster(&self, y_down: bool) -> stat_viz::RasterSpec {
        let (x0, dx) = self.x;
        let (y0, dy) = self.y;
        let x_range = (x0 - dx / 2.0, x0 + (self.cols as f64 - 0.5) * dx);
        let y_range = (y0 - dy / 2.0, y0 + (self.rows as f64 - 0.5) * dy);
        let (mut values, mut row_index) = (self.values.clone(), self.row_index.clone());
        if !y_down {
            // The spec's row 0 is the top: the greatest y.
            values = values.chunks(self.cols).rev().flatten().copied().collect();
            row_index = row_index.chunks(self.cols).rev().flatten().copied().collect();
        }
        stat_viz::RasterSpec {
            rows: self.rows,
            cols: self.cols,
            values,
            x_range,
            y_range,
            row_index: Some(row_index),
            y_down,
            equal_aspect: true,
            ..stat_viz::RasterSpec::default()
        }
    }
}

/// A file format [`encode_image`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFileFormat {
    /// 8-bit grey PNG.
    #[default]
    Png8,
    /// 16-bit grey TIFF.
    Tiff16,
}

impl ImageFileFormat {
    /// Every format, in menu order.
    pub const ALL: [Self; 2] = [Self::Png8, Self::Tiff16];

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Png8 => "PNG, 8-bit grey (256 levels)",
            Self::Tiff16 => "TIFF, 16-bit grey (65,536 levels)",
        }
    }

    /// File extension.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png8 => "png",
            Self::Tiff16 => "tif",
        }
    }

    /// Key used in scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Png8 => "png",
            Self::Tiff16 => "tiff",
        }
    }

    /// The format named by [`ImageFileFormat::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png8),
            "tif" | "tiff" => Some(Self::Tiff16),
            _ => None,
        }
    }

    fn levels(self) -> f64 {
        match self {
            Self::Png8 => 255.0,
            Self::Tiff16 => 65535.0,
        }
    }
}

/// The least and greatest finite values of a grid (`(0, 1)` for none).
#[must_use]
pub fn value_range(values: &[f64]) -> (f64, f64) {
    let (lo, hi) = values
        .iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
}

/// The level a value takes in a grey image of `levels + 1` levels: `low`
/// maps to 0 and `high` to `levels`, linearly, rounded and clamped; missing
/// values are 0. Equal limits put values at or above them at full level.
#[must_use]
pub fn grey_level(v: f64, low: f64, high: f64, levels: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    if high <= low {
        return if v >= high { levels } else { 0.0 };
    }
    ((v - low) / (high - low) * levels).round().clamp(0.0, levels)
}

/// A grid written as a grey image: row 0 of `grid` at the top, values from
/// `limits.0` (black) to `limits.1` (white).
///
/// # Errors
/// When encoding fails (it does not for valid grids).
pub fn encode_image(grid: &PixelGrid, limits: (f64, f64), format: ImageFileFormat) -> Result<Vec<u8>> {
    let (w, h) = (grid.cols as u32, grid.rows as u32);
    let levels = format.levels();
    let level = |v: f64| grey_level(v, limits.0, limits.1, levels);
    let mut out = Cursor::new(Vec::new());
    let failed = |e: image::ImageError| invalid(format!("could not write the image: {e}"));
    match format {
        ImageFileFormat::Png8 => {
            let data: Vec<u8> = grid.values.iter().map(|&v| level(v) as u8).collect();
            let buffer = image::GrayImage::from_raw(w, h, data).ok_or_else(|| invalid("grid size mismatch"))?;
            buffer.write_to(&mut out, ImageFormat::Png).map_err(failed)?;
        }
        ImageFileFormat::Tiff16 => {
            let data: Vec<u16> = grid.values.iter().map(|&v| level(v) as u16).collect();
            let buffer = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(w, h, data)
                .ok_or_else(|| invalid("grid size mismatch"))?;
            buffer.write_to(&mut out, ImageFormat::Tiff).map_err(failed)?;
        }
    }
    Ok(out.into_inner())
}

/// Columns of `table` an image is written from: `x`, `y` and the value
/// column, found by name (`x`, `y`; the value column `intensity`, else
/// `luminance`, else the first other numeric column).
#[must_use]
pub fn guess_columns(table: &DataTable) -> (Option<usize>, Option<usize>, Option<usize>) {
    let numeric = table.numeric_columns();
    let find = |name: &str| numeric.iter().copied().find(|&c| table.column(c).is_some_and(|c| c.name.eq_ignore_ascii_case(name)));
    let (x, y) = (find("x"), find("y"));
    let value = find("intensity")
        .or_else(|| find("luminance"))
        .or_else(|| numeric.iter().copied().find(|&c| Some(c) != x && Some(c) != y));
    (x, y, value)
}

#[cfg(test)]
mod tests;
