//! 2-D linear convolution, correlation and autocorrelation.
//!
//! The convolution of `a` (`R₁ × C₁`) with `b` (`R₂ × C₂`) is
//! `(a ∗ b)_{i,j} = Σ_{p,q} a_{p,q} b_{i−p, j−q}` over the
//! `(R₁ + R₂ − 1) × (C₁ + C₂ − 1)` *full* output; *same* keeps the
//! `R₁ × C₁` block starting at `(⌊(R₂ − 1)/2⌋, ⌊(C₂ − 1)/2⌋)` and *valid*
//! the block where the smaller input lies wholly inside the larger (one
//! input must be at least as large as the other along both axes). These
//! are the conventions of SciPy's `scipy.signal.convolve` (and
//! `fftconvolve`). The correlation is `Σ_{p,q} a_{p+i, q+j} b_{p,q}`,
//! computed as the convolution of `a` with `b` rotated by 180°
//! (`scipy.signal.correlate`).
//!
//! The sums are evaluated directly or by zero-padded 2-D FFTs
//! ([`ConvolutionMethod`]); `Auto` uses the FFT when
//! `R₁C₁R₂C₂ > 65 536`.
//!
//! The **autocorrelation** of an image (mean removed if asked) is its full
//! correlation with itself, lags `−(R − 1) … R − 1` down and
//! `−(C − 1) … C − 1` across; normalised, it is divided by its value at
//! zero lag so the peak is 1. Peaks away from zero lag reveal the period
//! and direction of repeating texture.

use super::Grid;
use super::transform::{Padding, fast_length, irfft2, pad, rfft2};
pub use crate::convolve::{ConvolutionMethod, ConvolutionMode};
use crate::{Result, invalid};

fn full_direct(a: &Grid, b: &Grid) -> Grid {
    let (rows, cols) = (a.rows + b.rows - 1, a.cols + b.cols - 1);
    let mut out = vec![0.0; rows * cols];
    for p in 0..a.rows {
        for q in 0..a.cols {
            let v = a.get(p, q);
            if v == 0.0 {
                continue;
            }
            for i in 0..b.rows {
                let row = &mut out[(p + i) * cols + q..(p + i) * cols + q + b.cols];
                for (o, &w) in row.iter_mut().zip(b.row(i)) {
                    *o += v * w;
                }
            }
        }
    }
    Grid { rows, cols, data: out, dx: a.dx, dy: a.dy }
}

fn full_fft(a: &Grid, b: &Grid) -> Grid {
    let (rows, cols) = (a.rows + b.rows - 1, a.cols + b.cols - 1);
    let (pr, pc) = (fast_length(rows), fast_length(cols));
    let embed = |g: &Grid| rfft2(&pad(g, Padding::Zero { rows: pr, cols: pc }).expect("padded size is larger"));
    let mut fa = embed(a);
    let fb = embed(b);
    for (x, &y) in fa.data.iter_mut().zip(&fb.data) {
        *x *= y;
    }
    let back = irfft2(&fa, pc);
    let mut out = back.crop(0, 0, rows, cols);
    (out.dx, out.dy) = (a.dx, a.dy);
    out
}

fn trim(full: Grid, a: &Grid, b: &Grid, mode: ConvolutionMode) -> Result<Grid> {
    match mode {
        ConvolutionMode::Full => Ok(full),
        ConvolutionMode::Same => Ok(full.crop((b.rows - 1) / 2, (b.cols - 1) / 2, a.rows, a.cols)),
        ConvolutionMode::Valid => {
            let (big, small) = if a.rows >= b.rows && a.cols >= b.cols {
                (a, b)
            } else if b.rows >= a.rows && b.cols >= a.cols {
                (b, a)
            } else {
                return Err(invalid("for valid mode one image must be at least as large as the other along both axes"));
            };
            Ok(full.crop(small.rows - 1, small.cols - 1, big.rows - small.rows + 1, big.cols - small.cols + 1))
        }
    }
}

