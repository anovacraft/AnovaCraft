//! Convolution, correlation, deconvolution and the Hilbert transform.
//!
//! # Convolution and correlation
//! The linear convolution of `a` (length `n`) and `b` (length `m`) is
//! `(a ∗ b)_k = Σ_j a_j b_{k−j}`, `k = 0, …, n + m − 2` (*full*); *same*
//! keeps the `n` central terms starting at `⌊(m − 1)/2⌋`, and *valid* the
//! `|n − m| + 1` terms where the shorter sequence lies wholly inside the
//! longer (the conventions of SciPy's `scipy.signal.convolve`). It is
//! evaluated directly or through zero-padded FFTs (`O((n + m) log(n + m))`);
//! [`ConvolutionMethod::Auto`] picks the FFT when `n·m` is large. The
//! circular convolution of two sequences zero-padded to `N` is
//! `Σ_j a_j b_{(k−j) mod N}`.
//!
//! The cross-correlation at lag `ℓ` is `r_xy(ℓ) = Σ_t x_{t+ℓ} y_t`, lags
//! `−(m − 1), …, n − 1` (SciPy's `correlate` and `correlation_lags`). The
//! *coefficient* normalisation subtracts the means and divides by
//! `√(Σ(x − x̄)² Σ(y − ȳ)²)`, so `|r| ≤ 1`; *biased* divides the raw sums
//! by `n` and *unbiased* by the number of overlapping terms (Marple 1999,
//! *Digital Spectral Analysis*, ch. 5; Bendat & Piersol 2010, *Random Data*,
//! §8.1).
//!
//! # Deconvolution
//! Both methods assume the observed `y` is the *same*-mode convolution of an
//! unknown `x` with a known kernel `h`.
//!
//! * **Wiener (regularized) division** (Wiener 1949, *Extrapolation,
//!   Interpolation, and Smoothing of Stationary Time Series*): with the
//!   kernel centred circularly, `X = Y H̄ / (|H|² + λ)`, where `λ ≥ 0` is a
//!   noise-to-signal power ratio (`λ = 0` is plain inverse filtering).
//! * **Richardson–Lucy iteration** (Richardson 1972, *JOSA* 62(1); Lucy
//!   1974, *Astronomical Journal* 79(6)), for non-negative data and kernel:
//!   `x ← x · ((y / (x ∗ h + 10⁻¹²)) ∗ h̃)` with `h̃` the reversed kernel
//!   (normalised to sum 1), as scikit-image's `restoration.richardson_lucy`.
//!
//! # Hilbert transform
//! The analytic signal `z = x + i H{x}` is formed in the frequency domain:
//! the DFT is multiplied by 1 at zero (and Nyquist for even `n`), 2 at
//! positive and 0 at negative frequencies, and inverted (Marple 1999,
//! "Computing the discrete-time analytic signal via FFT", *IEEE Trans.
//! Signal Processing* 47(9); SciPy's `hilbert`). The envelope is `|z|`, the
//! instantaneous phase the unwrapped `arg z`, and the instantaneous
//! frequency `f_s Δphase / 2π`.

use crate::complex::Complex;
use crate::fft::{FftPlan, fft, ifft};
use crate::spectrum::{check_signal, unwrap_phase};
use crate::{Result, StatError};

/// Which part of the full convolution or correlation is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvolutionMode {
    /// All `n + m − 1` terms.
    Full,
    /// The central `n` terms (aligned with the first input).
    Same,
    /// Only terms computed without zero padding (`|n − m| + 1` terms).
    Valid,
}

/// How the convolution sums are evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvolutionMethod {
    /// Direct summation, `O(n m)`.
    Direct,
    /// Through zero-padded FFTs, `O((n + m) log(n + m))`.
    Fft,
    /// FFT when `n·m > 4096`, direct otherwise.
    Auto,
}

fn check_pair(a: &[f64], b: &[f64]) -> Result<()> {
    check_signal(a, 1)?;
    if b.is_empty() {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    if b.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "kernel" });
    }
    Ok(())
}

