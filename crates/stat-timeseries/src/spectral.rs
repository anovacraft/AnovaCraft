//! Periodogram and smoothed spectral density estimates.
//!
//! # Method
//! The series `x₁, …, xₙ` is optionally detrended (mean removed, or the least
//! squares line on `t = 1, …, n` removed) and multiplied by a split cosine
//! bell taper that tapers a proportion `p` of the observations at each end
//! (`m = ⌊n p⌋` weights `½(1 − cos(π(2i − 1)/(2m)))`, `i = 1, …, m`, at the
//! start and mirrored at the end; Bloomfield 1976, *Fourier Analysis of Time
//! Series: An Introduction*, Wiley, §5.2). The raw periodogram at the Fourier
//! frequencies `ωⱼ = j/n` (cycles per observation), `j = 1, …, ⌊n/2⌋`, is
//!
//! ```text
//! I(ωⱼ) = |Σₜ xₜ e^{−2πi ωⱼ t}|² / (n · U₂),   U₂ = 1 − (5/4) p,
//! ```
//!
//! where `U₂` corrects for the power the taper removes. The scaling makes
//! `I` an estimate of the spectral density `f(ω)` with
//! `∫_{−½}^{½} f(ω) dω = var(x)` (a white-noise series of variance σ² has
//! `f ≡ σ²`). Frequencies are in cycles per observation; for a series with
//! `s` observations per unit of time (e.g. `s = 12` for monthly data and a
//! frequency in cycles per year) multiply the frequencies and the bandwidth
//! by `s` and divide the density and its limits by `s`.
//!
//! The smoothed estimate applies a symmetric kernel `{k₋M, …, k_M}`
//! circularly to the periodogram at all frequencies `j/n`, `j = 0, …, n − 1`
//! (the value at frequency zero, which the mean removal makes uninformative,
//! is first replaced by the mean of its two neighbours). The kernel is a
//! modified Daniell kernel or the convolution of several, one per entry of
//! `spans`: span `s` gives `m = ⌊s/2⌋` and weights `1/(2m)` for `|i| < m`
//! and `1/(4m)` at `|i| = m` (Bloomfield 1976, §7.3; Brockwell & Davis 1991,
//! *Time Series: Theory and Methods*, 2nd ed., Springer, §10.4).
//!
//! # Confidence limits
//! The estimate is approximately `f(ω) χ²_ν / ν` with equivalent degrees of
//! freedom `ν = 2 / (Σ kᵢ² · U₄/U₂²)`, `U₄ = 1 − (93/64) p` (2 for the raw,
//! untapered periodogram), so a `1 − α` interval is
//! `[ν f̂/χ²_ν(1 − α/2), ν f̂/χ²_ν(α/2)]`. The bandwidth (Bloomfield 1976)
//! is `√(Σ (1/12 + i²) kᵢ) / n` in cycles per observation.

use crate::trend::check_finite;
use crate::{Result, StatError};
use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use std::f64::consts::PI;

/// Trend removal before the periodogram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detrend {
    /// Use the series as it is.
    None,
    /// Subtract the mean.
    Mean,
    /// Subtract the least-squares line on `t = 1, …, n` (the default).
    Linear,
}

/// Settings of [`spectral_density`].
#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumOptions {
    /// Trend removal (default [`Detrend::Linear`]).
    pub detrend: Detrend,
    /// Proportion `p ∈ [0, 0.5]` tapered at each end by the split cosine bell
    /// (default 0.1).
    pub taper: f64,
    /// Spans of the modified Daniell kernels, each at least 2 (empty: the raw
    /// periodogram).
    pub spans: Vec<usize>,
    /// Coverage of the confidence limits (default 0.95).
    pub level: f64,
}

impl Default for SpectrumOptions {
    fn default() -> Self {
        Self { detrend: Detrend::Linear, taper: 0.1, spans: Vec::new(), level: 0.95 }
    }
}

