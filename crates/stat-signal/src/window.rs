//! Data windows (tapers) for spectral estimation and FIR design.
//!
//! # Definitions
//! For a window of `M` points with `n = 0, …, M − 1`, the symmetric forms
//! (Harris 1978, "On the use of windows for harmonic analysis with the
//! discrete Fourier transform", *Proc. IEEE* 66(1)) are
//!
//! * rectangular: `w = 1`;
//! * generalised cosine: `w = Σ_k (−1)^k a_k cos(2πkn/(M − 1))` with
//!   Hann `a = (0.5, 0.5)`, Hamming `(0.54, 0.46)`, Blackman
//!   `(0.42, 0.5, 0.08)` and flat-top
//!   `(0.21557895, 0.41663158, 0.277263158, 0.083578947, 0.006947368)`
//!   (D'Antona & Ferrero 2006, *Digital Signal Processing for Measurement
//!   Systems*, Springer, the coefficients SciPy uses);
//! * Kaiser: `w = I₀(β √(1 − (2n/(M − 1) − 1)²)) / I₀(β)` (Kaiser 1974);
//! * Tukey (tapered cosine) with fraction `α`: a cosine taper over
//!   `α(M − 1)/2` samples at each end and 1 in between; `α = 0` is
//!   rectangular and `α = 1` is Hann.
//!
//! The *periodic* form used for spectral analysis is the symmetric window of
//! `M + 1` points with its last point dropped (SciPy's `fftbins=True`), so
//! that the window is exactly periodic over a transform of length `M`. FIR
//! design uses the symmetric form.

use crate::{Result, StatError};
use std::f64::consts::PI;

/// A data window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Window {
    /// All weights 1 (no taper).
    Rectangular,
    /// Hann (raised cosine).
    Hann,
    /// Hamming.
    Hamming,
    /// Blackman.
    Blackman,
    /// Flat-top (accurate amplitudes of sinusoids between bins).
    FlatTop,
    /// Kaiser with shape parameter `β ≥ 0` (larger β: lower side lobes,
    /// wider main lobe).
    Kaiser {
        /// Shape parameter β.
        beta: f64,
    },
    /// Tukey (tapered cosine) with taper fraction `α ∈ [0, 1]`.
    Tukey {
        /// Fraction of the window inside the cosine tapers.
        alpha: f64,
    },
}

impl Window {
    /// Short display name, e.g. `"Hann"` or `"Kaiser (β = 8.6)"`.
    pub fn label(&self) -> String {
        match self {
            Window::Rectangular => "Rectangular".into(),
            Window::Hann => "Hann".into(),
            Window::Hamming => "Hamming".into(),
            Window::Blackman => "Blackman".into(),
            Window::FlatTop => "Flat-top".into(),
            Window::Kaiser { beta } => format!("Kaiser (β = {beta})"),
            Window::Tukey { alpha } => format!("Tukey (α = {alpha})"),
        }
    }

    fn check(&self) -> Result<()> {
        match *self {
            Window::Kaiser { beta } if !(beta.is_finite() && beta >= 0.0) => {
                Err(StatError::InvalidArgument { message: "the Kaiser β must be a non-negative number".into() })
            }
            Window::Tukey { alpha } if !(alpha.is_finite() && (0.0..=1.0).contains(&alpha)) => {
                Err(StatError::InvalidArgument { message: "the Tukey taper fraction must be between 0 and 1".into() })
            }
            _ => Ok(()),
        }
    }

    /// The symmetric window of `m` points.
    pub fn symmetric(&self, m: usize) -> Result<Vec<f64>> {
        self.check()?;
        Ok(symmetric(*self, m))
    }

    /// The periodic window of `m` points (symmetric of `m + 1`, last dropped).
    pub fn periodic(&self, m: usize) -> Result<Vec<f64>> {
        self.check()?;
        if m <= 1 {
            return Ok(vec![1.0; m]);
        }
        let mut w = symmetric(*self, m + 1);
        w.pop();
        Ok(w)
    }
}

