//! Image registration and deconvolution report: deblurring with a Gaussian
//! point-spread function (Wiener, constrained least squares or
//! Richardson–Lucy), registration of a second image by phase correlation,
//! and the 2-D autocorrelation.

use stat_core::Result;
use stat_data::{DataTable, JsonValue, RowStateManager};
use stat_signal::image2d::{
    Grid, Regularizer, autocorrelation2, fourier_shift, gaussian_psf, phase_correlation, richardson_lucy2, wiener2,
};

use super::{ImageInput, Images, count, flag, invalid, number, section, summary_table, text};
use crate::format::format_number;
use stat_viz::RasterSpec;
use stat_viz::palette::ColorScale;
use crate::project::s;
use crate::report::{ReportSection, SectionContent, SectionKind, TextTable};
use crate::report_columns::ReportColumn;
use crate::time_series_data::json_column;

/// How a blurred image is restored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeblurMethod {
    /// Parametric Wiener filter `H̄ / (|H|² + λ)`.
    Wiener {
        /// Noise-to-signal constant `λ ≥ 0`.
        lambda: f64,
    },
    /// Constrained least squares, `H̄ / (|H|² + λ|L|²)` with the Laplacian `L`.
    Smoothness {
        /// Weight `λ ≥ 0` of the roughness penalty.
        lambda: f64,
    },
    /// Richardson–Lucy iterations (non-negative images).
    RichardsonLucy {
        /// Number of iterations (1–10 000).
        iterations: usize,
    },
}

impl DeblurMethod {
    fn key(&self) -> &'static str {
        match self {
            Self::Wiener { .. } => "wiener",
            Self::Smoothness { .. } => "least-squares",
            Self::RichardsonLucy { .. } => "richardson-lucy",
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(&self) -> String {
        match *self {
            Self::Wiener { lambda } => format!("Wiener filter (λ = {})", format_number(lambda, 4)),
            Self::Smoothness { lambda } => format!("Constrained least squares (λ = {})", format_number(lambda, 4)),
            Self::RichardsonLucy { iterations } => format!("Richardson–Lucy ({iterations} iterations)"),
        }
    }
}

/// What the report does.
#[derive(Debug, Clone, PartialEq)]
pub enum RestoreOperation {
    /// Deblur with a Gaussian point-spread function of the given standard
    /// deviations (pixels across and down).
    Deblur {
        /// Restoration method.
        method: DeblurMethod,
        /// Blur standard deviation across, pixels.
        sigma_x: f64,
        /// Blur standard deviation down, pixels.
        sigma_y: f64,
    },
    /// Find the shift aligning a second image (another value column on the
    /// same pixels) with this one, and align it.
    Register {
        /// The moving image's value column.
        other: usize,
        /// Sub-pixel refinement factor (1: whole pixels; 100: 1/100 pixel).
        upsample: usize,
    },
    /// The 2-D autocorrelation (repeat distances and directions of texture).
    Autocorrelation {
        /// Subtract the mean first.
        remove_mean: bool,
    },
}

impl RestoreOperation {
    /// The second image's column, if any.
    #[must_use]
    pub fn other_column(&self) -> Option<usize> {
        match self {
            Self::Register { other, .. } => Some(*other),
            _ => None,
        }
    }

    /// Pass the second image's column through `f`.
    pub fn map_column(&mut self, f: &impl Fn(usize) -> usize) {
        if let Self::Register { other, .. } = self {
            *other = f(*other);
        }
    }
}

/// Settings of an image registration and deconvolution report.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRestoreSettings {
    /// The operation.
    pub operation: RestoreOperation,
}

impl Default for ImageRestoreSettings {
    fn default() -> Self {
        Self {
            operation: RestoreOperation::Deblur { method: DeblurMethod::Wiener { lambda: 0.01 }, sigma_x: 2.0, sigma_y: 2.0 },
        }
    }
}

impl ImageRestoreSettings {
    pub(super) fn json_members(&self, table: &DataTable) -> Vec<(String, JsonValue)> {
        let n = JsonValue::number;
        match &self.operation {
            RestoreOperation::Deblur { method, sigma_x, sigma_y } => {
                let mut m = vec![
                    ("operation".into(), s("deblur")),
                    ("method".into(), s(method.key())),
                    ("sigma_x".into(), n(*sigma_x)),
                    ("sigma_y".into(), n(*sigma_y)),
                ];
                match *method {
                    DeblurMethod::Wiener { lambda } | DeblurMethod::Smoothness { lambda } => m.push(("lambda".into(), n(lambda))),
                    DeblurMethod::RichardsonLucy { iterations } => m.push(("iterations".into(), n(iterations as f64))),
                }
                m
            }
            RestoreOperation::Register { other, upsample } => vec![
                ("operation".into(), s("register")),
                ("other".into(), s(table.column(*other).map_or("", |c| c.name.as_str()))),
                ("upsample".into(), n(*upsample as f64)),
            ],
            RestoreOperation::Autocorrelation { remove_mean } => vec![
                ("operation".into(), s("autocorrelation")),
                ("remove_mean".into(), JsonValue::Bool(*remove_mean)),
            ],
        }
    }

