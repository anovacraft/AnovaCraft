//! The 2-D discrete Fourier transform, centring, frequency axes, padding and
//! 2-D windows.
//!
//! # Transforms
//! [`fft2`] and [`ifft2`] are the complex transforms defined in the module
//! overview (`numpy.fft.fft2`/`ifft2`). [`rfft2`] transforms a real image
//! and keeps the `⌊C/2⌋ + 1` non-negative horizontal frequencies
//! (`numpy.fft.rfft2`); the rest follow from the conjugate symmetry
//! `X_{−k,−l} = X̄_{k,l}` of a real input. [`irfft2`] inverts it given the
//! original number of columns.
//!
//! # Centring
//! [`fft_shift2`] moves the zero frequency from cell `(0, 0)` to cell
//! `(⌊R/2⌋, ⌊C/2⌋)` so frequencies ascend down and across
//! (`numpy.fft.fftshift`); [`ifft_shift2`] undoes it.
//!
//! # Windows
//! A *separable* window is the outer product `w_r w_c` of two periodic 1-D
//! windows ([`crate::window`]). A *radial* window depends only on the
//! distance from the image centre: the symmetric 1-D window of
//! `M = max(R, C)` points is evaluated, by linear interpolation, at
//! `M/2 − ½ + d`, where `d` is the distance of the cell from the centre
//! after scaling both axes to `M` points (cells beyond the window's end get
//! zero). This is the construction of scikit-image's `filters.window`, so
//! a radial Hann window falls to zero on the inscribed ellipse and keeps
//! no corners (Harris 1978, *Proc. IEEE* 66(1), for the 1-D windows).
//!
//! # Padding
//! Zero padding appends zeros below and to the right (interpolating the
//! spectrum, as the `s` argument of `numpy.fft.fft2`). Mirror padding
//! appends the image reflected about its last row and column
//! (`numpy.pad(mode="symmetric")`) to double each side, which makes the
//! padded image continuous across its periodic edges and so avoids the
//! wrap-around ringing of frequency-domain filters at the image borders
//! (Gonzalez & Woods 2018, *Digital Image Processing*, 4th ed., §4.6).

use super::{ComplexGrid, Grid};
use crate::complex::Complex;
use crate::fft::{FftPlan, fft_frequencies};
use crate::window::Window;
use crate::{Result, invalid};
use rayon::prelude::*;

/// Transform every row of a row-major `rows × cols` array in place.
fn transform_rows(data: &mut [Complex], cols: usize, inverse: bool) {
    let plan = FftPlan::new(cols);
    data.par_chunks_mut(cols).for_each(|row| {
        let out = if inverse { plan.inverse(row) } else { plan.forward(row) };
        row.copy_from_slice(&out);
    });
}

fn transpose<T: Copy + Send + Sync>(data: &[T], rows: usize, cols: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(data.len());
    for c in 0..cols {
        out.extend((0..rows).map(|r| data[r * cols + c]));
    }
    out
}

fn transform2(x: &ComplexGrid, inverse: bool) -> ComplexGrid {
    let (rows, cols) = (x.rows, x.cols);
    if rows == 0 || cols == 0 {
        return x.clone();
    }
    let mut data = x.data.clone();
    transform_rows(&mut data, cols, inverse);
    let mut t = transpose(&data, rows, cols);
    transform_rows(&mut t, rows, inverse);
    ComplexGrid { rows, cols, data: transpose(&t, cols, rows) }
}

/// The 2-D DFT `X_{k,l} = Σ_r Σ_c x_{r,c} e^{−2πi(kr/R + lc/C)}` of any size.
pub fn fft2(x: &ComplexGrid) -> ComplexGrid {
    transform2(x, false)
}

/// The inverse 2-D DFT (scaled by `1/(RC)`, so `ifft2(fft2(x)) = x`).
pub fn ifft2(x: &ComplexGrid) -> ComplexGrid {
    transform2(x, true)
}

/// The 2-D DFT of a real image, keeping columns `0..=⌊C/2⌋`
/// (`R × (⌊C/2⌋ + 1)` terms, as `numpy.fft.rfft2`).
pub fn rfft2(x: &Grid) -> ComplexGrid {
    let (rows, cols) = (x.rows, x.cols);
    let keep = cols / 2 + 1;
    if rows == 0 || cols == 0 {
        return ComplexGrid { rows, cols: keep, data: Vec::new() };
    }
    // Rows first, keeping the non-negative half; then only those columns.
    let mut full = x.to_complex().data;
    transform_rows(&mut full, cols, false);
    let mut data = Vec::with_capacity(rows * keep);
    for r in 0..rows {
        data.extend_from_slice(&full[r * cols..r * cols + keep]);
    }
    let mut t = transpose(&data, rows, keep);
    transform_rows(&mut t, rows, false);
    ComplexGrid { rows, cols: keep, data: transpose(&t, keep, rows) }
}

