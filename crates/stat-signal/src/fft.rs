//! Fast Fourier transforms of any length.
//!
//! # Definition
//! The discrete Fourier transform of `x₀, …, x_{n−1}` is
//!
//! ```text
//! X_k = Σ_{t=0}^{n−1} x_t e^{−2πi k t / n},   k = 0, …, n − 1,
//! ```
//!
//! and the inverse is `x_t = (1/n) Σ_k X_k e^{2πi k t / n}` (the convention
//! of NumPy's `numpy.fft`, so results can be compared directly).
//!
//! # Method
//! Lengths whose prime factors are all at most [`MAX_DIRECT_RADIX`] use a
//! recursive mixed-radix decimation-in-time algorithm (Cooley & Tukey 1965,
//! "An algorithm for the machine calculation of complex Fourier series",
//! *Mathematics of Computation* 19; generalised to any factor as in
//! Singleton 1969, *IEEE Trans. Audio Electroacoustics* 17). Twiddle factors
//! come from one table of `e^{−2πi j/n}` evaluated directly (not by
//! recurrence), which keeps the error growth at `O(ε log n)`.
//!
//! Lengths with a larger prime factor use Bluestein's chirp-z algorithm
//! (Bluestein 1970, "A linear filtering approach to the computation of
//! discrete Fourier transform", *IEEE Trans. Audio Electroacoustics* 18):
//! with `w_k = e^{−πi k²/n}`, `X_k = w_k Σ_t (x_t w_t) w̄_{k−t}`, a circular
//! convolution of length `M ≥ 2n − 1` (a power of two) evaluated with the
//! mixed-radix transform. The chirp exponent `k² mod 2n` is reduced exactly
//! in integers so its argument stays accurate for large `k`.
//!
//! Both are `O(n log n)`. Accuracy is checked against NumPy (`pocketfft`)
//! for lengths 1–64, primes and large composite sizes in
//! `tests/fft_reference.rs`.

use crate::complex::Complex;
use std::f64::consts::PI;

/// Largest prime factor handled by the mixed-radix recursion; lengths with a
/// larger prime factor use Bluestein's algorithm.
pub const MAX_DIRECT_RADIX: usize = 13;

/// A precomputed transform of one length, for applying the same size many
/// times (as in Welch's method and spectrograms).
#[derive(Debug, Clone)]
pub struct FftPlan {
    n: usize,
    kind: PlanKind,
}

#[derive(Debug, Clone)]
enum PlanKind {
    /// Mixed radix: factors (smallest first) and twiddles `e^{−2πi j/n}`.
    Direct { factors: Vec<usize>, twiddles: Vec<Complex> },
    /// Bluestein: chirp `w_k`, the transformed conjugate chirp and the
    /// power-of-two plan.
    Chirp { chirp: Vec<Complex>, kernel: Vec<Complex>, inner: Box<FftPlan> },
}

fn prime_factors(mut n: usize) -> Vec<usize> {
    let mut out = Vec::new();
    // Radix 4 first: it halves the number of passes for powers of two.
    while n % 4 == 0 {
        out.push(4);
        n /= 4;
    }
    let mut p = 2;
    while p * p <= n {
        while n % p == 0 {
            out.push(p);
            n /= p;
        }
        p += 1;
    }
    if n > 1 {
        out.push(n);
    }
    out
}

fn twiddle_table(n: usize) -> Vec<Complex> {
    (0..n)
        .map(|j| {
            // Use the symmetric octant so sin/cos see arguments ≤ π/4·…; a
            // direct evaluation of each entry is accurate to an ulp or two.
            Complex::cis(-2.0 * PI * (j as f64) / (n as f64))
        })
        .collect()
}

impl FftPlan {
    /// Plan a transform of length `n` (`n ≥ 1`; `n = 0` gives an empty plan).
    pub fn new(n: usize) -> Self {
        let factors = prime_factors(n.max(1));
        let largest = factors.iter().copied().max().unwrap_or(1);
        if largest <= MAX_DIRECT_RADIX || n <= 1 {
            return FftPlan { n, kind: PlanKind::Direct { factors, twiddles: twiddle_table(n.max(1)) } };
        }
        let m = (2 * n - 1).next_power_of_two();
        let two_n = 2 * n as u128;
        let chirp: Vec<Complex> = (0..n)
            .map(|k| {
                let k = k as u128;
                let e = (k * k) % two_n;
                Complex::cis(-PI * (e as f64) / (n as f64))
            })
            .collect();
        let inner = FftPlan::new(m);
        let mut b = vec![Complex::ZERO; m];
        b[0] = chirp[0].conj();
        for k in 1..n {
            b[k] = chirp[k].conj();
            b[m - k] = chirp[k].conj();
        }
        let kernel = inner.forward(&b);
        FftPlan { n, kind: PlanKind::Chirp { chirp, kernel, inner: Box::new(inner) } }
    }

