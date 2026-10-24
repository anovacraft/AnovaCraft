//! Two-dimensional Fourier analysis of images and other regular grids.
//!
//! An image is a [`Grid`]: `rows × cols` values in row-major order, row 0
//! at the top, with pixel spacings `dx` (along a row, to the right) and
//! `dy` (down the columns). Frequencies are in cycles per unit of `dx` and
//! `dy` (cycles per pixel when both are 1).
//!
//! * [`transform`] — the 2-D DFT ([`fft2`], [`ifft2`], [`rfft2`],
//!   [`irfft2`]), centring ([`fft_shift2`], [`ifft_shift2`]), frequency
//!   axes, zero and mirror padding, mean removal and 2-D windows
//!   (separable or radially symmetric).
//! * [`spectrum`] — centred magnitude, phase and power spectra, radially
//!   and angularly averaged power with a log–log power-law fit, and the
//!   dominant spatial frequencies with their periods and directions.
//! * [`filter`] — frequency-domain filters (ideal, Butterworth and
//!   Gaussian low-, high-, band-pass and band-reject masks), notch filters
//!   placed in conjugate pairs, and filtering by inverse transform.
//! * [`convolve`] — 2-D linear convolution and correlation (full, same,
//!   valid) and the 2-D autocorrelation.
//! * [`register`] — phase-correlation registration with sub-pixel
//!   refinement, and Fourier (sub-pixel) shifting.
//! * [`restore`] — Gaussian point-spread functions and 2-D Wiener and
//!   Richardson–Lucy deconvolution.
//!
//! # Definition
//! The 2-D DFT of `x_{r,c}` (`r < R`, `c < C`) is
//!
//! ```text
//! X_{k,l} = Σ_r Σ_c x_{r,c} e^{−2πi (k r / R + l c / C)},
//! ```
//!
//! with inverse `x = (1/RC) Σ_k Σ_l X_{k,l} e^{2πi (kr/R + lc/C)}`, the
//! convention of `numpy.fft.fft2`. The transform is separable (Bracewell
//! 2000, *The Fourier Transform and Its Applications*, 3rd ed., ch. 13):
//! the 1-D transform of [`crate::fft`] is applied to every row and then to
//! every column, the rows of each pass in parallel. Row index `k` maps to
//! the vertical frequency `f_y = k/(R dy)` and column index `l` to the
//! horizontal frequency `f_x = l/(C dx)` (negative halves as in
//! `numpy.fft.fftfreq`).
//!
//! Every routine is checked against NumPy, SciPy or scikit-image
//! (`tests/image2d_reference.rs`, values from
//! `tests/data/image2d_reference_cases.py`).

pub mod convolve;
pub mod filter;
pub mod register;
pub mod restore;
pub mod spectrum;
pub mod transform;

pub use convolve::{Autocorrelation2, autocorrelation2, convolve2, correlate2};
pub use filter::{FilterShape, FilteredImage, FrequencyFilter, Notch, band_mask, frequency_filter, notch_mask};
pub use register::{Registration, fourier_shift, phase_correlation};
pub use restore::{Regularizer, gaussian_psf, richardson_lucy2, wiener2};
pub use spectrum::{
    AngularSpectrum, DirectionalProfile, FrequencyPeak, PowerLawFit, RadialSpectrum, Spectrum2, SpectrumOptions2, angular_spectrum,
    directional_profile, dominant_frequencies, power_law_fit, radial_spectrum, spectrum2,
};
pub use transform::{
    Padding, Taper2, fast_length, fft_shift2, fft2, frequency_axes, ifft_shift2, ifft2, irfft2, pad, rfft2, taper2,
};

use crate::complex::Complex;
use crate::{Result, StatError, invalid};

/// A real-valued image: `rows × cols` values in row-major order (row 0 at
/// the top), with pixel spacings `dx` across and `dy` down.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// Number of rows (image height).
    pub rows: usize,
    /// Number of columns (image width).
    pub cols: usize,
    /// Values, row-major: `data[r * cols + c]`.
    pub data: Vec<f64>,
    /// Spacing between columns (horizontal pixel size), positive.
    pub dx: f64,
    /// Spacing between rows (vertical pixel size), positive.
    pub dy: f64,
}

/// A complex-valued grid (a 2-D transform), row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct ComplexGrid {
    /// Number of rows.
    pub rows: usize,
    /// Number of columns.
    pub cols: usize,
    /// Values, row-major: `data[r * cols + c]`.
    pub data: Vec<Complex>,
}

/// A grid built from long-format pixel data (one table row per pixel).
#[derive(Debug, Clone, PartialEq)]
pub struct PixelGrid {
    /// The image.
    pub grid: Grid,
    /// The table row of each cell, row-major (for linking cells to rows).
    pub source_row: Vec<usize>,
    /// The x value of column 0 (left pixel centre).
    pub x0: f64,
    /// The y value of row 0 (top pixel centre).
    pub y0: f64,
}

