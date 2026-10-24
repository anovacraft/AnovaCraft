//! Image reports (Series and Signals ▸ Signals and spectra ▸ Images), bound to the
//! row state ([`stat_signal::image2d`]).
//!
//! **Images.** An image is a long-format table: one row per pixel, with
//! numeric `x` (across) and `y` (down) columns and a value column (an
//! opened image's `intensity`, or one of `red`, `green`, `blue`,
//! `luminance`). The distinct x values must be evenly spaced, and so must
//! the distinct y values; their spacings are the pixel sizes, so
//! frequencies are in cycles per unit of x and y. Rows that are excluded or
//! missing leave a hole, which is filled with the image's mean (the summary
//! counts them); a cell with more than one row is an error. With a group
//! column, each level is a separate image analysed the same way (the first
//! [`MAX_IMAGES`] are drawn; tables list them all).
//!
//! Three analyses share the input ([`ImageMethod`]):
//!
//! * [`spectrum`] — centred log-magnitude, phase and power spectra with
//!   windows and padding, the radially averaged power spectrum with a
//!   power-law fit, the angular (orientation) spectrum and the dominant
//!   spatial frequencies.
//! * [`filter`] — low-, high-, band-pass and band-reject masks (ideal,
//!   Butterworth, Gaussian) and notch filters, applied by inverse FFT; the
//!   filtered image and the removed part are saved as columns.
//! * [`restore`] — deblurring with a Gaussian point-spread function (Wiener
//!   or constrained least squares, Richardson–Lucy), registration of a
//!   second image by phase correlation, and the 2-D autocorrelation.

pub mod filter;
pub mod restore;
pub mod spectrum;

#[cfg(test)]
mod tests;

pub use filter::{BandChoice, ImageFilterSettings, NotchPoint, ShapeChoice};
pub use restore::{DeblurMethod, ImageRestoreSettings, RestoreOperation};
pub use spectrum::{ImageSpectrumSettings, TaperChoice};

use stat_core::{Result, StatError};
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::image2d::{Grid, Padding};

use crate::format::format_number;
use stat_viz::image_plot::NO_ROW;
use stat_viz::palette::ColorScale;
use stat_viz::{RasterMark, RasterSpec};
use crate::project::s;
use crate::report::{PlotSpec, PointGroups, Polyline, ReportSection, SectionContent, SectionKind, TextTable};
use crate::signal_analysis::MAX_PLOTTED_GROUPS;
use crate::report_columns::ReportColumn;
use crate::time_series_data::json_column;

/// Most images drawn per section group (one per group level).
pub const MAX_IMAGES: usize = 4;

/// Most pixels of an image the reports accept (4096 × 4096).
pub const MAX_PIXELS: usize = 4096 * 4096;

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

pub(crate) fn section(kind: SectionKind, title: &str, content: SectionContent) -> ReportSection {
    ReportSection { kind, title: title.to_owned(), content }
}

/// The pixel columns of an image report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInput {
    /// The numeric value column (intensity or a channel).
    pub value: usize,
    /// Horizontal pixel position (increasing to the right).
    pub x: usize,
    /// Vertical pixel position (increasing downwards).
    pub y: usize,
    /// Column whose levels are separate images (`None`: one image).
    pub group: Option<usize>,
}

impl ImageInput {
    /// Every column the input reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        [self.value, self.x, self.y].into_iter().chain(self.group).collect()
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.value = f(self.value);
        self.x = f(self.x);
        self.y = f(self.y);
        self.group = self.group.map(f);
    }

    /// The input for value column `value` with the table's `x` and `y`
    /// columns (found by name, case-insensitively), if it has them.
    #[must_use]
    pub fn guess(table: &DataTable, value: usize) -> Option<Self> {
        let find = |name: &str| {
            (0..table.n_cols()).find(|&i| table.column(i).is_some_and(|c| c.name.eq_ignore_ascii_case(name)))
        };
        Some(Self { value, x: find("x")?, y: find("y")?, group: None })
    }

    fn json_members(&self, table: &DataTable) -> Vec<(String, JsonValue)> {
        let name = |i: usize| s(table.column(i).map_or("", |c| c.name.as_str()));
        let mut m = vec![
            ("column".to_owned(), name(self.value)),
            ("x".to_owned(), name(self.x)),
            ("y".to_owned(), name(self.y)),
        ];
        if let Some(g) = self.group {
            m.push(("group".to_owned(), name(g)));
        }
        m
    }

    fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let need = |key: &str| -> Result<usize> {
            json_column(v, key, table)?.ok_or_else(|| invalid(format!("the image report needs {key:?}")))
        };
        Ok(Self { value: need("column")?, x: need("x")?, y: need("y")?, group: json_column(v, "group", table)? })
    }
}