/// A spectral density estimate at the Fourier frequencies.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    /// Frequencies `j/n`, `j = 1, …, ⌊n/2⌋`, in cycles per observation.
    pub frequency: Vec<f64>,
    /// Estimated spectral density at each frequency.
    pub density: Vec<f64>,
    /// Lower confidence limits.
    pub lower: Vec<f64>,
    /// Upper confidence limits.
    pub upper: Vec<f64>,
    /// Equivalent degrees of freedom `ν`.
    pub df: f64,
    /// Bandwidth in cycles per observation.
    pub bandwidth: f64,
    /// Kernel weights `k₋M, …, k_M` (`[1.0]` for the raw periodogram).
    pub kernel: Vec<f64>,
    /// Coverage of the limits.
    pub level: f64,
}

/// Weights `k₋M, …, k_M` of the convolution of modified Daniell kernels with
/// the given spans (see the module documentation).
///
/// # Errors
/// [`StatError::InvalidArgument`] for an empty list or a span below 2.
pub fn modified_daniell_kernel(spans: &[usize]) -> Result<Vec<f64>> {
    if spans.is_empty() {
        return Err(StatError::InvalidArgument { message: "give at least one kernel span".into() });
    }
    let mut kernel = vec![1.0];
    for &s in spans {
        if s < 2 {
            return Err(StatError::InvalidArgument { message: "each kernel span must be at least 2".into() });
        }
        let m = s / 2;
        let mut k = vec![1.0 / (2 * m) as f64; 2 * m + 1];
        k[0] = 1.0 / (4 * m) as f64;
        k[2 * m] = k[0];
        let mut out = vec![0.0; kernel.len() + k.len() - 1];
        for (i, a) in kernel.iter().enumerate() {
            for (j, b) in k.iter().enumerate() {
                out[i + j] += a * b;
            }
        }
        kernel = out;
    }
    Ok(kernel)
}

/// Multiply `x` in place by the split cosine bell tapering a proportion `p`
/// at each end.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `0 ≤ p ≤ 0.5`.
pub fn split_cosine_taper(x: &mut [f64], p: f64) -> Result<()> {
    if !(0.0..=0.5).contains(&p) {
        return Err(StatError::InvalidArgument { message: "the taper proportion must be between 0 and 0.5".into() });
    }
    let n = x.len();
    let m = (n as f64 * p).floor() as usize;
    for i in 0..m {
        let w = 0.5 * (1.0 - (PI * (2 * i + 1) as f64 / (2 * m) as f64).cos());
        x[i] *= w;
        x[n - 1 - i] *= w;
    }
    Ok(())
}

