//! Frequency-domain filters for images: low-, high-, band-pass and
//! band-reject masks, notch filters, and filtering by inverse transform.
//!
//! # Masks
//! With `D` the radial frequency of a cell (cycles per unit of `dx`), a
//! cut-off `D₀`, and for bands the centre `C₀ = (f₁ + f₂)/2` and width
//! `W = f₂ − f₁` of the band `[f₁, f₂]` (Gonzalez & Woods 2018, *Digital
//! Image Processing*, 4th ed., §4.8–4.10, Tables 4.6–4.7):
//!
//! | shape | low-pass `H_LP(D)` | band-reject `H_BR(D)` |
//! |---|---|---|
//! | ideal | `1` if `D ≤ D₀`, else `0` | `0` if `C₀ − W/2 ≤ D ≤ C₀ + W/2`, else `1` |
//! | Butterworth (order `n`) | `1 / (1 + (D/D₀)^{2n})` | `1 / (1 + (DW / (D² − C₀²))^{2n})` |
//! | Gaussian | `exp(−D² / (2D₀²))` | `1 − exp(−((D² − C₀²) / (DW))²)` |
//!
//! and `H_HP = 1 − H_LP`, `H_BP = 1 − H_BR`. The Gaussian low-pass with
//! `D₀ = 1/(2πσ)` is the transform of a Gaussian blur of standard deviation
//! `σ` pixels (SciPy's `ndimage.fourier_gaussian`); the Butterworth
//! low-pass with `D₀` in cycles per pixel is scikit-image's
//! `filters.butterworth` with `cutoff_frequency_ratio = D₀`.
//!
//! # Notch filters
//! A notch at frequency `(u, v)` removes a periodic pattern, which shows in
//! the spectrum of a real image as a conjugate pair of spikes at `(u, v)`
//! and `(−u, −v)`. The notch-reject mask is the product over notches of
//! high-pass masks centred on both members of each pair,
//! `H_NR = Π_k H_HP(D_k) H_HP(D_{−k})` with `D_{±k}` the distances from
//! `(±u_k, ±v_k)` and the notch radius as cut-off; the notch-pass mask is
//! `1 − H_NR` (Gonzalez & Woods 2018, §4.10, eq. 4-150).
//!
//! # Filtering
//! The image is padded ([`Padding`]; mirror padding avoids ringing at the
//! borders), transformed with [`rfft2`], multiplied by the mask, inverted
//! with [`irfft2`] and cropped back to its original size. The masks are
//! real and symmetric through the origin, so the result is real and has no
//! phase shift. The *removed* part is the original minus the filtered
//! image.

use super::Grid;
use super::transform::{Padding, fft_shift2, frequency_axes, irfft2, pad, rfft2};
use crate::filter::Band;
use crate::{Result, invalid};

/// The transition shape of a frequency-domain mask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterShape {
    /// A sharp cut-off (1 inside, 0 outside); rings in the image.
    Ideal,
    /// Butterworth roll-off of the given order (0.5–20; higher is sharper).
    Butterworth {
        /// The order `n`.
        order: f64,
    },
    /// Gaussian roll-off (no ringing).
    Gaussian,
}

impl FilterShape {
    /// Short display name.
    pub fn label(&self) -> String {
        match self {
            FilterShape::Ideal => "Ideal".into(),
            FilterShape::Butterworth { order } => format!("Butterworth (order {order})"),
            FilterShape::Gaussian => "Gaussian".into(),
        }
    }

    fn check(&self) -> Result<()> {
        match *self {
            FilterShape::Butterworth { order } if !(order.is_finite() && (0.5..=20.0).contains(&order)) => {
                Err(invalid("the Butterworth order must be between 0.5 and 20"))
            }
            _ => Ok(()),
        }
    }

    fn lowpass(&self, d: f64, d0: f64) -> f64 {
        match *self {
            FilterShape::Ideal => {
                if d <= d0 {
                    1.0
                } else {
                    0.0
                }
            }
            FilterShape::Butterworth { order } => 1.0 / (1.0 + (d / d0).powf(2.0 * order)),
            FilterShape::Gaussian => (-d * d / (2.0 * d0 * d0)).exp(),
        }
    }

    fn bandreject(&self, d: f64, c0: f64, w: f64) -> f64 {
        match *self {
            FilterShape::Ideal => {
                if (c0 - w / 2.0..=c0 + w / 2.0).contains(&d) {
                    0.0
                } else {
                    1.0
                }
            }
            FilterShape::Butterworth { order } => {
                let den = d * d - c0 * c0;
                if den == 0.0 { 0.0 } else { 1.0 / (1.0 + (d * w / den).abs().powf(2.0 * order)) }
            }
            FilterShape::Gaussian => {
                if d == 0.0 {
                    1.0
                } else {
                    1.0 - (-((d * d - c0 * c0) / (d * w)).powi(2)).exp()
                }
            }
        }
    }