/// One image taken from the table.
#[derive(Debug, Clone)]
pub struct Image {
    /// Group level (empty without a group column).
    pub label: String,
    /// The pixel values (holes filled with the mean), with the pixel spacings.
    pub grid: Grid,
    /// The table row of each cell, row-major (`None` for a hole).
    pub source_row: Vec<Option<usize>>,
    /// x of the left pixel centre.
    pub x0: f64,
    /// y of the top pixel centre.
    pub y0: f64,
    /// Cells with no usable row (filled with the mean).
    pub holes: usize,
}

impl Image {
    /// One value per table row from one value per cell (NaN on rows the
    /// image does not use).
    pub fn fill_rows(&self, cells: &Grid, out: &mut [f64]) {
        for (cell, row) in self.source_row.iter().enumerate() {
            if let Some(r) = *row {
                out[r] = cells.data[cell];
            }
        }
    }

    /// The x extent (cell edges) of the image.
    #[must_use]
    pub fn x_range(&self) -> (f64, f64) {
        let h = self.grid.dx / 2.0;
        (self.x0 - h, self.x0 + (self.grid.cols as f64 - 1.0) * self.grid.dx + h)
    }

    /// The y extent (cell edges) of the image.
    #[must_use]
    pub fn y_range(&self) -> (f64, f64) {
        let h = self.grid.dy / 2.0;
        (self.y0 - h, self.y0 + (self.grid.rows as f64 - 1.0) * self.grid.dy + h)
    }
}

/// The images of a request.
#[derive(Debug, Clone)]
pub struct Images {
    /// Name of the value column.
    pub name: String,
    /// Names of the x and y columns.
    pub x_name: String,
    /// Name of the y column.
    pub y_name: String,
    /// Name of the group column, if any.
    pub group_name: Option<String>,
    /// One image per group (one without groups).
    pub images: Vec<Image>,
    /// Rows left out because they are excluded or missing.
    pub left_out: usize,
    /// Rows of the table.
    pub n_rows: usize,
}

/// The distinct values of `v` (sorted) as an even lattice: its start, step
/// and length, and each value's index. Values within 10⁻⁶ of a step of
/// each other are one position.
fn lattice(v: &[f64], what: &str) -> Result<(f64, f64, usize, Vec<usize>)> {
    let mut sorted: Vec<f64> = v.to_vec();
    sorted.sort_by(f64::total_cmp);
    let span = sorted[sorted.len() - 1] - sorted[0];
    let tol = 1e-9 * span.abs().max(1.0);
    sorted.dedup_by(|a, b| (*a - *b).abs() <= tol);
    if sorted.len() == 1 {
        return Ok((sorted[0], 1.0, 1, vec![0; v.len()]));
    }
    let mut gaps: Vec<f64> = sorted.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_by(f64::total_cmp);
    let step = gaps[0];
    let n = (span / step).round() as usize + 1;
    if n > 4096 || sorted.iter().any(|&x| (((x - sorted[0]) / step) - ((x - sorted[0]) / step).round()).abs() > 1e-6) {
        return Err(invalid(format!(
            "the {what} values are not evenly spaced; an image needs one row per pixel on an even grid"
        )));
    }
    let idx = v.iter().map(|&x| ((x - sorted[0]) / step).round() as usize).collect();
    Ok((sorted[0], step, n, idx))
}

impl Images {
    /// Read the images of `input`.
    ///
    /// # Errors
    /// Missing or non-numeric columns, x or y values that are not on an even
    /// grid, two rows for one pixel, more than [`MAX_PIXELS`] pixels, or an
    /// image smaller than `min` × `min`.
    pub fn extract(table: &DataTable, states: &RowStateManager, input: &ImageInput, min: usize) -> Result<Self> {
        Self::extract_column(table, states, input, input.value, min)
    }