/// Periodogram (empty `spans`) or kernel-smoothed spectral density of `y`
/// with confidence limits (see the module documentation).
///
/// # Errors
/// * [`StatError::InsufficientData`] with fewer than 4 observations.
/// * [`StatError::NonFiniteInput`] for NaN or infinite values.
/// * [`StatError::InvalidArgument`] for a taper outside `[0, 0.5]`, a level
///   outside (0, 1), a span below 2, or a kernel longer than the series.
pub fn spectral_density(y: &[f64], options: &SpectrumOptions) -> Result<Spectrum> {
    let n = y.len();
    if n < 4 {
        return Err(StatError::InsufficientData { required: 4, found: n });
    }
    check_finite(y, "series")?;
    if !(options.level > 0.0 && options.level < 1.0) {
        return Err(StatError::InvalidArgument { message: "the confidence level must be between 0 and 1".into() });
    }
    let mut x = y.to_vec();
    let nf = n as f64;
    match options.detrend {
        Detrend::None => {}
        Detrend::Mean | Detrend::Linear => {
            let mean = stat_core::summation::sum(&x) / nf;
            x.iter_mut().for_each(|v| *v -= mean);
            if options.detrend == Detrend::Linear {
                let centre = (nf + 1.0) / 2.0;
                let tc: Vec<f64> = (1..=n).map(|t| t as f64 - centre).collect();
                let slope = stat_core::summation::dot(&x, &tc) / stat_core::summation::dot(&tc, &tc);
                x.iter_mut().zip(&tc).for_each(|(v, t)| *v -= slope * t);
            }
        }
    }
    split_cosine_taper(&mut x, options.taper)?;
    let p = options.taper;
    let u2 = 1.0 - 1.25 * p;
    let u4 = 1.0 - 93.0 / 64.0 * p;
    let power = dft_power(&x);
    let mut pgram: Vec<f64> = power.iter().map(|v| v / (nf * u2)).collect();
    let kernel = if options.spans.is_empty() { vec![1.0] } else { modified_daniell_kernel(&options.spans)? };
    let half = kernel.len() / 2;
    if kernel.len() > n {
        return Err(StatError::InvalidArgument {
            message: format!("the smoothing kernel ({} weights) is longer than the series ({n})", kernel.len()),
        });
    }
    let nyq = n / 2;
    let density: Vec<f64> = if half == 0 {
        pgram[1..=nyq].to_vec()
    } else {
        pgram[0] = 0.5 * (pgram[1] + pgram[n - 1]);
        (1..=nyq)
            .map(|j| kernel.iter().enumerate().map(|(i, k)| k * pgram[(j + n + i - half) % n]).sum::<f64>())
            .collect()
    };
    let sum_sq: f64 = kernel.iter().map(|k| k * k).sum();
    let df = 2.0 / (sum_sq * u4 / (u2 * u2));
    let bandwidth = kernel
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let d = i as f64 - half as f64;
            (1.0 / 12.0 + d * d) * k
        })
        .sum::<f64>()
        .sqrt()
        / nf;
    let chi = ChiSquared::new(df)?;
    let alpha = 1.0 - options.level;
    let q_hi = chi.quantile(1.0 - alpha / 2.0)?;
    let q_lo = chi.quantile(alpha / 2.0)?;
    let lower = density.iter().map(|s| df * s / q_hi).collect();
    let upper = density.iter().map(|s| df * s / q_lo).collect();
    let frequency = (1..=nyq).map(|j| j as f64 / nf).collect();
    Ok(Spectrum { frequency, density, lower, upper, df, bandwidth, kernel, level: options.level })
}

/// `|Σₜ xₜ e^{−2πi j t/n}|²` for `j = 0, …, n − 1`.
fn dft_power(x: &[f64]) -> Vec<f64> {
    let n = x.len();
    if n <= 4096 {
        // Direct sums with exactly reduced angles (index j·t mod n).
        let table: Vec<(f64, f64)> = (0..n).map(|k| (2.0 * PI * k as f64 / n as f64).sin_cos()).collect();
        let mut out = vec![0.0; n];
        for j in 0..=n / 2 {
            let (mut re, mut im) = (0.0, 0.0);
            let mut idx = 0usize;
            for &v in x {
                let (s, c) = table[idx];
                re += v * c;
                im -= v * s;
                idx += j;
                if idx >= n {
                    idx -= n;
                }
            }
            out[j] = re * re + im * im;
            out[(n - j) % n] = out[j];
        }
        out
    } else {
        bluestein(x).into_iter().map(|(re, im)| re * re + im * im).collect()
    }
}

/// Discrete Fourier transform of a real sequence of any length by
/// Bluestein's chirp-z algorithm over power-of-two FFTs.
fn bluestein(x: &[f64]) -> Vec<(f64, f64)> {
    let n = x.len();
    let m = (2 * n - 1).next_power_of_two();
    // Chirp w_k = exp(−iπ k²/n), with k² reduced mod 2n for accuracy.
    let chirp: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let k2 = ((k as u128 * k as u128) % (2 * n as u128)) as f64;
            let a = PI * k2 / n as f64;
            (a.cos(), -a.sin())
        })
        .collect();
    let mut a = vec![(0.0, 0.0); m];
    for k in 0..n {
        a[k] = (x[k] * chirp[k].0, x[k] * chirp[k].1);
    }
    let mut b = vec![(0.0, 0.0); m];
    b[0] = (chirp[0].0, -chirp[0].1);
    for k in 1..n {
        let c = (chirp[k].0, -chirp[k].1);
        b[k] = c;
        b[m - k] = c;
    }
    fft(&mut a, false);
    fft(&mut b, false);
    for (u, v) in a.iter_mut().zip(&b) {
        *u = (u.0 * v.0 - u.1 * v.1, u.0 * v.1 + u.1 * v.0);
    }
    fft(&mut a, true);
    (0..n)
        .map(|k| {
            let (re, im) = (a[k].0 / m as f64, a[k].1 / m as f64);
            let c = chirp[k];
            (re * c.0 - im * c.1, re * c.1 + im * c.0)
        })
        .collect()
}