/// The smallest length `≥ n` whose prime factors are 2, 3 and 5 only (a
/// fast transform size for zero padding).
pub fn fast_length(n: usize) -> usize {
    let mut m = n.max(1);
    loop {
        let mut k = m;
        for p in [2, 3, 5] {
            while k % p == 0 {
                k /= p;
            }
        }
        if k == 1 {
            return m;
        }
        m += 1;
    }
}

/// Inverse of [`rfft2`]: the real `R × cols` image whose half spectrum is
/// given (missing terms filled by conjugate symmetry, imaginary parts that
/// conjugate symmetry forces to zero ignored, as `numpy.fft.irfft2`).
pub fn irfft2(spectrum: &ComplexGrid, cols: usize) -> Grid {
    let rows = spectrum.rows;
    let half = cols / 2 + 1;
    let take = half.min(spectrum.cols);
    // Inverse along the columns of the kept half first, then a 1-D inverse
    // real transform of each row (the order NumPy uses).
    let mut part = vec![Complex::ZERO; rows * half];
    for r in 0..rows {
        part[r * half..r * half + take].copy_from_slice(&spectrum.data[r * spectrum.cols..r * spectrum.cols + take]);
    }
    let mut t = transpose(&part, rows, half);
    transform_rows(&mut t, rows, true);
    let part = transpose(&t, half, rows);
    let mut data = vec![0.0; rows * cols];
    data.par_chunks_mut(cols.max(1)).enumerate().for_each(|(r, out)| {
        let row = crate::fft::irfft(&part[r * half..(r + 1) * half], cols);
        out.copy_from_slice(&row);
    });
    Grid { rows, cols, data, dx: 1.0, dy: 1.0 }
}

/// Centre a transform-ordered array: zero frequency moves to
/// `(⌊R/2⌋, ⌊C/2⌋)` (`numpy.fft.fftshift` over both axes).
pub fn fft_shift2<T: Clone>(data: &[T], rows: usize, cols: usize) -> Vec<T> {
    roll2(data, rows, cols, rows / 2, cols / 2)
}

/// Undo [`fft_shift2`] (`numpy.fft.ifftshift` over both axes).
pub fn ifft_shift2<T: Clone>(data: &[T], rows: usize, cols: usize) -> Vec<T> {
    roll2(data, rows, cols, rows.div_ceil(2), cols.div_ceil(2))
}

/// Circularly shift so cell `(r, c)` moves to `((r + dr) mod R, (c + dc) mod C)`.
pub(crate) fn roll2<T: Clone>(data: &[T], rows: usize, cols: usize, dr: usize, dc: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(data.len());
    for r in 0..rows {
        let sr = (r + rows - dr % rows.max(1)) % rows.max(1);
        for c in 0..cols {
            let sc = (c + cols - dc % cols.max(1)) % cols.max(1);
            out.push(data[sr * cols + sc].clone());
        }
    }
    out
}

/// The frequency of each column (`f_x`, cycles per unit of `dx`) and each
/// row (`f_y`, cycles per unit of `dy`) of a transform of the grid, in
/// transform order (`numpy.fft.fftfreq(C, dx)`, `fftfreq(R, dy)`).
pub fn frequency_axes(rows: usize, cols: usize, dx: f64, dy: f64) -> (Vec<f64>, Vec<f64>) {
    (fft_frequencies(cols, dx), fft_frequencies(rows, dy))
}

/// How an image is extended before its transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Padding {
    /// No padding (the image is treated as periodic).
    #[default]
    None,
    /// Zeros below and to the right, to the given size (at least the image's).
    Zero {
        /// Padded number of rows.
        rows: usize,
        /// Padded number of columns.
        cols: usize,
    },
    /// The image mirrored below and to the right, doubling each side.
    Mirror,
}

/// Extend the image as `padding` says (spacing kept).
pub fn pad(grid: &Grid, padding: Padding) -> Result<Grid> {
    let (rows, cols) = (grid.rows, grid.cols);
    match padding {
        Padding::None => Ok(grid.clone()),
        Padding::Zero { rows: pr, cols: pc } => {
            if pr < rows || pc < cols {
                return Err(invalid("the padded size must be at least the image size"));
            }
            let mut data = vec![0.0; pr * pc];
            for r in 0..rows {
                data[r * pc..r * pc + cols].copy_from_slice(grid.row(r));
            }
            Ok(Grid { rows: pr, cols: pc, data, dx: grid.dx, dy: grid.dy })
        }
        Padding::Mirror => {
            let (pr, pc) = (2 * rows, 2 * cols);
            let mut data = Vec::with_capacity(pr * pc);
            for r in 0..pr {
                let sr = if r < rows { r } else { 2 * rows - 1 - r };
                for c in 0..pc {
                    let sc = if c < cols { c } else { 2 * cols - 1 - c };
                    data.push(grid.get(sr, sc));
                }
            }
            Ok(Grid { rows: pr, cols: pc, data, dx: grid.dx, dy: grid.dy })
        }
    }
}

/// A 2-D data window.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Taper2 {
    /// No window (all weights 1).
    #[default]
    None,
    /// Outer product of two periodic 1-D windows.
    Separable(Window),
    /// A 1-D symmetric window swept around the centre.
    Radial(Window),
}

