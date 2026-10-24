//! Point-spread functions and 2-D deconvolution (image restoration).
//!
//! Both methods model the observed image as `y = x ∗ h (+ noise)`, the
//! *same*-size convolution of the unknown image `x` with a known
//! point-spread function (PSF) `h`, centred at cell
//! `(⌊(R_h − 1)/2⌋, ⌊(C_h − 1)/2⌋)` as in [`super::convolve2`].
//!
//! * **Wiener (regularized) deconvolution** (Gonzalez & Woods 2018,
//!   *Digital Image Processing*, 4th ed., §5.8–5.9): with `Y` and `H` the
//!   transforms of the image and of the PSF centred circularly,
//!   `X = Y H̄ / (|H|² + λ |L|²)`. A [`Regularizer::Constant`] uses `L = 1`
//!   (the parametric Wiener filter, `λ` a noise-to-signal power ratio); a
//!   [`Regularizer::Laplacian`] uses the transform `L` of the 5-point
//!   Laplacian, the constrained least-squares filter, which penalises
//!   roughness instead of energy (scikit-image's `restoration.wiener` with
//!   `balance = λ`, unclipped). Edges are treated as periodic.
//! * **Richardson–Lucy iteration** (Richardson 1972, *JOSA* 62(1); Lucy
//!   1974, *Astronomical Journal* 79(6)), the maximum-likelihood restoration
//!   for Poisson noise, for non-negative data and PSF (normalised to sum 1):
//!   starting from `x = ½`, `x ← x · ((y / (x ∗ h + 10⁻¹²)) ∗ h̃)` with `h̃`
//!   the PSF rotated by 180° and *same*-size linear convolutions
//!   (scikit-image's `restoration.richardson_lucy`, unclipped). More
//!   iterations sharpen further and amplify noise.
//!
//! A Gaussian PSF of standard deviations `σ_r`, `σ_c` pixels is
//! `exp(−(i²/(2σ_r²) + j²/(2σ_c²)))` on `|i| ≤ ⌈4σ_r⌉`, `|j| ≤ ⌈4σ_c⌉`
//! (the truncation of SciPy's `ndimage.gaussian_filter`), normalised to
//! sum 1.

use super::transform::{Padding, fast_length, fft2, ifft2, irfft2, pad, rfft2};
use super::{ComplexGrid, Grid};
use crate::complex::Complex;
use crate::{Result, invalid};

/// The regularizing term of [`wiener2`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Regularizer {
    /// `|H|² + λ`: the parametric Wiener filter (`λ ≥ 0`; 0 is inverse filtering).
    Constant(f64),
    /// `|H|² + λ|L|²` with `L` the Laplacian: constrained least squares.
    Laplacian(f64),
}

/// A Gaussian PSF with standard deviations `sigma_rows` and `sigma_cols`
/// pixels, truncated at four standard deviations and normalised to sum 1.
pub fn gaussian_psf(sigma_rows: f64, sigma_cols: f64) -> Result<Grid> {
    if !(sigma_rows.is_finite() && sigma_rows > 0.0 && sigma_cols.is_finite() && sigma_cols > 0.0) {
        return Err(invalid("the PSF standard deviations must be positive"));
    }
    let hr = (4.0 * sigma_rows).ceil() as usize;
    let hc = (4.0 * sigma_cols).ceil() as usize;
    if hr > 500 || hc > 500 {
        return Err(invalid("the PSF is too wide (standard deviation above 125 pixels)"));
    }
    let mut g = Grid::from_fn(2 * hr + 1, 2 * hc + 1, |r, c| {
        let (i, j) = (r as f64 - hr as f64, c as f64 - hc as f64);
        (-(i * i / (2.0 * sigma_rows * sigma_rows) + j * j / (2.0 * sigma_cols * sigma_cols))).exp()
    });
    let s: f64 = g.data.iter().sum();
    for v in &mut g.data {
        *v /= s;
    }
    Ok(g)
}

