//! Amplitude, phase and power spectra; Welch's averaged periodogram; the
//! short-time Fourier transform (spectrogram).
//!
//! # Definitions
//! A signal `x₀, …, x_{n−1}` sampled at rate `f_s` is detrended (nothing,
//! the mean, or the least-squares line removed), multiplied by a periodic
//! window `w` ([`crate::window`]) and zero-padded to `N ≥ n` points before
//! its DFT `X_k` ([`crate::fft`]); the frequencies are `f_k = k f_s / N`.
//!
//! * **Amplitude spectrum** `A_k = |X_k| / Σw` (two-sided); the one-sided
//!   spectrum doubles every term except zero and Nyquist, so a sinusoid of
//!   amplitude `a` on a bin shows height `a`.
//! * **Phase spectrum** `φ_k = arg X_k` in radians.
//! * **Power spectrum** (periodogram) `P_k = |X_k|² · s`, with
//!   `s = 1/(f_s Σw²)` for *density* scaling (units²/Hz; integrates to the
//!   variance) or `s = 1/(Σw)²` for *spectrum* scaling (units²; a sinusoid
//!   of amplitude `a` shows `a²/2` one-sided). One-sided power doubles every
//!   term except zero and Nyquist (Nyquist exists only for even `N`).
//!
//! **Welch's method** (Welch 1967, "The use of fast Fourier transform for
//! the estimation of power spectra", *IEEE Trans. Audio Electroacoustics*
//! 15(2)) splits the signal into segments of `L` points overlapping by `D`
//! (step `L − D`; `⌊(n − D)/(L − D)⌋` segments, trailing samples that do not
//! fill a segment are dropped), computes each segment's periodogram as
//! above (detrended per segment) and averages them. The **spectrogram**
//! keeps the per-segment periodograms instead, timed at the segment
//! centres `(L/2 + j(L − D))/f_s`.
//!
//! These follow the conventions of SciPy's `scipy.signal.periodogram`,
//! `welch` and `spectrogram` (with `boundary=None, padded=False`), against
//! which `tests/spectrum_reference.rs` checks them.

use crate::complex::Complex;
use crate::fft::{FftPlan, fft_frequencies, rfft_frequencies};
use crate::window::Window;
use crate::{Result, StatError};

/// Trend removed from the signal (or from each segment) before the transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detrend {
    /// Use the values as they are.
    None,
    /// Subtract the mean.
    Constant,
    /// Subtract the least-squares straight line.
    Linear,
}

/// Which frequencies are reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sides {
    /// Frequencies `0 … f_s/2` with the power of negative frequencies folded in.
    One,
    /// All `N` frequencies, in transform order (`0, …, positive, negative, …`).
    Two,
}

/// Units of a power spectrum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaling {
    /// Power spectral density, units²/Hz.
    Density,
    /// Power spectrum, units² (sinusoid power on its bin).
    Spectrum,
}

/// Settings shared by the single-transform spectra.
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumOptions {
    /// Sampling rate `f_s` (samples per unit of time); frequencies are in
    /// cycles per that unit. Default 1.
    pub sampling_rate: f64,
    /// Window (default [`Window::Rectangular`] for a single periodogram).
    pub window: Window,
    /// Trend removal (default [`Detrend::Constant`]).
    pub detrend: Detrend,
    /// Transform length `N ≥ n` (zero padding); `None` uses `n`.
    pub fft_length: Option<usize>,
    /// One- or two-sided output (default one-sided).
    pub sides: Sides,
    /// Power scaling (default density).
    pub scaling: Scaling,
}

impl Default for SpectrumOptions {
    fn default() -> Self {
        SpectrumOptions {
            sampling_rate: 1.0,
            window: Window::Rectangular,
            detrend: Detrend::Constant,
            fft_length: None,
            sides: Sides::One,
            scaling: Scaling::Density,
        }
    }
}

/// Amplitude, phase and power of one transform.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    /// Frequencies `f_k`.
    pub frequency: Vec<f64>,
    /// Amplitude spectrum `A_k`.
    pub amplitude: Vec<f64>,
    /// Phase `arg X_k` in radians, `(−π, π]`.
    pub phase: Vec<f64>,
    /// Power (periodogram) with the requested scaling.
    pub power: Vec<f64>,
    /// The complex transform terms `X_k` of the windowed signal (same
    /// frequencies as above).
    pub transform: Vec<Complex>,
    /// Transform length `N`.
    pub fft_length: usize,
    /// Frequency resolution `f_s / N`.
    pub resolution: f64,
}

