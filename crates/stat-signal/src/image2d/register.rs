//! Phase-correlation image registration and Fourier shifting.
//!
//! # Phase correlation
//! If `m(r, c) = f(r − s_r, c − s_c)` is a shifted copy of `f`, their
//! transforms differ only by the phase ramp `e^{−2πi(k s_r/R + l s_c/C)}`.
//! The normalised cross-power spectrum
//! `P = F M̄ / |F M̄|` keeps only that phase, and its inverse transform is
//! (ideally) a single spike at the displacement (Kuglin & Hines 1975, "The
//! phase correlation image alignment method", *Proc. IEEE Conf. on
//! Cybernetics and Society*). The integer peak is refined to `1/u` of a
//! pixel by evaluating the inverse transform of `P` on a `⌈1.5u⌉`-point
//! grid of spacing `1/u` around it with a matrix-multiply DFT, which costs
//! far less than an upsampled FFT (Guizar-Sicairos, Thurman & Fienup 2008,
//! "Efficient subpixel image registration algorithms", *Optics Letters*
//! 33(2)). Displacements past half the image wrap to negative values.
//!
//! The result is the shift that, applied to the moving image, aligns it
//! with the reference (scikit-image's `registration.phase_cross_correlation`
//! with `normalization="phase"` and the same `upsample_factor`). The peak
//! height, `|P̌|` at the refined maximum divided by the number of pixels,
//! is 1 for an exact circular shift and falls towards 0 as the images
//! differ; the phase difference is the argument of the peak.
//!
//! # Fourier shift
//! [`fourier_shift`] moves an image by any (fractional) displacement by
//! multiplying its transform by the phase ramp and keeping the real part
//! of the inverse (circular, so content leaving one edge enters the
//! opposite one; `scipy.ndimage.fourier_shift`).

use super::transform::{fft2, ifft2};
use super::{ComplexGrid, Grid};
use crate::complex::Complex;
use crate::{Result, invalid};
use std::f64::consts::PI;

/// The displacement found by [`phase_correlation`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Registration {
    /// Shift down the rows (pixels) that aligns the moving image with the reference.
    pub row_shift: f64,
    /// Shift across the columns (pixels).
    pub col_shift: f64,
    /// `row_shift · dy`, in the image's units.
    pub y_shift: f64,
    /// `col_shift · dx`, in the image's units.
    pub x_shift: f64,
    /// Height of the correlation peak, `0…1` (1 for an exact circular shift).
    pub peak: f64,
    /// Argument of the correlation peak (radians).
    pub phase_difference: f64,
}

/// Signed index: `i` if `i ≤ ⌊n/2⌋`, else `i − n`.
fn wrap(i: usize, n: usize) -> f64 {
    if i > n / 2 { i as f64 - n as f64 } else { i as f64 }
}

fn argmax(v: &[Complex]) -> usize {
    let mut best = 0;
    let mut top = f64::NEG_INFINITY;
    for (i, z) in v.iter().enumerate() {
        let a = z.abs();
        if a > top {
            top = a;
            best = i;
        }
    }
    best
}

/// Integer frequency index of transform position `k` (NumPy `fftfreq` order).
fn freq_index(k: usize, n: usize) -> f64 {
    if k < n.div_ceil(2) { k as f64 } else { k as f64 - n as f64 }
}

/// Inverse DFT of `p` evaluated at rows `(u − off_r)/up` and columns
/// `(v − off_c)/up` for `u, v < size`, unscaled.
fn upsampled_inverse(p: &ComplexGrid, size: usize, up: f64, off_r: f64, off_c: f64) -> Vec<Complex> {
    let (rows, cols) = (p.rows, p.cols);
    let kernel = |n: usize, off: f64| -> Vec<Complex> {
        // kernel[u][k] = e^{2πi (u − off) k' / (n up)}.
        let mut out = Vec::with_capacity(size * n);
        for u in 0..size {
            for k in 0..n {
                out.push(Complex::cis(2.0 * PI * (u as f64 - off) * freq_index(k, n) / (n as f64 * up)));
            }
        }
        out
    };
    let kc = kernel(cols, off_c);
    let kr = kernel(rows, off_r);
    // Contract the columns: t[r][v] = Σ_l p[r][l] kc[v][l].
    let mut t = vec![Complex::ZERO; rows * size];
    for r in 0..rows {
        for v in 0..size {
            let mut s = Complex::ZERO;
            for l in 0..cols {
                s += p.data[r * cols + l] * kc[v * cols + l];
            }
            t[r * size + v] = s;
        }
    }
    // Then the rows: out[u][v] = Σ_k kr[u][k] t[k][v].
    let mut out = vec![Complex::ZERO; size * size];
    for u in 0..size {
        for k in 0..rows {
            let w = kr[u * rows + k];
            for v in 0..size {
                out[u * size + v] += w * t[k * size + v];
            }
        }
    }
    out
}