/// The PSF placed in a `rows × cols` circular buffer with its centre at
/// `(0, 0)`, transformed.
fn transfer(h: &Grid, rows: usize, cols: usize) -> ComplexGrid {
    let (cr, cc) = ((h.rows - 1) / 2, (h.cols - 1) / 2);
    let mut data = vec![Complex::ZERO; rows * cols];
    for i in 0..h.rows {
        let r = (i + rows - cr % rows) % rows;
        for j in 0..h.cols {
            let c = (j + cols - cc % cols) % cols;
            data[r * cols + c] += Complex::real(h.get(i, j));
        }
    }
    fft2(&ComplexGrid { rows, cols, data })
}

fn check_psf(image: &Grid, psf: &Grid) -> Result<()> {
    image.check("image")?;
    psf.check("point-spread function")?;
    if psf.rows > image.rows || psf.cols > image.cols {
        return Err(invalid("the point-spread function must not be larger than the image"));
    }
    Ok(())
}

/// Wiener (or constrained least-squares) deconvolution of `image` by `psf`.
pub fn wiener2(image: &Grid, psf: &Grid, regularizer: Regularizer) -> Result<Grid> {
    check_psf(image, psf)?;
    let lambda = match regularizer {
        Regularizer::Constant(l) | Regularizer::Laplacian(l) => l,
    };
    if !(lambda.is_finite() && lambda >= 0.0) {
        return Err(invalid("the regularization weight must be zero or positive"));
    }
    let (rows, cols) = (image.rows, image.cols);
    let fy = fft2(&image.to_complex());
    let fh = transfer(psf, rows, cols);
    let reg: Vec<f64> = match regularizer {
        Regularizer::Constant(_) => vec![1.0; rows * cols],
        Regularizer::Laplacian(_) => {
            let lap = Grid::new(3, 3, vec![0.0, -1.0, 0.0, -1.0, 4.0, -1.0, 0.0, -1.0, 0.0])?;
            if rows < 3 || cols < 3 {
                return Err(invalid("the Laplacian regularizer needs an image at least 3 × 3"));
            }
            transfer(&lap, rows, cols).data.iter().map(|z| z.norm_sqr()).collect()
        }
    };
    let out = ComplexGrid {
        rows,
        cols,
        data: fy
            .data
            .iter()
            .zip(&fh.data)
            .zip(&reg)
            .map(|((&y, &h), &l)| {
                let d = h.norm_sqr() + lambda * l;
                if d > 0.0 { y * h.conj() / d } else { Complex::ZERO }
            })
            .collect(),
    };
    Ok(image.like(ifft2(&out).data.iter().map(|z| z.re).collect()))
}

/// Repeated *same*-mode linear convolutions with one fixed kernel, through
/// zero-padded FFTs with the kernel's transform computed once.
struct SameConvolver {
    rows: usize,
    cols: usize,
    pr: usize,
    pc: usize,
    start_r: usize,
    start_c: usize,
    kernel: ComplexGrid,
}

impl SameConvolver {
    fn new(rows: usize, cols: usize, k: &Grid) -> Self {
        let (pr, pc) = (fast_length(rows + k.rows - 1), fast_length(cols + k.cols - 1));
        let kernel = rfft2(&pad(k, Padding::Zero { rows: pr, cols: pc }).expect("padded size is larger"));
        SameConvolver { rows, cols, pr, pc, start_r: (k.rows - 1) / 2, start_c: (k.cols - 1) / 2, kernel }
    }

    fn apply(&self, x: &Grid) -> Grid {
        let mut f = rfft2(&pad(x, Padding::Zero { rows: self.pr, cols: self.pc }).expect("padded size is larger"));
        for (a, &b) in f.data.iter_mut().zip(&self.kernel.data) {
            *a *= b;
        }
        let back = irfft2(&f, self.pc).crop(self.start_r, self.start_c, self.rows, self.cols);
        x.like(back.data)
    }
}