    pub(super) fn from_json(v: &JsonValue, table: &DataTable) -> Result<Self> {
        let operation = match text(v, "operation").unwrap_or("deblur") {
            "deblur" => {
                let method = match text(v, "method").unwrap_or("wiener") {
                    "wiener" => DeblurMethod::Wiener { lambda: number(v, "lambda", 0.01) },
                    "least-squares" => DeblurMethod::Smoothness { lambda: number(v, "lambda", 0.01) },
                    "richardson-lucy" => DeblurMethod::RichardsonLucy { iterations: count(v, "iterations", 30) },
                    other => return Err(invalid(format!("unknown deblurring method {other:?}"))),
                };
                RestoreOperation::Deblur { method, sigma_x: number(v, "sigma_x", 2.0), sigma_y: number(v, "sigma_y", 2.0) }
            }
            "register" => RestoreOperation::Register {
                other: json_column(v, "other", table)?.ok_or_else(|| invalid("registration needs the \"other\" image column"))?,
                upsample: count(v, "upsample", 20),
            },
            "autocorrelation" => RestoreOperation::Autocorrelation { remove_mean: flag(v, "remove_mean", true) },
            other => return Err(invalid(format!("unknown image operation {other:?}"))),
        };
        Ok(Self { operation })
    }
}

pub(super) fn run(
    table: &DataTable,
    states: &RowStateManager,
    input: &ImageInput,
    settings: &ImageRestoreSettings,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    match &settings.operation {
        RestoreOperation::Deblur { method, sigma_x, sigma_y } => deblur(table, states, input, *method, *sigma_x, *sigma_y),
        RestoreOperation::Register { other, upsample } => register(table, states, input, *other, *upsample),
        RestoreOperation::Autocorrelation { remove_mean } => autocorrelation(table, states, input, *remove_mean),
    }
}

fn deblur(
    table: &DataTable,
    states: &RowStateManager,
    input: &ImageInput,
    method: DeblurMethod,
    sigma_x: f64,
    sigma_y: f64,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    let images = Images::extract(table, states, input, 4)?;
    let psf = gaussian_psf(sigma_y, sigma_x)?;
    match method {
        DeblurMethod::Wiener { lambda } | DeblurMethod::Smoothness { lambda } if !(lambda.is_finite() && lambda >= 0.0) => {
            return Err(invalid("λ must be zero or positive"));
        }
        DeblurMethod::RichardsonLucy { iterations } if !(1..=10_000).contains(&iterations) => {
            return Err(invalid("use between 1 and 10 000 iterations"));
        }
        _ => {}
    }
    let mut summary = images.summary_rows();
    summary.push(("Point-spread function".into(), format!(
        "Gaussian, standard deviation {} pixels across and {} down",
        format_number(sigma_x, 4),
        format_number(sigma_y, 4)
    )));
    summary.push(("Method".into(), method.label()));
    let mut sections = vec![section(SectionKind::ImageSummary, "Summary", SectionContent::Table(summary_table(summary)))];
    let mut restored_col = vec![f64::NAN; images.n_rows];
    for (i, im) in images.images.iter().enumerate() {
        let restored = match method {
            DeblurMethod::Wiener { lambda } => wiener2(&im.grid, &psf, Regularizer::Constant(lambda))?,
            DeblurMethod::Smoothness { lambda } => wiener2(&im.grid, &psf, Regularizer::Laplacian(lambda))?,
            DeblurMethod::RichardsonLucy { iterations } => richardson_lucy2(&im.grid, &psf, iterations)?,
        };
        im.fill_rows(&restored, &mut restored_col);
        if i < images.drawn() {
            sections.push(images.image_section(i, SectionKind::ImagePlot, "Blurred Image", &im.grid, &images.name));
            sections.push(images.image_section(
                i,
                SectionKind::ImageRestored,
                "Restored Image",
                &restored,
                &format!("Restored {}", images.name),
            ));
        }
    }
    let name = &images.name;
    let columns = vec![ReportColumn::continuous("restored", "Save restored image", format!("Restored {name}"), restored_col)];
    Ok((format!("Image Deblurring: {name}"), sections, columns))
}