fn general_cosine(m: usize, a: &[f64]) -> Vec<f64> {
    let denom = (m - 1) as f64;
    (0..m)
        .map(|n| {
            // fac = −π + 2πn/(M − 1); cos(k·fac) = (−1)^k cos(2πkn/(M − 1)).
            let fac = -PI + 2.0 * PI * n as f64 / denom;
            a.iter().enumerate().map(|(k, &ak)| ak * (k as f64 * fac).cos()).sum()
        })
        .collect()
}

fn symmetric(w: Window, m: usize) -> Vec<f64> {
    if m == 0 {
        return Vec::new();
    }
    if m == 1 {
        return vec![1.0];
    }
    match w {
        Window::Rectangular => vec![1.0; m],
        Window::Hann => general_cosine(m, &[0.5, 0.5]),
        Window::Hamming => general_cosine(m, &[0.54, 0.46]),
        Window::Blackman => general_cosine(m, &[0.42, 0.50, 0.08]),
        Window::FlatTop => general_cosine(m, &[0.21557895, 0.41663158, 0.277263158, 0.083578947, 0.006947368]),
        Window::Kaiser { beta } => {
            let denom = bessel_i0(beta);
            let half = (m - 1) as f64 / 2.0;
            (0..m)
                .map(|n| {
                    let r = (n as f64 - half) / half;
                    bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / denom
                })
                .collect()
        }
        Window::Tukey { alpha } => {
            if alpha <= 0.0 {
                return vec![1.0; m];
            }
            if alpha >= 1.0 {
                return symmetric(Window::Hann, m);
            }
            let mm = (m - 1) as f64;
            let width = (alpha * mm / 2.0).floor() as usize;
            (0..m)
                .map(|n| {
                    let nf = n as f64;
                    if n <= width {
                        0.5 * (1.0 + (PI * (-1.0 + 2.0 * nf / alpha / mm)).cos())
                    } else if n >= m - width - 1 {
                        0.5 * (1.0 + (PI * (-2.0 / alpha + 1.0 + 2.0 * nf / alpha / mm)).cos())
                    } else {
                        1.0
                    }
                })
                .collect()
        }
    }
}

/// Modified Bessel function of the first kind of order zero,
/// `I₀(x) = Σ_k ((x/2)^k / k!)²`, summed until the terms are negligible
/// (the series converges for every `x`; accurate to a few ulps for the
/// `|x| ≲ 700` that does not overflow).
pub fn bessel_i0(x: f64) -> f64 {
    let q = x * x / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut k = 1.0;
    loop {
        term *= q / (k * k);
        sum += term;
        if term < sum * 1e-17 || k > 1000.0 {
            break;
        }
        k += 1.0;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_and_hamming_end_points() {
        let w = Window::Hann.symmetric(5).unwrap();
        assert!(w[0].abs() < 1e-16 && (w[2] - 1.0).abs() < 1e-15);
        let p = Window::Hann.periodic(4).unwrap();
        assert_eq!(p.len(), 4);
        assert!((p[2] - 1.0).abs() < 1e-15 && (p[1] - 0.5).abs() < 1e-15);
        let h = Window::Hamming.symmetric(3).unwrap();
        assert!((h[0] - 0.08).abs() < 1e-15);
    }

    #[test]
    fn bessel_i0_known_values() {
        // Abramowitz & Stegun table 9.8.
        assert!((bessel_i0(0.0) - 1.0).abs() < 1e-16);
        assert!((bessel_i0(1.0) - 1.266_065_877_752_008_4).abs() < 1e-15);
        assert!((bessel_i0(5.0) / 27.239_871_823_604_45 - 1.0).abs() < 1e-14);
    }

    #[test]
    fn rejects_bad_parameters() {
        assert!(Window::Tukey { alpha: 1.5 }.periodic(8).is_err());
        assert!(Window::Kaiser { beta: -1.0 }.periodic(8).is_err());
    }
}