impl Taper2 {
    /// Short display name, e.g. `"Radial Hann"`.
    pub fn label(&self) -> String {
        match self {
            Taper2::None => "None".into(),
            Taper2::Separable(w) => format!("Separable {}", w.label()),
            Taper2::Radial(w) => format!("Radial {}", w.label()),
        }
    }
}

/// The `rows × cols` window weights for `taper`, row-major.
pub fn taper2(taper: Taper2, rows: usize, cols: usize) -> Result<Grid> {
    match taper {
        Taper2::None => Ok(Grid { rows, cols, data: vec![1.0; rows * cols], dx: 1.0, dy: 1.0 }),
        Taper2::Separable(w) => {
            let wr = w.periodic(rows)?;
            let wc = w.periodic(cols)?;
            Ok(Grid::from_fn(rows, cols, |r, c| wr[r] * wc[c]))
        }
        Taper2::Radial(w) => {
            let m = rows.max(cols);
            let w1 = w.symmetric(m)?;
            let centre = m as f64 / 2.0 - 0.5;
            let (sr, sc) = (m as f64 / rows as f64, m as f64 / cols as f64);
            let at = |i: isize| if i >= 0 && (i as usize) < m { w1[i as usize] } else { 0.0 };
            Ok(Grid::from_fn(rows, cols, |r, c| {
                let gy = r as f64 * sr - centre;
                let gx = c as f64 * sc - centre;
                let pos = (gy * gy + gx * gx).sqrt() + centre;
                let i0 = pos.floor();
                let f = pos - i0;
                let i0 = i0 as isize;
                at(i0) * (1.0 - f) + at(i0 + 1) * f
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(rows: usize, cols: usize) -> Grid {
        Grid::from_fn(rows, cols, |r, c| ((r * 7 + c * 3) as f64 * 0.37).sin() + 0.1 * r as f64 - 0.05 * c as f64)
    }

    #[test]
    fn round_trips_and_parseval() {
        for (rows, cols) in [(1, 1), (1, 6), (5, 1), (7, 5), (8, 6), (13, 17)] {
            let x = image(rows, cols);
            let f = fft2(&x.to_complex());
            let back = ifft2(&f).real();
            for (a, b) in back.data.iter().zip(&x.data) {
                assert!((a - b).abs() < 1e-12);
            }
            let e_x: f64 = x.data.iter().map(|v| v * v).sum();
            let e_f: f64 = f.data.iter().map(|z| z.norm_sqr()).sum::<f64>() / (rows * cols) as f64;
            assert!((e_x - e_f).abs() < 1e-11 * e_x.max(1.0));
            // Real input: X_{−k,−l} = conj X_{k,l}.
            for k in 0..rows {
                for l in 0..cols {
                    let a = f.get(k, l);
                    let b = f.get((rows - k) % rows, (cols - l) % cols).conj();
                    assert!((a - b).abs() < 1e-11);
                }
            }
            let half = rfft2(&x);
            assert_eq!(half.cols, cols / 2 + 1);
            let y = irfft2(&half, cols);
            for (a, b) in y.data.iter().zip(&x.data) {
                assert!((a - b).abs() < 1e-12, "{rows}×{cols}");
            }
        }
    }

    #[test]
    fn fast_lengths_have_small_factors() {
        assert_eq!(fast_length(0), 1);
        assert_eq!(fast_length(7), 8);
        assert_eq!(fast_length(1040), 1080);
        assert_eq!(fast_length(1024), 1024);
    }

    #[test]
    fn shifts_invert_each_other() {
        let v: Vec<usize> = (0..35).collect();
        let s = fft_shift2(&v, 5, 7);
        assert_eq!(s[2 * 7 + 3], 0);
        assert_eq!(ifft_shift2(&s, 5, 7), v);
        let v: Vec<usize> = (0..24).collect();
        let s = fft_shift2(&v, 4, 6);
        assert_eq!(s[2 * 6 + 3], 0);
        assert_eq!(ifft_shift2(&s, 4, 6), v);
    }

    #[test]
    fn padding_shapes() {
        let x = image(2, 3);
        let z = pad(&x, Padding::Zero { rows: 3, cols: 4 }).unwrap();
        assert_eq!(z.row(0)[..3], x.row(0)[..]);
        assert_eq!(z.row(2), &[0.0; 4]);
        assert!(pad(&x, Padding::Zero { rows: 1, cols: 4 }).is_err());
        let m = pad(&x, Padding::Mirror).unwrap();
        assert_eq!((m.rows, m.cols), (4, 6));
        assert_eq!(m.get(3, 5), x.get(0, 0));
        assert_eq!(m.get(2, 3), x.get(1, 2));
    }

    #[test]
    fn radial_window_is_symmetric_and_peaks_in_the_middle() {
        let w = taper2(Taper2::Radial(Window::Hann), 9, 9).unwrap();
        assert!((w.get(4, 4) - 1.0).abs() < 1e-12);
        assert!(w.get(0, 0).abs() < 1e-12);
        assert!((w.get(1, 3) - w.get(3, 1)).abs() < 1e-12);
    }
}