pub(crate) fn check_signal(x: &[f64], min: usize) -> Result<()> {
    if x.len() < min {
        return Err(StatError::InsufficientData { required: min, found: x.len() });
    }
    if x.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "signal" });
    }
    Ok(())
}

fn check_rate(fs: f64) -> Result<()> {
    if !(fs.is_finite() && fs > 0.0) {
        return Err(StatError::InvalidArgument { message: "the sampling rate must be a positive number".into() });
    }
    Ok(())
}

/// Remove the requested trend in place.
pub fn detrend(x: &mut [f64], how: Detrend) {
    let n = x.len();
    if n == 0 {
        return;
    }
    match how {
        Detrend::None => {}
        Detrend::Constant => {
            let mean = x.iter().sum::<f64>() / n as f64;
            x.iter_mut().for_each(|v| *v -= mean);
        }
        Detrend::Linear => {
            if n == 1 {
                x[0] = 0.0;
                return;
            }
            let tm = (n - 1) as f64 / 2.0;
            let ym = x.iter().sum::<f64>() / n as f64;
            let (mut sxy, mut sxx) = (0.0, 0.0);
            for (t, &v) in x.iter().enumerate() {
                let d = t as f64 - tm;
                sxy += d * (v - ym);
                sxx += d * d;
            }
            let b = sxy / sxx;
            for (t, v) in x.iter_mut().enumerate() {
                *v -= ym + b * (t as f64 - tm);
            }
        }
    }
}

/// Fold a two-sided power vector (transform order, length `nfft`) to
/// one-sided, doubling all terms but zero and Nyquist.
fn one_sided(power: &[f64], nfft: usize) -> Vec<f64> {
    let half = nfft / 2;
    let mut out = power[..=half].to_vec();
    let last = if nfft % 2 == 0 { half } else { half + 1 };
    for v in out.iter_mut().take(last).skip(1) {
        *v *= 2.0;
    }
    out
}

/// Amplitude, phase and power spectrum of the whole signal (one transform).
pub fn spectrum(x: &[f64], opts: &SpectrumOptions) -> Result<Spectrum> {
    check_signal(x, 2)?;
    check_rate(opts.sampling_rate)?;
    let n = x.len();
    let nfft = opts.fft_length.unwrap_or(n);
    if nfft < n {
        return Err(StatError::InvalidArgument {
            message: "the transform length must be at least the number of samples".into(),
        });
    }
    let w = opts.window.periodic(n)?;
    let plan = FftPlan::new(nfft);
    let (raw, sum_w, sum_w2) = windowed_transform(x, &w, opts.detrend, &plan);
    let fs = opts.sampling_rate;
    let scale = match opts.scaling {
        Scaling::Density => 1.0 / (fs * sum_w2),
        Scaling::Spectrum => 1.0 / (sum_w * sum_w),
    };
    let power2: Vec<f64> = raw.iter().map(|z| z.norm_sqr() * scale).collect();
    let amp2: Vec<f64> = raw.iter().map(|z| z.abs() / sum_w).collect();
    let (frequency, power, amplitude, transform) = match opts.sides {
        Sides::Two => (fft_frequencies(nfft, 1.0 / fs), power2, amp2, raw),
        Sides::One => {
            let half = nfft / 2;
            let amp = one_sided(&amp2, nfft);
            (rfft_frequencies(nfft, 1.0 / fs), one_sided(&power2, nfft), amp, raw[..=half].to_vec())
        }
    };
    let phase = transform.iter().map(|z| z.arg()).collect();
    Ok(Spectrum { frequency, amplitude, phase, power, transform, fft_length: nfft, resolution: fs / nfft as f64 })
}