/// In-place iterative radix-2 FFT (length a power of two); `inverse` uses
/// `e^{+2πi…}` without the `1/n` factor.
fn fft(a: &mut [(f64, f64)], inverse: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        for k in 0..half {
            let ang = sign * 2.0 * PI * k as f64 / len as f64;
            let w = (ang.cos(), ang.sin());
            for start in (0..n).step_by(len) {
                let u = a[start + k];
                let v = a[start + k + half];
                let t = (v.0 * w.0 - v.1 * w.1, v.0 * w.1 + v.1 * w.0);
                a[start + k] = (u.0 + t.0, u.1 + t.1);
                a[start + k + half] = (u.0 - t.0, u.1 - t.1);
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_weights_sum_to_one() {
        let k = modified_daniell_kernel(&[3, 5]).unwrap();
        assert_eq!(k.len(), 7);
        assert!((k.iter().sum::<f64>() - 1.0).abs() < 1e-15);
        assert_eq!(modified_daniell_kernel(&[3]).unwrap(), vec![0.25, 0.5, 0.25]);
        assert!(modified_daniell_kernel(&[1]).is_err());
    }

    #[test]
    fn bluestein_matches_direct_transform() {
        let x: Vec<f64> = (0..5003).map(|t| ((t * 37 % 101) as f64).sin() + 0.01 * t as f64).collect();
        let fast = bluestein(&x);
        for j in [0usize, 1, 17, 2501, 4999] {
            let (mut re, mut im) = (0.0, 0.0);
            for (t, v) in x.iter().enumerate() {
                let a = 2.0 * PI * ((j * t) % x.len()) as f64 / x.len() as f64;
                re += v * a.cos();
                im -= v * a.sin();
            }
            let scale = (re * re + im * im).sqrt().max(1.0);
            assert!((fast[j].0 - re).abs() < 1e-9 * scale && (fast[j].1 - im).abs() < 1e-9 * scale, "{j}");
        }
    }

    #[test]
    fn periodogram_mean_equals_variance() {
        // Parseval: with the mean removed and no taper, Σ_{j=0}^{n−1} |X_j|²/n = Σ (x − x̄)² and
        // the j = 0 term vanishes, so Σ_{j=1}^{n−1} I(j/n) equals the centred sum of squares.
        let x: Vec<f64> = (0..64).map(|t| ((t * t) % 13) as f64).collect();
        let s = spectral_density(&x, &SpectrumOptions { detrend: Detrend::Mean, taper: 0.0, ..Default::default() })
            .unwrap();
        let mean = x.iter().sum::<f64>() / 64.0;
        let ss: f64 = x.iter().map(|v| (v - mean).powi(2)).sum();
        // Frequencies 1..31 counted twice plus the Nyquist frequency once.
        let total: f64 = 2.0 * s.density[..31].iter().sum::<f64>() + s.density[31];
        assert!((total - ss).abs() < 1e-9 * ss);
        assert!((s.df - 2.0).abs() < 1e-15);
    }

    #[test]
    fn bad_input() {
        assert!(spectral_density(&[1.0, 2.0, 3.0], &SpectrumOptions::default()).is_err());
        let x = [1.0, 2.0, 3.0, 2.0, 1.0];
        assert!(spectral_density(&x, &SpectrumOptions { taper: 0.6, ..Default::default() }).is_err());
        assert!(spectral_density(&x, &SpectrumOptions { spans: vec![9], ..Default::default() }).is_err());
        assert!(spectral_density(&[1.0, f64::NAN, 3.0, 4.0], &SpectrumOptions::default()).is_err());
    }
}