    /// The transform length.
    pub fn len(&self) -> usize {
        self.n
    }

    /// Whether the plan is for length zero.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Forward transform `X_k = Σ x_t e^{−2πi kt/n}`. Panics if `x.len()`
    /// differs from the planned length.
    pub fn forward(&self, x: &[Complex]) -> Vec<Complex> {
        assert_eq!(x.len(), self.n, "FFT input length does not match the plan");
        if self.n <= 1 {
            return x.to_vec();
        }
        match &self.kind {
            PlanKind::Direct { factors, twiddles } => {
                let mut out = vec![Complex::ZERO; self.n];
                let mut scratch = Vec::new();
                recurse(x, 0, 1, self.n, &mut out, factors, twiddles, self.n, &mut scratch);
                out
            }
            PlanKind::Chirp { chirp, kernel, inner } => {
                let m = inner.len();
                let mut a = vec![Complex::ZERO; m];
                for (k, (&xk, &wk)) in x.iter().zip(chirp).enumerate() {
                    a[k] = xk * wk;
                }
                let fa = inner.forward(&a);
                let prod: Vec<Complex> = fa.iter().zip(kernel).map(|(&p, &q)| p * q).collect();
                let conv = inner.inverse_unscaled(&prod);
                let scale = 1.0 / m as f64;
                (0..self.n).map(|k| conv[k] * chirp[k] * scale).collect()
            }
        }
    }

    /// Inverse transform `x_t = (1/n) Σ X_k e^{2πi kt/n}`.
    pub fn inverse(&self, x: &[Complex]) -> Vec<Complex> {
        let scale = 1.0 / self.n.max(1) as f64;
        self.inverse_unscaled(x).into_iter().map(|z| z * scale).collect()
    }

    fn inverse_unscaled(&self, x: &[Complex]) -> Vec<Complex> {
        let conj: Vec<Complex> = x.iter().map(|z| z.conj()).collect();
        self.forward(&conj).into_iter().map(Complex::conj).collect()
    }
}

/// Mixed-radix decimation in time: writes the DFT of
/// `input[offset], input[offset + stride], …` (`n` terms) into `out[..n]`.
/// `total` is the planned length, so `W_n^j = twiddles[j · total/n]`.
#[allow(clippy::too_many_arguments)]
fn recurse(
    input: &[Complex],
    offset: usize,
    stride: usize,
    n: usize,
    out: &mut [Complex],
    factors: &[usize],
    twiddles: &[Complex],
    total: usize,
    scratch: &mut Vec<Complex>,
) {
    if n == 1 {
        out[0] = input[offset];
        return;
    }
    let p = factors[0];
    let m = n / p;
    for r in 0..p {
        recurse(
            input,
            offset + r * stride,
            stride * p,
            m,
            &mut out[r * m..(r + 1) * m],
            &factors[1..],
            twiddles,
            total,
            scratch,
        );
    }
    let step = total / n; // W_n^j = twiddles[j·step]
    match p {
        2 => {
            for k in 0..m {
                let a = out[k];
                let b = out[k + m] * twiddles[k * step];
                out[k] = a + b;
                out[k + m] = a - b;
            }
        }
        4 => {
            for k in 0..m {
                let a0 = out[k];
                let a1 = out[k + m] * twiddles[k * step];
                let a2 = out[k + 2 * m] * twiddles[2 * k * step];
                let a3 = out[k + 3 * m] * twiddles[3 * k * step];
                let s02 = a0 + a2;
                let d02 = a0 - a2;
                let s13 = a1 + a3;
                let d13 = a1 - a3;
                // −i · d13
                let rot = Complex::new(d13.im, -d13.re);
                out[k] = s02 + s13;
                out[k + m] = d02 + rot;
                out[k + 2 * m] = s02 - s13;
                out[k + 3 * m] = d02 - rot;
            }
        }
        _ => {
            scratch.resize(p, Complex::ZERO);
            let wp = total / p; // W_p^j = twiddles[j·wp]
            for k in 0..m {
                for r in 0..p {
                    scratch[r] = out[r * m + k] * twiddles[(r * k * step) % total];
                }
                for q in 0..p {
                    let mut s = scratch[0];
                    for r in 1..p {
                        s += scratch[r] * twiddles[((r * q) % p) * wp];
                    }
                    out[q * m + k] = s;
                }
            }
        }
    }
}

/// Forward DFT of a complex sequence (any length).
pub fn fft(x: &[Complex]) -> Vec<Complex> {
    FftPlan::new(x.len()).forward(x)
}

/// Inverse DFT (scaled by `1/n`, so `ifft(fft(x)) = x`).
pub fn ifft(x: &[Complex]) -> Vec<Complex> {
    FftPlan::new(x.len()).inverse(x)
}