    /// As [`Images::extract`], reading value column `column` instead (a
    /// second image on the same pixels).
    ///
    /// # Errors
    /// As [`Images::extract`].
    pub fn extract_column(
        table: &DataTable,
        states: &RowStateManager,
        input: &ImageInput,
        column: usize,
        min: usize,
    ) -> Result<Self> {
        let numeric = |i: usize| -> Result<(String, &[f64])> {
            let c = table.column(i).ok_or_else(|| invalid(format!("no column {i}")))?;
            let v = c.as_numeric().ok_or_else(|| invalid(format!("{} must be numeric", c.name)))?;
            Ok((c.name.clone(), v))
        };
        let (name, values) = numeric(column)?;
        let (x_name, xs) = numeric(input.x)?;
        let (y_name, ys) = numeric(input.y)?;
        let groups: Vec<String> = match input.group {
            Some(g) => {
                let c = table.column(g).ok_or_else(|| invalid(format!("no column {g}")))?;
                (0..table.n_rows()).map(|r| c.level_text(r)).collect()
            }
            None => vec![String::new(); table.n_rows()],
        };
        let group_name = input.group.and_then(|g| table.column(g)).map(|c| c.name.clone());
        let mut order: Vec<String> = Vec::new();
        let mut members: Vec<Vec<usize>> = Vec::new();
        for (r, g) in groups.iter().enumerate() {
            if input.group.is_some() && g.trim().is_empty() {
                continue;
            }
            // Pixel positions come from every row with a position (so an
            // excluded pixel leaves a hole rather than shrinking the image).
            if !(xs[r].is_finite() && ys[r].is_finite()) {
                continue;
            }
            match order.iter().position(|o| o == g) {
                Some(k) => members[k].push(r),
                None => {
                    order.push(g.clone());
                    members.push(vec![r]);
                }
            }
        }
        if members.is_empty() {
            return Err(invalid("no rows with x and y values"));
        }
        let mut left_out = 0;
        let mut images = Vec::with_capacity(members.len());
        for (label, rows) in order.into_iter().zip(members) {
            let which = if label.is_empty() { String::new() } else { format!(" (group {label})") };
            let px: Vec<f64> = rows.iter().map(|&r| xs[r]).collect();
            let py: Vec<f64> = rows.iter().map(|&r| ys[r]).collect();
            let (x0, dx, cols, ix) = lattice(&px, &x_name)?;
            let (y0, dy, n_rows, iy) = lattice(&py, &y_name)?;
            if cols * n_rows > MAX_PIXELS {
                return Err(invalid(format!("the image{which} has more than 4096 × 4096 pixels")));
            }
            if cols < min || n_rows < min {
                return Err(invalid(format!("the image{which} is {cols} × {n_rows} pixels; it needs at least {min} × {min}")));
            }
            let mut data = vec![f64::NAN; cols * n_rows];
            let mut source_row = vec![None; cols * n_rows];
            for (k, &r) in rows.iter().enumerate() {
                let cell = iy[k] * cols + ix[k];
                if source_row[cell].is_some() {
                    return Err(invalid(format!(
                        "rows {} and {} are the same pixel{which}; each pixel needs one row",
                        source_row[cell].map_or(0, |p: usize| p + 1),
                        r + 1
                    )));
                }
                if states.is_excluded(r) || !values[r].is_finite() {
                    left_out += 1;
                    continue;
                }
                data[cell] = values[r];
                source_row[cell] = Some(r);
            }
            let used: Vec<f64> = data.iter().copied().filter(|v| v.is_finite()).collect();
            if used.is_empty() {
                return Err(invalid(format!("the image{which} has no usable pixels (excluded or missing)")));
            }
            let mean = used.iter().sum::<f64>() / used.len() as f64;
            let holes = data.len() - used.len();
            for v in &mut data {
                if !v.is_finite() {
                    *v = mean;
                }
            }
            let grid = Grid::new(n_rows, cols, data)?.with_spacing(dx, dy)?;
            images.push(Image { label, grid, source_row, x0, y0, holes });
        }
        Ok(Self { name, x_name, y_name, group_name, images, left_out, n_rows: table.n_rows() })
    }