impl Grid {
    /// A grid with unit spacing. Fails when `data.len() ≠ rows · cols`, the
    /// grid is empty, or a value is not finite.
    pub fn new(rows: usize, cols: usize, data: Vec<f64>) -> Result<Self> {
        if rows == 0 || cols == 0 {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        if data.len() != rows * cols {
            return Err(StatError::DimensionMismatch {
                context: "image values vs. rows × cols",
                expected: rows * cols,
                found: data.len(),
            });
        }
        if data.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "image" });
        }
        Ok(Grid { rows, cols, data, dx: 1.0, dy: 1.0 })
    }

    /// The same grid with pixel spacings `dx` (across) and `dy` (down);
    /// both must be positive and finite.
    pub fn with_spacing(mut self, dx: f64, dy: f64) -> Result<Self> {
        if !(dx.is_finite() && dx > 0.0 && dy.is_finite() && dy > 0.0) {
            return Err(invalid("the pixel spacings must be positive numbers"));
        }
        self.dx = dx;
        self.dy = dy;
        Ok(self)
    }

    /// A grid of zeros with unit spacing.
    pub fn zeros(rows: usize, cols: usize) -> Self {
        Grid { rows, cols, data: vec![0.0; rows * cols], dx: 1.0, dy: 1.0 }
    }

    /// Build a grid from a function of `(row, col)`, unit spacing.
    pub fn from_fn(rows: usize, cols: usize, f: impl Fn(usize, usize) -> f64) -> Self {
        let mut data = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            for c in 0..cols {
                data.push(f(r, c));
            }
        }
        Grid { rows, cols, data, dx: 1.0, dy: 1.0 }
    }

    /// Build a grid from long-format pixel data: `x`, `y` and `value` give
    /// one pixel per entry (in any order). Columns are the distinct `x`
    /// values in increasing order and rows the distinct `y` values in
    /// increasing order (so with y pointing down, row 0 is the top). The
    /// spacings are the gaps between neighbouring distinct values, which
    /// must be equal within a relative 10⁻⁶. Fails when a pixel is missing,
    /// repeated, or not finite.
    pub fn from_pixels(x: &[f64], y: &[f64], value: &[f64]) -> Result<PixelGrid> {
        let n = value.len();
        if x.len() != n || y.len() != n {
            return Err(StatError::DimensionMismatch {
                context: "x and y vs. pixel values",
                expected: n,
                found: if x.len() != n { x.len() } else { y.len() },
            });
        }
        if n == 0 {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        if x.iter().chain(y).chain(value).any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "pixel" });
        }
        let (xs, dx) = axis_levels(x, "x")?;
        let (ys, dy) = axis_levels(y, "y")?;
        let (rows, cols) = (ys.len(), xs.len());
        if rows * cols != n {
            return Err(invalid(format!(
                "the pixels do not fill a {rows} × {cols} grid ({n} pixels; every x, y pair must appear once)"
            )));
        }
        let index = |levels: &[f64], v: f64, step: f64| ((v - levels[0]) / step).round() as usize;
        let mut data = vec![f64::NAN; n];
        let mut source_row = vec![usize::MAX; n];
        for i in 0..n {
            let c = index(&xs, x[i], dx);
            let r = index(&ys, y[i], dy);
            let cell = r * cols + c;
            if source_row[cell] != usize::MAX {
                return Err(invalid(format!("the pixel at x = {}, y = {} appears twice", x[i], y[i])));
            }
            source_row[cell] = i;
            data[cell] = value[i];
        }
        Ok(PixelGrid { grid: Grid { rows, cols, data, dx, dy }, source_row, x0: xs[0], y0: ys[0] })
    }

    /// Number of cells.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the grid has no cells.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The value at `(row, col)`.
    pub fn get(&self, row: usize, col: usize) -> f64 {
        self.data[row * self.cols + col]
    }

    /// One row of values.
    pub fn row(&self, row: usize) -> &[f64] {
        &self.data[row * self.cols..(row + 1) * self.cols]
    }

    /// The mean value.
    pub fn mean(&self) -> f64 {
        self.data.iter().sum::<f64>() / self.data.len().max(1) as f64
    }

    /// The grid with its mean subtracted.
    pub fn subtract_mean(&self) -> Grid {
        let m = self.mean();
        self.map(|v| v - m)
    }

    /// Apply `f` to every value (keeping the shape and spacing).
    pub fn map(&self, f: impl Fn(f64) -> f64) -> Grid {
        self.like(self.data.iter().map(|&v| f(v)).collect())
    }

    /// Cell-by-cell difference `self − other` (same shape).
    pub fn sub(&self, other: &Grid) -> Grid {
        self.like(self.data.iter().zip(&other.data).map(|(a, b)| a - b).collect())
    }

    /// The grid with the order of rows and columns reversed (rotated 180°).
    pub fn flipped(&self) -> Grid {
        self.like(self.data.iter().rev().copied().collect())
    }

    /// The `rows × cols` block whose top-left cell is `(row, col)`.
    pub fn crop(&self, row: usize, col: usize, rows: usize, cols: usize) -> Grid {
        let mut data = Vec::with_capacity(rows * cols);
        for r in row..row + rows {
            data.extend_from_slice(&self.data[r * self.cols + col..r * self.cols + col + cols]);
        }
        Grid { rows, cols, data, dx: self.dx, dy: self.dy }
    }

    /// A grid of the same shape and spacing holding `data`.
    pub fn like(&self, data: Vec<f64>) -> Grid {
        debug_assert_eq!(data.len(), self.rows * self.cols);
        Grid { rows: self.rows, cols: self.cols, data, dx: self.dx, dy: self.dy }
    }

    /// The grid as complex values.
    pub fn to_complex(&self) -> ComplexGrid {
        ComplexGrid { rows: self.rows, cols: self.cols, data: self.data.iter().map(|&v| Complex::real(v)).collect() }
    }

    pub(crate) fn check(&self, what: &'static str) -> Result<()> {
        if self.rows == 0 || self.cols == 0 {
            return Err(StatError::InsufficientData { required: 1, found: 0 });
        }
        if self.data.len() != self.rows * self.cols {
            return Err(StatError::DimensionMismatch {
                context: "image values vs. rows × cols",
                expected: self.rows * self.cols,
                found: self.data.len(),
            });
        }
        if self.data.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: what });
        }
        if !(self.dx.is_finite() && self.dx > 0.0 && self.dy.is_finite() && self.dy > 0.0) {
            return Err(invalid("the pixel spacings must be positive numbers"));
        }
        Ok(())
    }
}