fn full_direct(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; a.len() + b.len() - 1];
    for (i, &x) in a.iter().enumerate() {
        for (j, &y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    out
}

fn full_fft(a: &[f64], b: &[f64]) -> Vec<f64> {
    let len = a.len() + b.len() - 1;
    // A fast size: the next power of two (the FFT handles any length, but
    // smooth lengths are quickest).
    let nfft = len.next_power_of_two();
    let plan = FftPlan::new(nfft);
    let pad = |v: &[f64]| {
        let mut buf = vec![Complex::ZERO; nfft];
        for (d, &s) in buf.iter_mut().zip(v) {
            *d = Complex::real(s);
        }
        plan.forward(&buf)
    };
    let fa = pad(a);
    let fb = pad(b);
    let prod: Vec<Complex> = fa.iter().zip(&fb).map(|(&p, &q)| p * q).collect();
    plan.inverse(&prod).into_iter().take(len).map(|z| z.re).collect()
}

fn trim(full: Vec<f64>, n: usize, m: usize, mode: ConvolutionMode) -> Vec<f64> {
    match mode {
        ConvolutionMode::Full => full,
        ConvolutionMode::Same => {
            let start = (m - 1) / 2;
            full[start..start + n].to_vec()
        }
        ConvolutionMode::Valid => {
            let (big, small) = if n >= m { (n, m) } else { (m, n) };
            full[small - 1..big].to_vec()
        }
    }
}

/// Linear convolution of `a` with `b`.
pub fn convolve(a: &[f64], b: &[f64], mode: ConvolutionMode, method: ConvolutionMethod) -> Result<Vec<f64>> {
    check_pair(a, b)?;
    let use_fft = match method {
        ConvolutionMethod::Direct => false,
        ConvolutionMethod::Fft => true,
        ConvolutionMethod::Auto => a.len().saturating_mul(b.len()) > 4096,
    };
    let full = if use_fft { full_fft(a, b) } else { full_direct(a, b) };
    Ok(trim(full, a.len(), b.len(), mode))
}

/// Circular convolution of `a` and `b`, both zero-padded to
/// `length` (default the longer of the two).
pub fn circular_convolve(a: &[f64], b: &[f64], length: Option<usize>) -> Result<Vec<f64>> {
    check_pair(a, b)?;
    let n = length.unwrap_or(a.len().max(b.len()));
    if n < a.len().max(b.len()) {
        return Err(StatError::InvalidArgument {
            message: "the circular length must be at least the longer sequence".into(),
        });
    }
    let pad = |v: &[f64]| {
        let mut buf = vec![Complex::ZERO; n];
        for (d, &s) in buf.iter_mut().zip(v) {
            *d = Complex::real(s);
        }
        fft(&buf)
    };
    let fa = pad(a);
    let fb = pad(b);
    let prod: Vec<Complex> = fa.iter().zip(&fb).map(|(&p, &q)| p * q).collect();
    Ok(ifft(&prod).into_iter().map(|z| z.re).collect())
}

/// Normalisation of a correlation sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrelationScaling {
    /// Raw sums `Σ x_{t+ℓ} y_t`.
    None,
    /// Raw sums divided by `n` (the longer length).
    Biased,
    /// Raw sums divided by the number of overlapping terms at each lag.
    Unbiased,
    /// Means removed, divided by `√(Σ(x − x̄)² Σ(y − ȳ)²)`.
    Coefficient,
}

/// A correlation sequence with its lags.
#[derive(Debug, Clone, PartialEq)]
pub struct Correlation {
    /// Lags `ℓ` (in samples).
    pub lag: Vec<i64>,
    /// `r(ℓ)` with the requested scaling.
    pub value: Vec<f64>,
    /// Lag at which `|r|` is largest (ties go to the smallest `|ℓ|`).
    pub lag_of_max: i64,
    /// `r` at that lag.
    pub max_value: f64,
}

/// Cross-correlation `r_xy(ℓ) = Σ_t x_{t+ℓ} y_t` over lags
/// `−(m − 1), …, n − 1`, optionally restricted to `|ℓ| ≤ max_lag`.
pub fn cross_correlation(
    x: &[f64],
    y: &[f64],
    scaling: CorrelationScaling,
    max_lag: Option<usize>,
) -> Result<Correlation> {
    check_pair(x, y)?;
    let (n, m) = (x.len(), y.len());
    let (xs, ys): (Vec<f64>, Vec<f64>) = if scaling == CorrelationScaling::Coefficient {
        let mx = x.iter().sum::<f64>() / n as f64;
        let my = y.iter().sum::<f64>() / m as f64;
        (x.iter().map(|v| v - mx).collect(), y.iter().map(|v| v - my).collect())
    } else {
        (x.to_vec(), y.to_vec())
    };
    let rev: Vec<f64> = ys.iter().rev().copied().collect();
    let full = convolve(&xs, &rev, ConvolutionMode::Full, ConvolutionMethod::Auto)?;
    let denom_coef = if scaling == CorrelationScaling::Coefficient {
        let sx: f64 = xs.iter().map(|v| v * v).sum();
        let sy: f64 = ys.iter().map(|v| v * v).sum();
        (sx * sy).sqrt()
    } else {
        1.0
    };
    let mut lag = Vec::new();
    let mut value = Vec::new();
    for (i, &r) in full.iter().enumerate() {
        let l = i as i64 - (m as i64 - 1);
        if let Some(ml) = max_lag
            && l.unsigned_abs() as usize > ml
        {
            continue;
        }
        let overlap = {
            // Terms t with 0 ≤ t < m and 0 ≤ t + l < n.
            let lo = 0i64.max(-l);
            let hi = (m as i64).min(n as i64 - l);
            (hi - lo).max(1) as f64
        };
        let v = match scaling {
            CorrelationScaling::None => r,
            CorrelationScaling::Biased => r / n.max(m) as f64,
            CorrelationScaling::Unbiased => r / overlap,
            CorrelationScaling::Coefficient => {
                if denom_coef > 0.0 {
                    r / denom_coef
                } else {
                    0.0
                }
            }
        };
        lag.push(l);
        value.push(v);
    }
    let mut best = 0;
    for i in 1..value.len() {
        let (a, b) = (value[i].abs(), value[best].abs());
        if a > b || (a == b && lag[i].abs() < lag[best].abs()) {
            best = i;
        }
    }
    Ok(Correlation { lag_of_max: lag[best], max_value: value[best], lag, value })
}