    /// Heading suffix of image `i` (" (group level)" with groups, else "").
    #[must_use]
    pub fn suffix(&self, i: usize) -> String {
        match &self.group_name {
            Some(g) => format!(" ({g} = {})", self.images[i].label),
            None => String::new(),
        }
    }

    /// The drawn images (the first [`MAX_IMAGES`]).
    #[must_use]
    pub fn drawn(&self) -> usize {
        self.images.len().min(MAX_IMAGES)
    }

    /// Unit of spatial frequency for labels.
    #[must_use]
    pub fn frequency_unit(&self) -> String {
        format!("cycles per unit of {}", self.x_name)
    }

    /// Summary lines common to every image report.
    #[must_use]
    pub fn summary_rows(&self) -> Vec<(String, String)> {
        let mut out = vec![("Image values".to_owned(), self.name.clone())];
        if let Some(g) = &self.group_name {
            out.push((format!("Images (levels of {g})"), self.images.len().to_string()));
        }
        let sizes: Vec<String> = self.images.iter().map(|im| format!("{} × {}", im.grid.cols, im.grid.rows)).collect();
        let mut distinct = sizes.clone();
        distinct.dedup();
        out.push(("Size (width × height, pixels)".to_owned(), distinct.join(", ")));
        if let Some(im) = self.images.first() {
            out.push((
                "Pixel spacing (x, y)".to_owned(),
                format!("{}, {}", format_number(im.grid.dx, 6), format_number(im.grid.dy, 6)),
            ));
            out.push((
                "Nyquist frequency (x, y)".to_owned(),
                format!("{}, {}", format_number(0.5 / im.grid.dx, 6), format_number(0.5 / im.grid.dy, 6)),
            ));
        }
        out.push(("Frequency unit".to_owned(), self.frequency_unit()));
        let holes: usize = self.images.iter().map(|im| im.holes).sum();
        if holes > 0 {
            out.push(("Pixels filled with the image mean (excluded or missing)".to_owned(), holes.to_string()));
        }
        if self.images.len() > MAX_IMAGES {
            out.push(("Images drawn".to_owned(), format!("the first {MAX_IMAGES} (tables list all)")));
        }
        out
    }

    /// One curve per image (spectra), grouped when there are groups.
    #[must_use]
    pub fn curves(&self, curves: Vec<Polyline>, x_label: &str, y_label: &str, name: &str) -> PlotSpec {
        let mut p = PlotSpec { x_label: x_label.to_owned(), y_label: y_label.to_owned(), ..PlotSpec::default() };
        match &self.group_name {
            Some(title) => {
                let k = curves.len().min(MAX_PLOTTED_GROUPS);
                p.groups = Some(PointGroups {
                    title: title.clone(),
                    labels: self.images.iter().take(k).map(|im| im.label.clone()).collect(),
                    membership: Vec::new(),
                    lines: curves.into_iter().take(k).collect(),
                });
            }
            None => {
                p.line = curves.into_iter().next();
                p.line_name = Some(name.to_owned());
            }
        }
        p
    }

    /// An image section: `grid` drawn over the pixel positions of image
    /// `i`, linked to its rows.
    #[must_use]
    pub fn image_section(&self, i: usize, kind: SectionKind, title: &str, grid: &Grid, value_label: &str) -> ReportSection {
        let im = &self.images[i];
        let spec = RasterSpec {
            rows: grid.rows,
            cols: grid.cols,
            values: grid.data.clone(),
            x_range: im.x_range(),
            y_range: im.y_range(),
            x_title: self.x_name.clone(),
            y_title: self.y_name.clone(),
            value_title: value_label.to_owned(),
            y_down: true,
            row_index: Some(im.source_row.iter().map(|r| r.unwrap_or(NO_ROW)).collect()),
            // Dark for low values and light for high, as in a photograph.
            scale: ColorScale::Grey,
            reversed: true,
            equal_aspect: true,
            ..RasterSpec::default()
        };
        section(kind, &format!("{title}{}", self.suffix(i)), SectionContent::Raster(Box::new(spec)))
    }
}