impl ComplexGrid {
    /// A complex grid; fails when `data.len() ≠ rows · cols`.
    pub fn new(rows: usize, cols: usize, data: Vec<Complex>) -> Result<Self> {
        if data.len() != rows * cols {
            return Err(StatError::DimensionMismatch {
                context: "image values vs. rows × cols",
                expected: rows * cols,
                found: data.len(),
            });
        }
        Ok(ComplexGrid { rows, cols, data })
    }

    /// The value at `(row, col)`.
    pub fn get(&self, row: usize, col: usize) -> Complex {
        self.data[row * self.cols + col]
    }

    /// The real parts.
    pub fn real(&self) -> Grid {
        Grid { rows: self.rows, cols: self.cols, data: self.data.iter().map(|z| z.re).collect(), dx: 1.0, dy: 1.0 }
    }

    /// The moduli `|z|`.
    pub fn abs(&self) -> Grid {
        Grid { rows: self.rows, cols: self.cols, data: self.data.iter().map(|z| z.abs()).collect(), dx: 1.0, dy: 1.0 }
    }
}

/// Distinct sorted levels of one coordinate and their common spacing.
fn axis_levels(v: &[f64], name: &str) -> Result<(Vec<f64>, f64)> {
    let mut levels = v.to_vec();
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    if levels.len() == 1 {
        return Ok((levels, 1.0));
    }
    let span = levels[levels.len() - 1] - levels[0];
    let step = span / (levels.len() - 1) as f64;
    let tol = 1e-6 * step.abs().max(span.abs() * 1e-9);
    for (j, &l) in levels.iter().enumerate() {
        if (l - (levels[0] + j as f64 * step)).abs() > tol.max(1e-6 * step) {
            return Err(invalid(format!("the {name} values are not evenly spaced")));
        }
    }
    Ok((levels, step))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_from_long_pixels_in_any_order() {
        // 2 rows × 3 cols with spacing 0.5 across and 2 down, shuffled.
        let x = [1.0, 0.0, 0.5, 0.5, 1.0, 0.0];
        let y = [2.0, 0.0, 0.0, 2.0, 0.0, 2.0];
        let v = [6.0, 1.0, 2.0, 5.0, 3.0, 4.0];
        let p = Grid::from_pixels(&x, &y, &v).unwrap();
        assert_eq!((p.grid.rows, p.grid.cols), (2, 3));
        assert_eq!(p.grid.data, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!((p.grid.dx, p.grid.dy), (0.5, 2.0));
        assert_eq!(p.source_row, vec![1, 2, 4, 5, 3, 0]);
        assert!(Grid::from_pixels(&x[..5], &y[..5], &v[..5]).is_err());
        let dup_x = [0.0, 0.0, 1.0, 1.0];
        let dup_y = [0.0, 0.0, 0.0, 1.0];
        assert!(Grid::from_pixels(&dup_x, &dup_y, &[1.0; 4]).is_err());
        assert!(Grid::from_pixels(&[0.0, 1.0, 3.0], &[0.0; 3], &[1.0; 3]).is_err());
    }

    #[test]
    fn grid_checks_its_shape() {
        assert!(Grid::new(2, 2, vec![1.0; 3]).is_err());
        assert!(Grid::new(0, 2, vec![]).is_err());
        assert!(Grid::new(1, 2, vec![1.0, f64::NAN]).is_err());
        assert!(Grid::new(1, 1, vec![1.0]).unwrap().with_spacing(0.0, 1.0).is_err());
        let g = Grid::from_fn(3, 4, |r, c| (r * 4 + c) as f64);
        assert_eq!(g.crop(1, 1, 2, 2).data, vec![5.0, 6.0, 9.0, 10.0]);
        assert_eq!(g.flipped().data[0], 11.0);
    }
}