/// Richardson–Lucy deconvolution of a non-negative image by a non-negative
/// PSF for `iterations` steps (1–10 000).
pub fn richardson_lucy2(image: &Grid, psf: &Grid, iterations: usize) -> Result<Grid> {
    check_psf(image, psf)?;
    if image.data.iter().chain(&psf.data).any(|&v| v < 0.0) {
        return Err(invalid("Richardson–Lucy needs a non-negative image and point-spread function"));
    }
    let sum: f64 = psf.data.iter().sum();
    if sum <= 0.0 {
        return Err(invalid("the point-spread function must have a positive sum"));
    }
    if !(1..=10_000).contains(&iterations) {
        return Err(invalid("use between 1 and 10 000 iterations"));
    }
    let h = psf.map(|v| v / sum);
    let (rows, cols) = (image.rows, image.cols);
    let blur = SameConvolver::new(rows, cols, &h);
    let mirror = SameConvolver::new(rows, cols, &h.flipped());
    let mut est = image.like(vec![0.5; rows * cols]);
    for _ in 0..iterations {
        let conv = blur.apply(&est);
        let rel = image.like(image.data.iter().zip(&conv.data).map(|(&y, &c)| y / (c + 1e-12)).collect());
        let corr = mirror.apply(&rel);
        for (e, c) in est.data.iter_mut().zip(&corr.data) {
            *e *= c;
        }
    }
    Ok(est)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image2d::convolve::{ConvolutionMethod, ConvolutionMode, convolve2};

    fn scene() -> Grid {
        Grid::from_fn(24, 30, |r, c| {
            let block = if (8..14).contains(&r) && (10..20).contains(&c) { 1.0 } else { 0.1 };
            block + 0.3 * (-(((r as f64) - 5.0).powi(2) + ((c as f64) - 24.0).powi(2)) / 3.0).exp()
        })
    }

    #[test]
    fn gaussian_psf_is_normalised_and_centred() {
        let h = gaussian_psf(1.0, 2.0).unwrap();
        assert_eq!((h.rows, h.cols), (9, 17));
        assert!((h.data.iter().sum::<f64>() - 1.0).abs() < 1e-14);
        assert!(h.data.iter().all(|&v| v <= h.get(4, 8)));
        assert!(gaussian_psf(0.0, 1.0).is_err());
    }

    #[test]
    fn wiener_without_regularization_inverts_circular_blur() {
        let x = scene();
        let h = Grid::new(3, 3, vec![0.05, 0.1, 0.05, 0.1, 0.4, 0.1, 0.05, 0.1, 0.05]).unwrap();
        let fh = transfer(&h, x.rows, x.cols);
        let fx = fft2(&x.to_complex());
        let y = ComplexGrid {
            rows: x.rows,
            cols: x.cols,
            data: fx.data.iter().zip(&fh.data).map(|(&a, &b)| a * b).collect(),
        };
        let y = x.like(ifft2(&y).data.iter().map(|z| z.re).collect());
        let back = wiener2(&y, &h, Regularizer::Constant(0.0)).unwrap();
        for (a, b) in back.data.iter().zip(&x.data) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn richardson_lucy_sharpens_a_blurred_scene() {
        let x = scene();
        let h = gaussian_psf(1.2, 1.2).unwrap();
        let y = convolve2(&x, &h, ConvolutionMode::Same, ConvolutionMethod::Direct).unwrap();
        let est = richardson_lucy2(&y, &h, 30).unwrap();
        // Away from the borders, where the zero padding of the blur darkens both.
        let err = |g: &Grid| {
            let mut s = 0.0;
            for r in 5..19 {
                for c in 5..25 {
                    s += (g.get(r, c) - x.get(r, c)).powi(2);
                }
            }
            s
        };
        assert!(err(&est) < 0.6 * err(&y), "{} vs {}", err(&est), err(&y));
        let mut neg = y.clone();
        neg.data[0] = -1.0;
        assert!(richardson_lucy2(&neg, &h, 5).is_err());
    }
}