/// Forward DFT of a real sequence, zero-padded (or truncated) to `n` points,
/// returning the `⌊n/2⌋ + 1` non-negative-frequency terms (as `numpy.fft.rfft`).
pub fn rfft(x: &[f64], n: usize) -> Vec<Complex> {
    let mut buf = vec![Complex::ZERO; n];
    for (b, &v) in buf.iter_mut().zip(x) {
        *b = Complex::real(v);
    }
    let mut full = fft(&buf);
    full.truncate(n / 2 + 1);
    full
}

/// Inverse of [`rfft`]: the real sequence of length `n` whose transform has
/// the given non-negative-frequency terms (Hermitian symmetry supplies the
/// rest; imaginary parts of the zero and Nyquist terms are ignored, as in
/// `numpy.fft.irfft`).
pub fn irfft(spectrum: &[Complex], n: usize) -> Vec<f64> {
    let mut full = vec![Complex::ZERO; n];
    let half = n / 2;
    let take = (half + 1).min(spectrum.len()).min(n);
    full[..take].copy_from_slice(&spectrum[..take]);
    if n > 0 {
        full[0].im = 0.0;
        if n % 2 == 0 && half < spectrum.len() {
            full[half].im = 0.0;
        }
    }
    for k in 1..n.div_ceil(2) {
        full[n - k] = full[k].conj();
    }
    ifft(&full).into_iter().map(|z| z.re).collect()
}

/// Sample frequencies of an `n`-point DFT with sample spacing `d`, in the
/// order the transform returns them: `0, 1, …, ⌈n/2⌉ − 1, −⌊n/2⌋, …, −1`
/// divided by `n d` (as `numpy.fft.fftfreq`).
pub fn fft_frequencies(n: usize, d: f64) -> Vec<f64> {
    let nd = n as f64 * d;
    (0..n)
        .map(|k| {
            let j = if k < n.div_ceil(2) { k as f64 } else { k as f64 - n as f64 };
            j / nd
        })
        .collect()
}

/// Non-negative frequencies `0, 1, …, ⌊n/2⌋` divided by `n d` (as
/// `numpy.fft.rfftfreq`).
pub fn rfft_frequencies(n: usize, d: f64) -> Vec<f64> {
    let nd = n as f64 * d;
    (0..=n / 2).map(|k| k as f64 / nd).collect()
}

/// Reorder a two-sided spectrum (or its frequencies) so the zero frequency
/// is in the middle and frequencies ascend (as `numpy.fft.fftshift`).
pub fn fft_shift<T: Clone>(x: &[T]) -> Vec<T> {
    let n = x.len();
    let h = n.div_ceil(2);
    x[h..].iter().chain(&x[..h]).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(x: &[Complex]) -> Vec<Complex> {
        let n = x.len();
        (0..n)
            .map(|k| {
                let mut s = Complex::ZERO;
                for (t, &v) in x.iter().enumerate() {
                    s += v * Complex::cis(-2.0 * PI * ((k * t) % n) as f64 / n as f64);
                }
                s
            })
            .collect()
    }

    #[test]
    fn matches_the_definition_for_many_lengths() {
        for n in 1..=70 {
            let x: Vec<Complex> =
                (0..n).map(|t| Complex::new((t as f64 * 0.7).sin() + 0.1 * t as f64, (t as f64 * 1.3).cos())).collect();
            let a = fft(&x);
            let b = naive(&x);
            let scale: f64 = b.iter().map(|z| z.abs()).fold(1.0, f64::max);
            for (p, q) in a.iter().zip(&b) {
                assert!((*p - *q).abs() < 1e-12 * scale, "n = {n}");
            }
            let back = ifft(&a);
            for (p, q) in back.iter().zip(&x) {
                assert!((*p - *q).abs() < 1e-12 * scale, "inverse n = {n}");
            }
        }
    }

    #[test]
    fn real_transform_round_trip() {
        for n in [1, 2, 7, 16, 17, 101] {
            let x: Vec<f64> = (0..n).map(|t| (t as f64 * 0.37).sin() + 0.5).collect();
            let s = rfft(&x, n);
            assert_eq!(s.len(), n / 2 + 1);
            let y = irfft(&s, n);
            for (a, b) in x.iter().zip(&y) {
                assert!((a - b).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn frequencies_follow_numpy_order() {
        assert_eq!(fft_frequencies(5, 0.5), vec![0.0, 0.4, 0.8, -0.8, -0.4]);
        assert_eq!(fft_frequencies(4, 1.0), vec![0.0, 0.25, -0.5, -0.25]);
        assert_eq!(rfft_frequencies(4, 1.0), vec![0.0, 0.25, 0.5]);
        assert_eq!(fft_shift(&[0, 1, 2, -2, -1]), vec![-2, -1, 0, 1, 2]);
        assert_eq!(fft_shift(&[0, 1, -2, -1]), vec![-2, -1, 0, 1]);
    }
}