/// A centred frequency-domain map (spectrum or mask) as a raster: `grid`'s
/// rows run over `fy` and columns over `fx` (both ascending), so with y
/// pointing down the top row is the most negative vertical frequency.
#[must_use]
pub(crate) fn frequency_section(
    kind: SectionKind,
    title: &str,
    grid: &Grid,
    unit: &str,
    value_label: &str,
    marks: Vec<RasterMark>,
) -> ReportSection {
    let (sx, sy) = (grid.dx, grid.dy);
    let (cr, cc) = (grid.rows / 2, grid.cols / 2);
    let x0 = -(cc as f64) * sx - sx / 2.0;
    let y0 = -(cr as f64) * sy - sy / 2.0;
    let spec = RasterSpec {
        rows: grid.rows,
        cols: grid.cols,
        values: grid.data.clone(),
        x_range: (x0, x0 + grid.cols as f64 * sx),
        y_range: (y0, y0 + grid.rows as f64 * sy),
        x_title: format!("Horizontal frequency ({unit})"),
        y_title: format!("Vertical frequency ({unit})"),
        value_title: value_label.to_owned(),
        y_down: true,
        marks,
        scale: ColorScale::Sequential,
        equal_aspect: true,
        ..RasterSpec::default()
    };
    section(kind, title, SectionContent::Raster(Box::new(spec)))
}

/// What an image report does with its images.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageMethod {
    /// Spectra, radial and angular spectra, dominant frequencies.
    Spectrum(ImageSpectrumSettings),
    /// Frequency-domain filtering.
    Filter(ImageFilterSettings),
    /// Deblurring, registration and autocorrelation.
    Restore(ImageRestoreSettings),
}

impl ImageMethod {
    /// Project-file analysis name.
    #[must_use]
    pub fn key(&self) -> &'static str {
        match self {
            Self::Spectrum(_) => "image-spectrum",
            Self::Filter(_) => "image-filter",
            Self::Restore(_) => "image-restoration",
        }
    }

    /// The analysis names this module reads.
    pub const KEYS: [&'static str; 3] = ["image-spectrum", "image-filter", "image-restoration"];
}

/// A Series and Signals ▸ Signals and spectra ▸ Images request.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageAnalysisRequest {
    /// The image.
    pub input: ImageInput,
    /// What to do with it.
    pub method: ImageMethod,
}

impl ImageAnalysisRequest {
    /// Every column the request reads.
    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        let mut out = self.input.columns();
        if let ImageMethod::Restore(r) = &self.method {
            out.extend(r.operation.other_column());
        }
        out
    }

    /// Pass every column index through `f`.
    pub fn map_columns(&mut self, f: &impl Fn(usize) -> usize) {
        self.input.map_columns(f);
        if let ImageMethod::Restore(r) = &mut self.method {
            r.operation.map_column(f);
        }
    }

    /// The project-file (JSON) form, columns by name.
    #[must_use]
    pub fn to_json(&self, table: &DataTable) -> JsonValue {
        let mut m: Vec<(String, JsonValue)> = vec![("analysis".into(), s(self.method.key()))];
        m.extend(self.input.json_members(table));
        match &self.method {
            ImageMethod::Spectrum(x) => m.extend(x.json_members()),
            ImageMethod::Filter(x) => m.extend(x.json_members()),
            ImageMethod::Restore(x) => m.extend(x.json_members(table)),
        }
        JsonValue::Object(m)
    }

    /// Read the JSON form of analysis `analysis` (one of
    /// [`ImageMethod::KEYS`]); settings left out take their defaults.
    ///
    /// # Errors
    /// Missing or unknown columns, unknown option names.
    pub fn from_json(analysis: &str, v: &JsonValue, table: &DataTable) -> Result<Self> {
        let input = ImageInput::from_json(v, table)?;
        let method = match analysis {
            "image-spectrum" => ImageMethod::Spectrum(ImageSpectrumSettings::from_json(v)?),
            "image-filter" => ImageMethod::Filter(ImageFilterSettings::from_json(v)?),
            "image-restoration" => ImageMethod::Restore(ImageRestoreSettings::from_json(v, table)?),
            other => return Err(invalid(format!("unknown image analysis {other:?}"))),
        };
        Ok(Self { input, method })
    }
}