fn register(
    table: &DataTable,
    states: &RowStateManager,
    input: &ImageInput,
    other: usize,
    upsample: usize,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    if !(1..=1000).contains(&upsample) {
        return Err(invalid("the sub-pixel factor must be between 1 and 1000"));
    }
    let reference = Images::extract(table, states, input, 4)?;
    let moving = Images::extract_column(table, states, input, other, 4)?;
    if moving.images.len() != reference.images.len() {
        return Err(invalid("the two images must have the same groups"));
    }
    let fmt = |v: f64| format_number(v, 5);
    let mut summary = reference.summary_rows();
    summary.insert(1, ("Image aligned with it".into(), moving.name.clone()));
    summary.push((
        "Sub-pixel precision".into(),
        if upsample == 1 { "whole pixels".into() } else { format!("1/{upsample} pixel") },
    ));
    let mut sections = vec![section(SectionKind::ImageSummary, "Summary", SectionContent::Table(summary_table(summary)))];
    let mut shifts = TextTable::new(
        ["Image", "Shift across (pixels)", "Shift down (pixels)", &format!("Shift in {}", reference.x_name), &format!("Shift in {}", reference.y_name), "Correlation peak", "RMS difference before", "RMS difference after"]
            .into_iter()
            .skip(usize::from(reference.group_name.is_none())),
    );
    let mut aligned_col = vec![f64::NAN; reference.n_rows];
    let mut image_sections = Vec::new();
    let rms = |a: &Grid, b: &Grid| {
        let n = a.data.len() as f64;
        (a.data.iter().zip(&b.data).map(|(x, y)| (x - y) * (x - y)).sum::<f64>() / n).sqrt()
    };
    for (i, (r, m)) in reference.images.iter().zip(&moving.images).enumerate() {
        if (r.grid.rows, r.grid.cols) != (m.grid.rows, m.grid.cols) {
            return Err(invalid(format!(
                "{} and {} must be images of the same size",
                reference.name, moving.name
            )));
        }
        let reg = phase_correlation(&r.grid, &m.grid, upsample)?;
        let aligned = fourier_shift(&m.grid, reg.row_shift, reg.col_shift)?;
        m.fill_rows(&aligned, &mut aligned_col);
        let before = r.grid.sub(&m.grid);
        let after = aligned.sub(&r.grid);
        let mut row = vec![
            fmt(reg.col_shift),
            fmt(reg.row_shift),
            fmt(reg.x_shift),
            fmt(reg.y_shift),
            format_number(reg.peak, 4),
            fmt(rms(&r.grid, &m.grid)),
            fmt(rms(&r.grid, &aligned)),
        ];
        if reference.group_name.is_some() {
            row.insert(0, r.label.clone());
        }
        shifts.push(row);
        if i < reference.drawn() {
            image_sections.push(reference.image_section(i, SectionKind::ImagePlot, "Reference Image", &r.grid, &reference.name));
            image_sections.push(moving.image_section(i, SectionKind::ImageMoving, "Image to Align", &m.grid, &moving.name));
            image_sections.push(moving.image_section(i, SectionKind::ImageAligned, "Aligned Image", &aligned, &format!("Aligned {}", moving.name)));
            image_sections.push(reference.image_section(i, SectionKind::ImageDifferenceBefore, "Difference Before Aligning", &before, "Reference − image to align"));
            image_sections.push(reference.image_section(i, SectionKind::ImageDifferenceAfter, "Difference After Aligning", &after, "Aligned − reference"));
        }
    }
    sections.push(section(SectionKind::ImageShift, "Shift Found", SectionContent::Table(shifts)));
    sections.extend(image_sections);
    let columns = vec![ReportColumn::continuous(
        "aligned",
        "Save aligned image",
        format!("Aligned {}", moving.name),
        aligned_col,
    )];
    Ok((format!("Image Registration: {} to {}", moving.name, reference.name), sections, columns))
}

fn autocorrelation(
    table: &DataTable,
    states: &RowStateManager,
    input: &ImageInput,
    remove_mean: bool,
) -> Result<(String, Vec<ReportSection>, Vec<ReportColumn>)> {
    let images = Images::extract(table, states, input, 4)?;
    let mut summary = images.summary_rows();
    summary.push(("Mean removed".into(), if remove_mean { "Yes" } else { "No" }.into()));
    let mut sections = vec![section(SectionKind::ImageSummary, "Summary", SectionContent::Table(summary_table(summary)))];
    for i in 0..images.drawn() {
        let im = &images.images[i];
        let ac = autocorrelation2(&im.grid, remove_mean, true)?;
        let (dx, dy) = (im.grid.dx, im.grid.dy);
        let lag0 = |lags: &[i64], d: f64| lags.first().map_or(0.0, |&l| l as f64 * d) - d / 2.0;
        let spec = RasterSpec {
            rows: ac.value.rows,
            cols: ac.value.cols,
            values: ac.value.data.clone(),
            x_range: (lag0(&ac.lag_cols, dx), lag0(&ac.lag_cols, dx) + ac.value.cols as f64 * dx),
            y_range: (lag0(&ac.lag_rows, dy), lag0(&ac.lag_rows, dy) + ac.value.rows as f64 * dy),
            x_title: format!("Lag in {}", images.x_name),
            y_title: format!("Lag in {}", images.y_name),
            value_title: "Autocorrelation".into(),
            y_down: true,
            scale: ColorScale::Diverging,
            equal_aspect: true,
            ..RasterSpec::default()
        };
        sections.push(images.image_section(i, SectionKind::ImagePlot, "Image", &im.grid, &images.name));
        sections.push(section(
            SectionKind::ImageAutocorrelation,
            &format!("Autocorrelation{}", images.suffix(i)),
            SectionContent::Raster(Box::new(spec)),
        ));
    }
    Ok((format!("Image Autocorrelation: {}", images.name), sections, Vec::new()))
}