/// Autocorrelation `r_xx(ℓ)` for `ℓ = 0, …, max_lag` (default `n − 1`).
pub fn autocorrelation(x: &[f64], scaling: CorrelationScaling, max_lag: Option<usize>) -> Result<Correlation> {
    let full = cross_correlation(x, x, scaling, max_lag)?;
    let keep: Vec<usize> = (0..full.lag.len()).filter(|&i| full.lag[i] >= 0).collect();
    let lag: Vec<i64> = keep.iter().map(|&i| full.lag[i]).collect();
    let value: Vec<f64> = keep.iter().map(|&i| full.value[i]).collect();
    // The largest |r| away from lag 0 is the informative one.
    let start = usize::from(lag.len() > 1);
    let mut best = start;
    for i in start..value.len() {
        if value[i].abs() > value[best].abs() {
            best = i;
        }
    }
    Ok(Correlation { lag_of_max: lag[best], max_value: value[best], lag, value })
}

/// Place a kernel's centre (`⌊(m − 1)/2⌋`) at index 0 of a length-`n`
/// circular buffer, so circular convolution with it matches *same*-mode
/// alignment.
fn centred_kernel(h: &[f64], n: usize) -> Vec<Complex> {
    let c = (h.len() - 1) / 2;
    let mut buf = vec![Complex::ZERO; n];
    for (j, &v) in h.iter().enumerate() {
        let idx = (j as i64 - c as i64).rem_euclid(n as i64) as usize;
        buf[idx] += Complex::real(v);
    }
    buf
}

/// Wiener (regularized) deconvolution of `y` by the kernel `h` with
/// noise-to-signal ratio `lambda ≥ 0`.
pub fn wiener_deconvolve(y: &[f64], h: &[f64], lambda: f64) -> Result<Vec<f64>> {
    check_pair(y, h)?;
    if !(lambda.is_finite() && lambda >= 0.0) {
        return Err(StatError::InvalidArgument {
            message: "the noise-to-signal ratio must be zero or positive".into(),
        });
    }
    let n = y.len();
    if h.len() > n {
        return Err(StatError::InvalidArgument { message: "the kernel must not be longer than the signal".into() });
    }
    let fy = fft(&y.iter().map(|&v| Complex::real(v)).collect::<Vec<_>>());
    let fh = fft(&centred_kernel(h, n));
    let mut out = Vec::with_capacity(n);
    for (&a, &b) in fy.iter().zip(&fh) {
        let d = b.norm_sqr() + lambda;
        out.push(if d > 0.0 { a * b.conj() / d } else { Complex::ZERO });
    }
    Ok(ifft(&out).into_iter().map(|z| z.re).collect())
}

/// Richardson–Lucy deconvolution of non-negative `y` by a non-negative
/// kernel `h` (normalised to sum 1) for `iterations` steps.
pub fn richardson_lucy(y: &[f64], h: &[f64], iterations: usize) -> Result<Vec<f64>> {
    check_pair(y, h)?;
    if y.iter().chain(h).any(|&v| v < 0.0) {
        return Err(StatError::InvalidArgument {
            message: "Richardson–Lucy needs non-negative data and kernel".into(),
        });
    }
    let sum: f64 = h.iter().sum();
    if sum <= 0.0 {
        return Err(StatError::InvalidArgument { message: "the kernel must have a positive sum".into() });
    }
    if !(1..=10_000).contains(&iterations) {
        return Err(StatError::InvalidArgument { message: "use between 1 and 10 000 iterations".into() });
    }
    let psf: Vec<f64> = h.iter().map(|v| v / sum).collect();
    let mirror: Vec<f64> = psf.iter().rev().copied().collect();
    let mut est = vec![0.5; y.len()];
    for _ in 0..iterations {
        let conv = convolve(&est, &psf, ConvolutionMode::Same, ConvolutionMethod::Auto)?;
        let rel: Vec<f64> = y.iter().zip(&conv).map(|(&a, &c)| a / (c + 1e-12)).collect();
        let corr = convolve(&rel, &mirror, ConvolutionMode::Same, ConvolutionMethod::Auto)?;
        for (e, c) in est.iter_mut().zip(&corr) {
            *e *= c;
        }
    }
    Ok(est)
}