/// Detrend, window and transform one segment: returns `(X, Σw, Σw²)`.
fn windowed_transform(seg: &[f64], w: &[f64], how: Detrend, plan: &FftPlan) -> (Vec<Complex>, f64, f64) {
    let mut s = seg.to_vec();
    detrend(&mut s, how);
    let mut buf = vec![Complex::ZERO; plan.len()];
    for ((b, &v), &wi) in buf.iter_mut().zip(&s).zip(w) {
        *b = Complex::real(v * wi);
    }
    let sum_w: f64 = w.iter().sum();
    let sum_w2: f64 = w.iter().map(|v| v * v).sum();
    (plan.forward(&buf), sum_w, sum_w2)
}

/// Settings of [`welch`] and [`spectrogram`].
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentOptions {
    /// Sampling rate `f_s` (default 1).
    pub sampling_rate: f64,
    /// Window applied to each segment (Welch default Hann; spectrogram
    /// default Tukey with α = 0.25).
    pub window: Window,
    /// Segment length `L` (default 256, or `n` when the signal is shorter).
    pub segment_length: usize,
    /// Overlap `D < L` in samples; `None` uses `L/2` (Welch) or `L/8`
    /// (spectrogram).
    pub overlap: Option<usize>,
    /// Transform length per segment (`≥ L`); `None` uses `L`.
    pub fft_length: Option<usize>,
    /// Trend removed from each segment (default the mean).
    pub detrend: Detrend,
    /// One- or two-sided output.
    pub sides: Sides,
    /// Power scaling.
    pub scaling: Scaling,
}

impl SegmentOptions {
    /// Welch defaults: Hann window, 256-point segments, half overlap,
    /// density scaling, one-sided.
    pub fn welch() -> Self {
        SegmentOptions {
            sampling_rate: 1.0,
            window: Window::Hann,
            segment_length: 256,
            overlap: None,
            fft_length: None,
            detrend: Detrend::Constant,
            sides: Sides::One,
            scaling: Scaling::Density,
        }
    }

    /// Spectrogram defaults: Tukey (α = 0.25) window, 256-point segments,
    /// one-eighth overlap.
    pub fn spectrogram() -> Self {
        SegmentOptions { window: Window::Tukey { alpha: 0.25 }, ..SegmentOptions::welch() }
    }
}

/// Welch's averaged periodogram.
#[derive(Debug, Clone, PartialEq)]
pub struct WelchSpectrum {
    /// Frequencies.
    pub frequency: Vec<f64>,
    /// Averaged power.
    pub power: Vec<f64>,
    /// Number of segments averaged.
    pub segments: usize,
    /// Segment length actually used.
    pub segment_length: usize,
    /// Overlap actually used.
    pub overlap: usize,
    /// Frequency resolution `f_s / N`.
    pub resolution: f64,
}

/// Power of each segment, as a time–frequency grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrogram {
    /// Frequencies (rows of [`Spectrogram::power`]' inner vectors).
    pub frequency: Vec<f64>,
    /// Segment centre times.
    pub time: Vec<f64>,
    /// `power[j][k]`: power of segment `j` at frequency `k`.
    pub power: Vec<Vec<f64>>,
    /// Segment length used.
    pub segment_length: usize,
    /// Overlap used.
    pub overlap: usize,
}

struct Segmented {
    frequency: Vec<f64>,
    power: Vec<Vec<f64>>,
    length: usize,
    overlap: usize,
    nfft: usize,
}

fn segment_powers(x: &[f64], opts: &SegmentOptions, default_overlap_divisor: usize) -> Result<Segmented> {
    check_signal(x, 2)?;
    check_rate(opts.sampling_rate)?;
    let n = x.len();
    if opts.segment_length < 2 {
        return Err(StatError::InvalidArgument { message: "segments need at least 2 samples".into() });
    }
    let length = opts.segment_length.min(n);
    let overlap = opts.overlap.unwrap_or(length / default_overlap_divisor);
    if overlap >= length {
        return Err(StatError::InvalidArgument {
            message: "the overlap must be smaller than the segment length".into(),
        });
    }
    let nfft = opts.fft_length.unwrap_or(length);
    if nfft < length {
        return Err(StatError::InvalidArgument {
            message: "the transform length must be at least the segment length".into(),
        });
    }
    let w = opts.window.periodic(length)?;
    let plan = FftPlan::new(nfft);
    let step = length - overlap;
    let count = (n - overlap) / step;
    let fs = opts.sampling_rate;
    let mut power = Vec::with_capacity(count);
    for j in 0..count {
        let seg = &x[j * step..j * step + length];
        let (raw, sum_w, sum_w2) = windowed_transform(seg, &w, opts.detrend, &plan);
        let scale = match opts.scaling {
            Scaling::Density => 1.0 / (fs * sum_w2),
            Scaling::Spectrum => 1.0 / (sum_w * sum_w),
        };
        let p: Vec<f64> = raw.iter().map(|z| z.norm_sqr() * scale).collect();
        power.push(match opts.sides {
            Sides::Two => p,
            Sides::One => one_sided(&p, nfft),
        });
    }
    let frequency = match opts.sides {
        Sides::Two => fft_frequencies(nfft, 1.0 / fs),
        Sides::One => rfft_frequencies(nfft, 1.0 / fs),
    };
    Ok(Segmented { frequency, power, length, overlap, nfft })
}