/// 2-D linear convolution of `a` with `b` (the result keeps `a`'s spacing).
pub fn convolve2(a: &Grid, b: &Grid, mode: ConvolutionMode, method: ConvolutionMethod) -> Result<Grid> {
    a.check("image")?;
    b.check("kernel")?;
    let use_fft = match method {
        ConvolutionMethod::Direct => false,
        ConvolutionMethod::Fft => true,
        ConvolutionMethod::Auto => a.len() * b.len() > 65_536,
    };
    let full = if use_fft { full_fft(a, b) } else { full_direct(a, b) };
    trim(full, a, b, mode)
}

/// 2-D cross-correlation of `a` with `b`: the convolution of `a` with `b`
/// rotated by 180°.
pub fn correlate2(a: &Grid, b: &Grid, mode: ConvolutionMode, method: ConvolutionMethod) -> Result<Grid> {
    convolve2(a, &b.flipped(), mode, method)
}

/// A 2-D autocorrelation surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Autocorrelation2 {
    /// Vertical lags (rows of `value`), `−(R − 1) … R − 1`, in pixels.
    pub lag_rows: Vec<i64>,
    /// Horizontal lags (columns of `value`), `−(C − 1) … C − 1`, in pixels.
    pub lag_cols: Vec<i64>,
    /// Autocorrelation at each lag; zero lag is the centre cell.
    pub value: Grid,
}

/// The 2-D autocorrelation of an image, with the mean removed first when
/// `remove_mean`, and divided by its zero-lag value when `normalise`.
pub fn autocorrelation2(image: &Grid, remove_mean: bool, normalise: bool) -> Result<Autocorrelation2> {
    image.check("image")?;
    let x = if remove_mean { image.subtract_mean() } else { image.clone() };
    let mut value = correlate2(&x, &x, ConvolutionMode::Full, ConvolutionMethod::Auto)?;
    if normalise {
        let zero = value.get(image.rows - 1, image.cols - 1);
        if zero <= 0.0 {
            return Err(invalid("the image is constant, so its autocorrelation cannot be normalised"));
        }
        for v in &mut value.data {
            *v /= zero;
        }
    }
    let lags = |n: usize| (-(n as i64 - 1)..=n as i64 - 1).collect();
    Ok(Autocorrelation2 { lag_rows: lags(image.rows), lag_cols: lags(image.cols), value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_and_fft_agree_in_every_mode() {
        let a = Grid::from_fn(9, 7, |r, c| ((r * 3 + c * 5) as f64 * 0.41).sin());
        let b = Grid::from_fn(4, 3, |r, c| (r + 2 * c) as f64 * 0.1 - 0.2);
        for mode in [ConvolutionMode::Full, ConvolutionMode::Same, ConvolutionMode::Valid] {
            let d = convolve2(&a, &b, mode, ConvolutionMethod::Direct).unwrap();
            let f = convolve2(&a, &b, mode, ConvolutionMethod::Fft).unwrap();
            assert_eq!((d.rows, d.cols), (f.rows, f.cols));
            for (x, y) in d.data.iter().zip(&f.data) {
                assert!((x - y).abs() < 1e-12);
            }
        }
        let tall = Grid::zeros(5, 2);
        let wide = Grid::zeros(2, 5);
        assert!(convolve2(&tall, &wide, ConvolutionMode::Valid, ConvolutionMethod::Direct).is_err());
    }

    #[test]
    fn autocorrelation_peaks_at_zero_lag() {
        let a = Grid::from_fn(12, 10, |r, c| ((r * 7 + c) as f64 * 0.9).cos());
        let ac = autocorrelation2(&a, true, true).unwrap();
        assert_eq!((ac.value.rows, ac.value.cols), (23, 19));
        assert!((ac.value.get(11, 9) - 1.0).abs() < 1e-12);
        assert!(ac.value.data.iter().all(|&v| v <= 1.0 + 1e-12));
        assert_eq!(ac.lag_rows[0], -11);
    }
}