/// The analytic signal and its derived quantities.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyticSignal {
    /// `z = x + i H{x}`.
    pub analytic: Vec<Complex>,
    /// Envelope `|z|`.
    pub envelope: Vec<f64>,
    /// Unwrapped instantaneous phase in radians.
    pub phase: Vec<f64>,
    /// Instantaneous frequency `f_s Δphase / 2π` (length `n − 1`, between
    /// samples).
    pub frequency: Vec<f64>,
}

/// The analytic signal by the FFT method (as SciPy's `hilbert`).
pub fn hilbert(x: &[f64], sampling_rate: f64) -> Result<AnalyticSignal> {
    check_signal(x, 2)?;
    if !(sampling_rate.is_finite() && sampling_rate > 0.0) {
        return Err(StatError::InvalidArgument { message: "the sampling rate must be a positive number".into() });
    }
    let n = x.len();
    let mut s = fft(&x.iter().map(|&v| Complex::real(v)).collect::<Vec<_>>());
    for (k, v) in s.iter_mut().enumerate() {
        let h = if k == 0 || (n % 2 == 0 && k == n / 2) {
            1.0
        } else if k < n.div_ceil(2) {
            2.0
        } else {
            0.0
        };
        *v = *v * h;
    }
    let analytic = ifft(&s);
    let envelope = analytic.iter().map(|z| z.abs()).collect();
    let phase = unwrap_phase(&analytic.iter().map(|z| z.arg()).collect::<Vec<_>>());
    let frequency = phase.windows(2).map(|w| (w[1] - w[0]) * sampling_rate / (2.0 * std::f64::consts::PI)).collect();
    Ok(AnalyticSignal { analytic, envelope, phase, frequency })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_and_methods_agree() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [1.0, 0.5, 0.25];
        let full = convolve(&a, &b, ConvolutionMode::Full, ConvolutionMethod::Direct).unwrap();
        assert_eq!(full, vec![1.0, 2.5, 4.25, 6.0, 7.75, 3.5, 1.25]);
        let f2 = convolve(&a, &b, ConvolutionMode::Full, ConvolutionMethod::Fft).unwrap();
        for (p, q) in full.iter().zip(&f2) {
            assert!((p - q).abs() < 1e-12);
        }
        assert_eq!(
            convolve(&a, &b, ConvolutionMode::Same, ConvolutionMethod::Direct).unwrap(),
            vec![2.5, 4.25, 6.0, 7.75, 3.5]
        );
        assert_eq!(convolve(&a, &b, ConvolutionMode::Valid, ConvolutionMethod::Direct).unwrap(), vec![4.25, 6.0, 7.75]);
    }

    #[test]
    fn correlation_finds_a_known_delay() {
        let y: Vec<f64> = (0..50).map(|t| ((t * t) % 17) as f64).collect();
        let mut x = vec![0.0; 3];
        x.extend_from_slice(&y[..47]);
        let r = cross_correlation(&x, &y, CorrelationScaling::Coefficient, Some(10)).unwrap();
        assert_eq!(r.lag_of_max, 3);
    }

    #[test]
    fn wiener_with_no_regularization_inverts_circular_blur() {
        let x: Vec<f64> = (0..32).map(|t| ((t * 7) % 5) as f64).collect();
        let h = [0.2, 0.6, 0.2];
        let hc = centred_kernel(&h, 32);
        let fx = fft(&x.iter().map(|&v| Complex::real(v)).collect::<Vec<_>>());
        let fh = fft(&hc);
        let y: Vec<f64> =
            ifft(&fx.iter().zip(&fh).map(|(&a, &b)| a * b).collect::<Vec<_>>()).iter().map(|z| z.re).collect();
        let back = wiener_deconvolve(&y, &h, 0.0).unwrap();
        let err: f64 = back.iter().zip(&x).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(err < 1e-12, "{err}");
    }

    #[test]
    fn hilbert_of_cosine_is_sine() {
        let n = 64;
        let x: Vec<f64> = (0..n).map(|t| (2.0 * std::f64::consts::PI * 5.0 * t as f64 / n as f64).cos()).collect();
        let a = hilbert(&x, 64.0).unwrap();
        for t in 0..n {
            assert!((a.analytic[t].im - (2.0 * std::f64::consts::PI * 5.0 * t as f64 / n as f64).sin()).abs() < 1e-12);
            assert!((a.envelope[t] - 1.0).abs() < 1e-12);
        }
        assert!((a.frequency[10] - 5.0).abs() < 1e-9);
    }
}