/// Welch's averaged, modified periodogram (mean of the segment periodograms).
pub fn welch(x: &[f64], opts: &SegmentOptions) -> Result<WelchSpectrum> {
    let s = segment_powers(x, opts, 2)?;
    let count = s.power.len();
    let mut power = vec![0.0; s.frequency.len()];
    for p in &s.power {
        for (a, b) in power.iter_mut().zip(p) {
            *a += b;
        }
    }
    power.iter_mut().for_each(|v| *v /= count as f64);
    Ok(WelchSpectrum {
        frequency: s.frequency,
        power,
        segments: count,
        segment_length: s.length,
        overlap: s.overlap,
        resolution: opts.sampling_rate / s.nfft as f64,
    })
}

/// Short-time Fourier transform power (spectrogram).
pub fn spectrogram(x: &[f64], opts: &SegmentOptions) -> Result<Spectrogram> {
    let s = segment_powers(x, opts, 8)?;
    let step = s.length - s.overlap;
    let fs = opts.sampling_rate;
    let time = (0..s.power.len()).map(|j| (s.length as f64 / 2.0 + (j * step) as f64) / fs).collect();
    Ok(Spectrogram { frequency: s.frequency, time, power: s.power, segment_length: s.length, overlap: s.overlap })
}

/// Unwrap a phase sequence: add multiples of 2π so successive differences
/// lie in `[−π, π]` (as `numpy.unwrap`).
pub fn unwrap_phase(phase: &[f64]) -> Vec<f64> {
    use std::f64::consts::PI;
    let mut out = Vec::with_capacity(phase.len());
    let mut offset = 0.0;
    for (i, &p) in phase.iter().enumerate() {
        if i > 0 {
            let d = p - phase[i - 1];
            // numpy: ddmod = mod(d + π, 2π) − π, with −π kept for d > 0.
            let mut dd = (d + PI).rem_euclid(2.0 * PI) - PI;
            if dd == -PI && d > 0.0 {
                dd = PI;
            }
            if d.abs() >= PI {
                offset += dd - d;
            }
        }
        out.push(p + offset);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn sinusoid_on_a_bin_has_its_amplitude() {
        let n = 64;
        let x: Vec<f64> = (0..n).map(|t| 3.0 * (2.0 * PI * 8.0 * t as f64 / n as f64).cos()).collect();
        let s = spectrum(&x, &SpectrumOptions { scaling: Scaling::Spectrum, ..Default::default() }).unwrap();
        assert!((s.amplitude[8] - 3.0).abs() < 1e-12);
        assert!((s.power[8] - 4.5).abs() < 1e-12);
        assert!(s.amplitude[3].abs() < 1e-12);
    }

    #[test]
    fn density_integrates_to_the_variance() {
        let x: Vec<f64> = (0..200).map(|t| ((t * 37 % 101) as f64 / 101.0) - 0.5).collect();
        let s = spectrum(&x, &SpectrumOptions { sampling_rate: 10.0, ..Default::default() }).unwrap();
        let mean = x.iter().sum::<f64>() / 200.0;
        let var = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 200.0;
        let area: f64 = s.power.iter().sum::<f64>() * s.resolution;
        assert!((area - var).abs() < 1e-12);
    }

    #[test]
    fn unwrap_removes_jumps() {
        let p = [0.0, 3.0, -3.0, -0.5];
        let u = unwrap_phase(&p);
        assert!((u[2] - (2.0 * PI - 3.0)).abs() < 1e-12);
    }
}