    /// The gain at radial frequency `d` for `band`.
    pub fn gain(&self, band: Band, d: f64) -> f64 {
        match band {
            Band::Lowpass(f) => self.lowpass(d, f),
            Band::Highpass(f) => 1.0 - self.lowpass(d, f),
            Band::Bandstop(a, b) => self.bandreject(d, (a + b) / 2.0, b - a),
            Band::Bandpass(a, b) => 1.0 - self.bandreject(d, (a + b) / 2.0, b - a),
        }
    }
}

fn check_band(band: Band) -> Result<()> {
    let ok = |f: f64| f.is_finite() && f > 0.0;
    match band {
        Band::Lowpass(f) | Band::Highpass(f) if ok(f) => Ok(()),
        Band::Bandpass(a, b) | Band::Bandstop(a, b) if ok(a) && ok(b) && a < b => Ok(()),
        _ => Err(invalid("cut-off frequencies must be positive, with the lower edge below the upper")),
    }
}

/// A notch at frequency `(fx, fy)` (with its conjugate at `(−fx, −fy)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Notch {
    /// Horizontal frequency (cycles per unit of `dx`).
    pub fx: f64,
    /// Vertical frequency (cycles per unit of `dy`).
    pub fy: f64,
    /// Radius of the notch (its cut-off), in the same units.
    pub radius: f64,
}

/// The `rows × cols` mask for `band` in transform order (zero frequency at
/// cell `(0, 0)`; centre it with [`fft_shift2`] for display).
pub fn band_mask(rows: usize, cols: usize, dx: f64, dy: f64, band: Band, shape: FilterShape) -> Result<Grid> {
    check_band(band)?;
    shape.check()?;
    let (fx, fy) = frequency_axes(rows, cols, dx, dy);
    Ok(Grid::from_fn(rows, cols, |r, c| shape.gain(band, fx[c].hypot(fy[r]))))
}

/// The notch-reject mask (or notch-pass when `pass`) for `notches`, in
/// transform order.
pub fn notch_mask(
    rows: usize,
    cols: usize,
    dx: f64,
    dy: f64,
    notches: &[Notch],
    shape: FilterShape,
    pass: bool,
) -> Result<Grid> {
    shape.check()?;
    if notches.iter().any(|n| !(n.fx.is_finite() && n.fy.is_finite() && n.radius.is_finite() && n.radius > 0.0)) {
        return Err(invalid("each notch needs a finite frequency and a positive radius"));
    }
    let (fx, fy) = frequency_axes(rows, cols, dx, dy);
    Ok(Grid::from_fn(rows, cols, |r, c| {
        let reject: f64 = notches
            .iter()
            .map(|n| {
                let d1 = (fx[c] - n.fx).hypot(fy[r] - n.fy);
                let d2 = (fx[c] + n.fx).hypot(fy[r] + n.fy);
                (1.0 - shape.lowpass(d1, n.radius)) * (1.0 - shape.lowpass(d2, n.radius))
            })
            .product();
        if pass { 1.0 - reject } else { reject }
    }))
}

/// A frequency-domain filter: an optional band mask times an optional set
/// of notches.
#[derive(Debug, Clone, PartialEq)]
pub struct FrequencyFilter {
    /// Low-, high-, band-pass or band-reject mask and its shape.
    pub band: Option<(Band, FilterShape)>,
    /// Notches (each with its conjugate).
    pub notches: Vec<Notch>,
    /// Shape of the notches.
    pub notch_shape: FilterShape,
    /// Keep only the notched frequencies instead of removing them.
    pub notch_pass: bool,
    /// Padding before the transform.
    pub padding: Padding,
}

impl Default for FrequencyFilter {
    fn default() -> Self {
        FrequencyFilter {
            band: None,
            notches: Vec::new(),
            notch_shape: FilterShape::Gaussian,
            notch_pass: false,
            padding: Padding::None,
        }
    }
}

impl FrequencyFilter {
    /// The combined mask at size `rows × cols`, in transform order.
    pub fn mask(&self, rows: usize, cols: usize, dx: f64, dy: f64) -> Result<Grid> {
        let mut m = Grid { rows, cols, data: vec![1.0; rows * cols], dx: 1.0, dy: 1.0 };
        if let Some((band, shape)) = self.band {
            m = band_mask(rows, cols, dx, dy, band, shape)?;
        }
        if !self.notches.is_empty() {
            let n = notch_mask(rows, cols, dx, dy, &self.notches, self.notch_shape, self.notch_pass)?;
            for (a, b) in m.data.iter_mut().zip(&n.data) {
                *a *= b;
            }
        }
        Ok(m)
    }
}

