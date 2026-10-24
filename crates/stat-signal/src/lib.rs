//! # stat-signal
//!
//! Signals and spectra: sequences of values `yᵢ` measured at positions `xᵢ`
//! (time, wavenumber, retention time), usually evenly spaced.
//!
//! * [`smooth`] — smoothing filters: Savitzky–Golay polynomial smoothing and
//!   derivatives, centred and trailing moving averages, exponential
//!   smoothing, running medians and percentiles, and the Gaussian kernel.
//! * [`calculus`] — derivatives (central differences), cumulative integrals
//!   (trapezoid and Simpson) and interpolation (linear, cubic spline, PCHIP)
//!   of x–y data, and resampling onto a new grid.
//! * [`baseline`] — baseline (background) estimation for spectra:
//!   asymmetric least squares, asymmetrically reweighted penalized least
//!   squares (arPLS) and the SNIP clipping filter.
//! * [`peaks`] — peak finding: local maxima with their prominence, width
//!   and bases, filtered by height, prominence and separation, and a robust
//!   estimate of the noise level.
//! * [`profiles`] — peak shapes: Gaussian, Lorentzian, pseudo-Voigt, Voigt
//!   (through the Faddeeva function) and split (asymmetric) Gaussian and
//!   Lorentzian profiles, with their heights, widths and areas.
//! * [`peak_fit`] — least-squares fits of a sum of peaks plus a baseline,
//!   with shared widths or spacings, standard errors of each peak's centre,
//!   height, width and area, and comparison of numbers of peaks.
//!
//! * [`fft`] — fast Fourier transforms of any length (mixed radix and
//!   Bluestein), real transforms and frequency grids.
//! * [`window`] — rectangular, Hann, Hamming, Blackman, flat-top, Kaiser and
//!   Tukey windows.
//! * [`spectrum`] — amplitude, phase and power spectra, Welch's averaged
//!   periodogram and the spectrogram.
//! * [`filter`] — Butterworth, Chebyshev I/II and elliptic IIR filters in
//!   second-order sections, windowed-sinc FIR filters, notch filters,
//!   forward and zero-phase filtering, frequency responses and FFT filters.
//! * [`convolve`] — linear and circular convolution, correlation,
//!   Wiener and Richardson–Lucy deconvolution, and the Hilbert transform.
//! * [`image2d`] — two-dimensional Fourier analysis of images: 2-D
//!   transforms, centred spectra, radial and angular averages, dominant
//!   spatial frequencies, frequency-domain and notch filters, 2-D
//!   convolution and correlation, phase-correlation registration, and
//!   Wiener and Richardson–Lucy deconvolution.
//! * [`wavelet`] — discrete wavelet transforms (Haar, Daubechies, symlets,
//!   coiflets), multilevel decomposition, wavelet denoising and the
//!   continuous wavelet transform.
//!
//! The routines take plain slices without missing values; callers drop
//! missing rows first. Filters that need even spacing say so (the Fourier,
//! filter and wavelet routines take evenly spaced samples at a sampling
//! rate `f_s`; callers resample uneven x–y data first). Every routine is
//! checked against NumPy, SciPy, pandas, pybaselines, lmfit, PyWavelets or
//! scikit-image (`tests/`).

pub mod baseline;
mod banded;
pub mod calculus;
pub mod complex;
pub mod convolve;
pub mod fft;
pub mod filter;
pub mod image2d;
pub mod peak_fit;
pub mod peaks;
pub mod profiles;
pub mod smooth;
pub mod spectrum;
pub mod wavelet;
pub mod window;

pub use baseline::{BaselineMethod, arpls, asls, estimate_baseline, snip};
pub use calculus::{
    Integration, Interpolation, Interpolator, cumulative_integral, gradient, linear_grid, resample,
};
pub use peak_fit::{
    BaselineTerm, PeakCountComparison, PeakEstimate, PeakFit, PeakFitSpec, PeakStart, Spacing, compare_peak_counts,
    fit_peaks,
};
pub use peaks::{Peak, PeakSearch, find_peaks, noise_level};
pub use profiles::{PeakShape, faddeeva_real, voigt_profile};
pub use smooth::{MovingAverage, Smoother, savitzky_golay, savitzky_golay_coefficients, smooth};
pub use stat_core::error::{Result, StatError};

pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}