/// A computed image report.
#[derive(Debug, Clone)]
pub struct ImageAnalysis {
    request: ImageAnalysisRequest,
    title: String,
    sections: Vec<ReportSection>,
    columns: Vec<ReportColumn>,
}

impl ImageAnalysis {
    /// Read the images and run the analysis.
    ///
    /// # Errors
    /// Missing or non-numeric columns, pixels off an even grid, too small an
    /// image, or invalid settings (cut-offs above the Nyquist frequency, a
    /// second image of a different size, and so on).
    pub fn run(table: &DataTable, states: &RowStateManager, request: &ImageAnalysisRequest) -> Result<Self> {
        let (title, sections, columns) = match &request.method {
            ImageMethod::Spectrum(x) => spectrum::run(table, states, &request.input, x)?,
            ImageMethod::Filter(x) => filter::run(table, states, &request.input, x)?,
            ImageMethod::Restore(x) => restore::run(table, states, &request.input, x)?,
        };
        Ok(Self { request: request.clone(), title, sections, columns })
    }

    /// The request.
    #[must_use]
    pub fn request(&self) -> &ImageAnalysisRequest {
        &self.request
    }

    /// Report title.
    #[must_use]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// Report sections.
    #[must_use]
    pub fn sections(&self) -> Vec<ReportSection> {
        self.sections.clone()
    }

    /// Columns the report can add (filtered, removed or restored images).
    #[must_use]
    pub fn saveable_columns(&self) -> Vec<ReportColumn> {
        self.columns.clone()
    }
}

/// A two-column table of `(label, value)` lines.
pub(crate) fn summary_table(rows: Vec<(String, String)>) -> TextTable {
    let mut t = TextTable::new(["", "Value"]);
    for (k, v) in rows {
        t.push([k, v]);
    }
    t
}

// ---------------------------------------------------------------------------
// Padding in project files and dialogs.

/// Padding choices offered (zero padding to the next fast size).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaddingChoice {
    /// None: the image is treated as periodic.
    #[default]
    None,
    /// Zeros to the next size with small prime factors.
    Zero,
    /// Mirror the image (doubles each side; no edge jumps).
    Mirror,
}

impl PaddingChoice {
    /// All choices in menu order.
    pub const ALL: [Self; 3] = [Self::None, Self::Zero, Self::Mirror];

    /// Project-file key.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Zero => "zero",
            Self::Mirror => "mirror",
        }
    }

    /// Menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None (periodic)",
            Self::Zero => "Zeros to a fast size",
            Self::Mirror => "Mirror",
        }
    }

    /// The engine padding for a `rows × cols` image.
    #[must_use]
    pub fn padding(self, rows: usize, cols: usize) -> Padding {
        use stat_signal::image2d::fast_length;
        match self {
            Self::None => Padding::None,
            Self::Zero => Padding::Zero { rows: fast_length(rows), cols: fast_length(cols) },
            Self::Mirror => Padding::Mirror,
        }
    }

    pub(crate) fn from_key(key: Option<&str>) -> Result<Self> {
        Ok(match key {
            None => Self::None,
            Some(k) => Self::ALL.into_iter().find(|p| p.key() == k).ok_or_else(|| invalid(format!("unknown padding {k:?}")))?,
        })
    }
}

pub(crate) fn text<'a>(v: &'a JsonValue, key: &str) -> Option<&'a str> {
    v.get(key).and_then(JsonValue::as_str)
}

pub(crate) fn number(v: &JsonValue, key: &str, default: f64) -> f64 {
    v.get(key).and_then(JsonValue::as_f64).unwrap_or(default)
}

pub(crate) fn count(v: &JsonValue, key: &str, default: usize) -> usize {
    v.get(key).and_then(JsonValue::as_usize).unwrap_or(default)
}

pub(crate) fn flag(v: &JsonValue, key: &str, default: bool) -> bool {
    v.get(key).and_then(JsonValue::as_bool).unwrap_or(default)
}

/// `10 log₁₀ v` with a floor 120 dB below `top` (for zeros).
pub(crate) fn decibels(values: &[f64]) -> Vec<f64> {
    let top = values.iter().copied().fold(0.0_f64, f64::max);
    let floor = if top > 0.0 { top * 1e-12 } else { 1e-300 };
    values.iter().map(|&v| 10.0 * v.max(floor).log10()).collect()
}