/// The result of [`frequency_filter`].
#[derive(Debug, Clone, PartialEq)]
pub struct FilteredImage {
    /// The filtered image (same size as the input).
    pub filtered: Grid,
    /// The part removed: input − filtered.
    pub removed: Grid,
    /// The mask used, centred (zero frequency in the middle), at the padded size.
    pub mask: Grid,
}

/// Filter an image by multiplying its transform by `filter`'s mask.
pub fn frequency_filter(image: &Grid, filter: &FrequencyFilter) -> Result<FilteredImage> {
    image.check("image")?;
    let padded = pad(image, filter.padding)?;
    let (rows, cols) = (padded.rows, padded.cols);
    let mask = filter.mask(rows, cols, image.dx, image.dy)?;
    let mut spec = rfft2(&padded);
    let half = spec.cols;
    for r in 0..rows {
        for c in 0..half {
            spec.data[r * half + c] = spec.data[r * half + c] * mask.get(r, c);
        }
    }
    let back = irfft2(&spec, cols);
    let filtered = back.crop(0, 0, image.rows, image.cols);
    let filtered = image.like(filtered.data);
    let removed = image.sub(&filtered);
    let centred = Grid {
        rows,
        cols,
        data: fft_shift2(&mask.data, rows, cols),
        dx: 1.0 / (cols as f64 * image.dx),
        dy: 1.0 / (rows as f64 * image.dy),
    };
    Ok(FilteredImage { filtered, removed, mask: centred })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn complementary_masks_sum_to_one() {
        for shape in [FilterShape::Ideal, FilterShape::Butterworth { order: 2.0 }, FilterShape::Gaussian] {
            let lp = band_mask(9, 12, 1.0, 1.0, Band::Lowpass(0.2), shape).unwrap();
            let hp = band_mask(9, 12, 1.0, 1.0, Band::Highpass(0.2), shape).unwrap();
            let bp = band_mask(9, 12, 1.0, 1.0, Band::Bandpass(0.1, 0.3), shape).unwrap();
            let br = band_mask(9, 12, 1.0, 1.0, Band::Bandstop(0.1, 0.3), shape).unwrap();
            for i in 0..lp.len() {
                assert!((lp.data[i] + hp.data[i] - 1.0).abs() < 1e-15);
                assert!((bp.data[i] + br.data[i] - 1.0).abs() < 1e-15);
            }
            assert_eq!(lp.data[0], 1.0);
            assert_eq!(br.data[0], 1.0);
        }
        assert!(band_mask(4, 4, 1.0, 1.0, Band::Bandpass(0.3, 0.1), FilterShape::Ideal).is_err());
        assert!(band_mask(4, 4, 1.0, 1.0, Band::Lowpass(0.1), FilterShape::Butterworth { order: 0.0 }).is_err());
    }

    #[test]
    fn notch_removes_a_periodic_pattern() {
        let (rows, cols) = (40, 50);
        let base = Grid::from_fn(rows, cols, |r, c| (r as f64 - 20.0).powi(2) * 0.01 + c as f64 * 0.02);
        let pattern =
            Grid::from_fn(rows, cols, |r, c| (2.0 * PI * (5.0 * c as f64 / 50.0 + 4.0 * r as f64 / 40.0)).cos());
        let image = base.like(base.data.iter().zip(&pattern.data).map(|(a, b)| a + b).collect());
        let filter = FrequencyFilter {
            notches: vec![Notch { fx: 0.1, fy: 0.1, radius: 0.004 }],
            notch_shape: FilterShape::Ideal,
            ..Default::default()
        };
        let out = frequency_filter(&image, &filter).unwrap();
        // The pattern is exactly on a bin, so the ideal notch removes it and nothing else
        // except the base image's own content at those two cells.
        let err: f64 = out.removed.data.iter().zip(&pattern.data).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(err < 0.05, "{err}");
        let pass = FrequencyFilter { notch_pass: true, ..filter };
        let kept = frequency_filter(&image, &pass).unwrap();
        for i in 0..image.len() {
            assert!((kept.filtered.data[i] + out.filtered.data[i] - image.data[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn mirror_padding_keeps_size_and_constant_images() {
        let image = Grid::from_fn(7, 9, |_, _| 2.5);
        let filter = FrequencyFilter {
            band: Some((Band::Lowpass(0.1), FilterShape::Gaussian)),
            padding: Padding::Mirror,
            ..Default::default()
        };
        let out = frequency_filter(&image, &filter).unwrap();
        assert_eq!((out.filtered.rows, out.filtered.cols), (7, 9));
        assert_eq!((out.mask.rows, out.mask.cols), (14, 18));
        assert!(out.filtered.data.iter().all(|v| (v - 2.5).abs() < 1e-12));
    }
}