/// Register `moving` to `reference` (same size) by phase correlation,
/// refined to `1/upsample` pixel (`upsample ≥ 1`; 1 gives whole pixels).
pub fn phase_correlation(reference: &Grid, moving: &Grid, upsample: usize) -> Result<Registration> {
    reference.check("reference image")?;
    moving.check("moving image")?;
    if (reference.rows, reference.cols) != (moving.rows, moving.cols) {
        return Err(invalid(format!(
            "the moving image is {} × {} but the reference is {} × {}",
            moving.rows, moving.cols, reference.rows, reference.cols
        )));
    }
    if !(1..=1000).contains(&upsample) {
        return Err(invalid("the upsampling factor must be between 1 and 1000"));
    }
    let (rows, cols) = (reference.rows, reference.cols);
    let n = (rows * cols) as f64;
    let fa = fft2(&reference.to_complex());
    let fb = fft2(&moving.to_complex());
    let floor = 100.0 * f64::EPSILON;
    let p = ComplexGrid {
        rows,
        cols,
        data: fa
            .data
            .iter()
            .zip(&fb.data)
            .map(|(&a, &b)| {
                let z = a * b.conj();
                z / z.abs().max(floor)
            })
            .collect(),
    };
    let cc = ifft2(&p);
    let best = argmax(&cc.data);
    let mut shift_r = wrap(best / cols, rows);
    let mut shift_c = wrap(best % cols, cols);
    let mut peak = cc.data[best];
    if upsample > 1 {
        let up = upsample as f64;
        let size = (1.5 * up).ceil() as usize;
        let centre = (size as f64 / 2.0).trunc();
        let fine = upsampled_inverse(&p, size, up, centre - shift_r * up, centre - shift_c * up);
        let at = argmax(&fine);
        peak = fine[at] * (1.0 / n);
        shift_r += ((at / size) as f64 - centre) / up;
        shift_c += ((at % size) as f64 - centre) / up;
    }
    Ok(Registration {
        row_shift: shift_r,
        col_shift: shift_c,
        y_shift: shift_r * reference.dy,
        x_shift: shift_c * reference.dx,
        peak: peak.abs(),
        phase_difference: peak.arg(),
    })
}

/// Shift an image by `row_shift` pixels down and `col_shift` across
/// (fractional shifts allowed; circular).
pub fn fourier_shift(image: &Grid, row_shift: f64, col_shift: f64) -> Result<Grid> {
    image.check("image")?;
    if !(row_shift.is_finite() && col_shift.is_finite()) {
        return Err(invalid("the shift must be finite"));
    }
    let (rows, cols) = (image.rows, image.cols);
    let mut f = fft2(&image.to_complex());
    for k in 0..rows {
        let a = row_shift * freq_index(k, rows) / rows as f64;
        for l in 0..cols {
            let b = col_shift * freq_index(l, cols) / cols as f64;
            f.data[k * cols + l] *= Complex::cis(-2.0 * PI * (a + b));
        }
    }
    Ok(image.like(ifft2(&f).data.iter().map(|z| z.re).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(rows: usize, cols: usize) -> Grid {
        Grid::from_fn(rows, cols, |r, c| {
            let (y, x) = (r as f64 - 14.0, c as f64 - 19.0);
            (-(x * x + y * y) / 30.0).exp()
                + 0.5 * (-((x - 8.0).powi(2) + (y + 5.0).powi(2)) / 8.0).exp()
                + 0.2 * ((r * 3 + c * 7) as f64).sin()
        })
    }

    #[test]
    fn recovers_integer_and_fractional_shifts() {
        let reference = scene(32, 40);
        let moving = fourier_shift(&reference, 3.0, -5.0).unwrap();
        let reg = phase_correlation(&reference, &moving, 1).unwrap();
        assert_eq!((reg.row_shift, reg.col_shift), (-3.0, 5.0));
        assert!((reg.peak - 1.0).abs() < 1e-9);
        let moving = fourier_shift(&reference, 1.3, 2.75).unwrap();
        let reg = phase_correlation(&reference, &moving, 20).unwrap();
        assert!((reg.row_shift + 1.3).abs() <= 0.05 + 1e-12, "{}", reg.row_shift);
        assert!((reg.col_shift + 2.75).abs() <= 0.05 + 1e-12, "{}", reg.col_shift);
        let back = fourier_shift(&moving, reg.row_shift, reg.col_shift).unwrap();
        let err = back.data.iter().zip(&reference.data).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(err < 0.1, "{err}");
        assert!(phase_correlation(&reference, &scene(31, 40), 1).is_err());
    }
}
