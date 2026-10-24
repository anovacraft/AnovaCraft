//! Discrete and continuous wavelet transforms and wavelet denoising.
//!
//! # Discrete wavelet transform
//! One level of Mallat's pyramid algorithm (Mallat 1989, "A theory for
//! multiresolution signal decomposition: the wavelet representation", *IEEE
//! Trans. PAMI* 11(7)) filters the signal with the decomposition low-pass
//! `h` and high-pass `g` filters (length `F`) and keeps every second sample:
//!
//! ```text
//! a_o = Σ_j h_j x̃_{2o+1−j},   d_o = Σ_j g_j x̃_{2o+1−j},
//! ```
//!
//! where `x̃` extends the signal beyond its ends. With *symmetric* extension
//! (half-sample mirror, `x̃_{−1} = x₀`, `x̃_n = x_{n−1}`) or *zero* extension
//! each level gives `⌊(n + F − 1)/2⌋` coefficients; *periodic* extension
//! (periodization) gives `⌈n/2⌉` (an odd-length signal is first extended by
//! repeating its last value) and indexes `x̃` cyclically with an offset of
//! `F/2 − 1`, making the transform orthogonal. The inverse upsamples, filters
//! with the reconstruction filters and keeps the samples aligned with the
//! original (`2·len − F + 2` of them for symmetric and zero extension). These
//! are the conventions of PyWavelets (`dwt`, `idwt`, `wavedec`, `waverec`
//! with modes `symmetric`, `zero` and `periodization`), against which
//! `tests/wavelet_reference.rs` checks every wavelet.
//!
//! The orthogonal families are Haar, Daubechies' extremal-phase `dbN`
//! (N vanishing moments, `F = 2N`), the least-asymmetric symlets `symN` and
//! the coiflets `coifN` (`F = 6N`) (Daubechies 1992, *Ten Lectures on
//! Wavelets*, SIAM, ch. 6 and 8). Reconstruction high-pass coefficients are
//! `g̃_k = (−1)^k h̃_{F−1−k}`; decomposition filters are the reversed
//! reconstruction filters.
//!
//! # Denoising
//! Detail coefficients are shrunk towards zero with a *soft*
//! (`sign(d)·max(|d| − λ, 0)`) or *hard* (`d·[|d| ≥ λ]`) threshold, and the
//! signal reconstructed (Donoho & Johnstone 1994, "Ideal spatial adaptation
//! by wavelet shrinkage", *Biometrika* 81(3)). The noise level is
//! `σ̂ = median(|d₁|)/0.6745` from the finest level (or per level).
//! Thresholds:
//!
//! * *universal* `λ = σ̂ √(2 ln n)`;
//! * *SURE* (SureShrink; Donoho & Johnstone 1995, "Adapting to unknown
//!   smoothness via wavelet shrinkage", *JASA* 90(432)): per level, with
//!   `x = d/σ̂` (`m` coefficients), the `t ∈ {|xᵢ|}` minimising Stein's
//!   unbiased risk `m − 2#{|xᵢ| ≤ t} + Σ min(xᵢ², t²)`, capped at
//!   `√(2 ln m)`; when the level is sparse (`(Σxᵢ² − m)/m ≤ (log₂ m)^{3/2}/√m`)
//!   the universal `√(2 ln m)` is used instead (the hybrid rule);
//! * a fixed λ.
//!
//! # Continuous wavelet transform
//! For scales `s`, `W(s, t) = −√s · Δ(x ∗ Ψ_s)` where `Ψ` is the running
//! integral of the mother wavelet sampled on `[−8, 8]` (4096 points),
//! resampled at spacing `1/s` and reversed, and `Δ` a first difference, the
//! result trimmed to the signal length (the discretisation of PyWavelets'
//! `cwt` with `method='conv'`). Mother wavelets: Morlet
//! `ψ(t) = e^{−t²/2} cos 5t` and the Mexican hat
//! `ψ(t) = 2/(√3 π^{1/4}) (1 − t²) e^{−t²/2}` (Daubechies 1992, §2.2; Torrence
//! & Compo 1998, *Bull. Am. Meteor. Soc.* 79(1)). A scale corresponds to the
//! frequency `f_c / (s Δt)` with `f_c` the peak frequency of the wavelet's
//! Fourier transform (`≈ 5/2π` for Morlet, `√2/2π` for the Mexican hat).

mod filters;

use crate::spectrum::check_signal;
use crate::{Result, StatError};
use std::f64::consts::PI;

/// An orthogonal wavelet filter bank.
#[derive(Debug, Clone, PartialEq)]
pub struct Wavelet {
    name: String,
    rec_lo: Vec<f64>,
    rec_hi: Vec<f64>,
    dec_lo: Vec<f64>,
    dec_hi: Vec<f64>,
}

impl Wavelet {
    /// Look up a wavelet by name: `haar`, `db1`–`db10`, `sym2`–`sym10`,
    /// `coif1`–`coif5`.
    pub fn new(name: &str) -> Result<Self> {
        let key = name.trim().to_ascii_lowercase();
        let (_, rec) = filters::FILTERS.iter().find(|(n, _)| *n == key).ok_or_else(|| StatError::InvalidArgument {
            message: format!("unknown wavelet `{name}`; use haar, db1–db10, sym2–sym10 or coif1–coif5"),
        })?;
        let rec_lo = rec.to_vec();
        let f = rec_lo.len();
        let rec_hi: Vec<f64> =
            (0..f).map(|k| if k % 2 == 0 { rec_lo[f - 1 - k] } else { -rec_lo[f - 1 - k] }).collect();
        let dec_lo = rec_lo.iter().rev().copied().collect();
        let dec_hi = rec_hi.iter().rev().copied().collect();
        Ok(Wavelet { name: key, rec_lo, rec_hi, dec_lo, dec_hi })
    }

    /// Every supported wavelet name.
    pub fn names() -> Vec<&'static str> {
        filters::FILTERS.iter().map(|(n, _)| *n).collect()
    }

    /// The wavelet's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Filter length `F`.
    pub fn filter_length(&self) -> usize {
        self.rec_lo.len()
    }

    /// Decomposition low-pass filter `h`.
    pub fn dec_lo(&self) -> &[f64] {
        &self.dec_lo
    }

    /// Decomposition high-pass filter `g`.
    pub fn dec_hi(&self) -> &[f64] {
        &self.dec_hi
    }

    /// Reconstruction low-pass filter.
    pub fn rec_lo(&self) -> &[f64] {
        &self.rec_lo
    }

    /// Reconstruction high-pass filter.
    pub fn rec_hi(&self) -> &[f64] {
        &self.rec_hi
    }
}

/// How the signal is extended beyond its ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extension {
    /// Half-sample mirror image.
    Symmetric,
    /// Periodic (periodization; orthogonal, `⌈n/2⌉` coefficients per level).
    Periodic,
    /// Zeros.
    Zero,
}

/// Coefficients of one level.
#[derive(Debug, Clone, PartialEq)]
pub struct DwtLevel {
    /// Approximation coefficients.
    pub approximation: Vec<f64>,
    /// Detail coefficients.
    pub detail: Vec<f64>,
}

fn extended(x: &[f64], k: i64, mode: Extension) -> f64 {
    let n = x.len() as i64;
    if (0..n).contains(&k) {
        return x[k as usize];
    }
    match mode {
        Extension::Zero => 0.0,
        Extension::Symmetric => {
            let r = k.rem_euclid(2 * n);
            if r < n { x[r as usize] } else { x[(2 * n - 1 - r) as usize] }
        }
        Extension::Periodic => x[k.rem_euclid(n) as usize],
    }
}

/// One level of the discrete wavelet transform.
pub fn dwt(x: &[f64], wavelet: &Wavelet, mode: Extension) -> Result<DwtLevel> {
    check_signal(x, 1)?;
    let f = wavelet.filter_length() as i64;
    let (h, g) = (&wavelet.dec_lo, &wavelet.dec_hi);
    let mut approximation = Vec::new();
    let mut detail = Vec::new();
    match mode {
        Extension::Periodic => {
            let mut xp = x.to_vec();
            if xp.len() % 2 == 1 {
                xp.push(*x.last().expect("non-empty"));
            }
            let m = xp.len() as i64;
            let shift = f / 2 - 1;
            for o in 0..m / 2 {
                let (mut a, mut d) = (0.0, 0.0);
                for j in 0..f {
                    let v = xp[(2 * o + 1 - j + shift).rem_euclid(m) as usize];
                    a += h[j as usize] * v;
                    d += g[j as usize] * v;
                }
                approximation.push(a);
                detail.push(d);
            }
        }
        _ => {
            let n = x.len() as i64;
            let len = (n + f - 1) / 2;
            for o in 0..len {
                let (mut a, mut d) = (0.0, 0.0);
                for j in 0..f {
                    let v = extended(x, 2 * o + 1 - j, mode);
                    a += h[j as usize] * v;
                    d += g[j as usize] * v;
                }
                approximation.push(a);
                detail.push(d);
            }
        }
    }
    Ok(DwtLevel { approximation, detail })
}

/// Inverse of one level ([`dwt`]); `approximation` and `detail` must have
/// equal lengths.
pub fn idwt(approximation: &[f64], detail: &[f64], wavelet: &Wavelet, mode: Extension) -> Result<Vec<f64>> {
    if approximation.len() != detail.len() {
        return Err(StatError::DimensionMismatch {
            context: "approximation vs. detail length",
            expected: approximation.len(),
            found: detail.len(),
        });
    }
    let l = approximation.len();
    if l == 0 {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let f = wavelet.filter_length();
    let (rl, rh) = (&wavelet.rec_lo, &wavelet.rec_hi);
    match mode {
        Extension::Periodic => {
            let m = 2 * l;
            let shift = (f / 2) as i64 - 1;
            let mut out = vec![0.0; m];
            for k in 0..l {
                for j in 0..f {
                    let n = (2 * k as i64 + j as i64 - shift).rem_euclid(m as i64) as usize;
                    out[n] += approximation[k] * rl[j] + detail[k] * rh[j];
                }
            }
            Ok(out)
        }
        _ => {
            if 2 * l + 2 < f + 1 {
                return Err(StatError::InvalidArgument { message: "too few coefficients for this wavelet".into() });
            }
            let nr = 2 * l + 2 - f;
            let off = f - 2;
            let mut out = vec![0.0; nr];
            for k in 0..l {
                for j in 0..f {
                    let idx = 2 * k + j;
                    if idx >= off && idx < off + nr {
                        out[idx - off] += approximation[k] * rl[j] + detail[k] * rh[j];
                    }
                }
            }
            Ok(out)
        }
    }
}

/// The deepest useful decomposition level, `⌊log₂(n/(F − 1))⌋` (0 when the
/// signal is shorter than the filter).
pub fn max_level(n: usize, wavelet: &Wavelet) -> usize {
    let f = wavelet.filter_length();
    if f < 2 || n < f - 1 {
        return 0;
    }
    ((n as f64) / ((f - 1) as f64)).log2().floor() as usize
}

/// A multilevel decomposition `[a_J, d_J, d_{J−1}, …, d₁]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Decomposition {
    /// Coarsest approximation `a_J`.
    pub approximation: Vec<f64>,
    /// Details from the coarsest (`d_J`) to the finest (`d₁`) level.
    pub details: Vec<Vec<f64>>,
    /// Length of the original signal.
    pub signal_length: usize,
}

impl Decomposition {
    /// Number of levels `J`.
    pub fn levels(&self) -> usize {
        self.details.len()
    }
}

/// Multilevel decomposition to `level` (default [`max_level`]).
pub fn wavedec(x: &[f64], wavelet: &Wavelet, mode: Extension, level: Option<usize>) -> Result<Decomposition> {
    check_signal(x, 2)?;
    let maxl = max_level(x.len(), wavelet);
    let level = level.unwrap_or(maxl.max(1));
    if level == 0 {
        return Err(StatError::InvalidArgument { message: "use at least one decomposition level".into() });
    }
    if level > maxl.max(1) + 2 {
        return Err(StatError::InvalidArgument {
            message: format!(
                "{level} levels is too many for {} samples with {} (at most {})",
                x.len(),
                wavelet.name,
                maxl.max(1)
            ),
        });
    }
    let mut a = x.to_vec();
    let mut details = Vec::with_capacity(level);
    for _ in 0..level {
        let lv = dwt(&a, wavelet, mode)?;
        details.push(lv.detail);
        a = lv.approximation;
    }
    details.reverse();
    Ok(Decomposition { approximation: a, details, signal_length: x.len() })
}

/// Reconstruct from a multilevel decomposition (as `pywt.waverec`; the
/// result has the original length, trimming the extra sample an odd length
/// produces).
pub fn waverec(dec: &Decomposition, wavelet: &Wavelet, mode: Extension) -> Result<Vec<f64>> {
    let mut a = dec.approximation.clone();
    for d in &dec.details {
        if a.len() == d.len() + 1 {
            a.pop();
        }
        a = idwt(&a, d, wavelet, mode)?;
    }
    a.truncate(dec.signal_length);
    Ok(a)
}

/// Shrinkage applied to coefficients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdKind {
    /// `sign(d)·max(|d| − λ, 0)`.
    Soft,
    /// `d` if `|d| ≥ λ`, else 0.
    Hard,
}

/// Apply a threshold to one coefficient.
pub fn threshold(d: f64, lambda: f64, kind: ThresholdKind) -> f64 {
    match kind {
        ThresholdKind::Soft => d.signum() * (d.abs() - lambda).max(0.0),
        ThresholdKind::Hard => {
            if d.abs() >= lambda {
                d
            } else {
                0.0
            }
        }
    }
}

/// How the threshold is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThresholdRule {
    /// `σ̂ √(2 ln n)`.
    Universal,
    /// SureShrink hybrid, per level.
    Sure,
    /// The given λ at every level.
    Fixed(f64),
}

/// Where the noise level is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseEstimate {
    /// From the finest detail level, used at every level.
    FinestLevel,
    /// Separately at each level (for correlated noise).
    EachLevel,
}

/// Settings of [`denoise`].
#[derive(Debug, Clone, PartialEq)]
pub struct DenoiseOptions {
    /// Number of levels (default [`max_level`]).
    pub level: Option<usize>,
    /// Signal extension (default symmetric).
    pub extension: Extension,
    /// Threshold rule (default universal).
    pub rule: ThresholdRule,
    /// Soft or hard shrinkage (default soft).
    pub kind: ThresholdKind,
    /// Noise estimate (default finest level).
    pub noise: NoiseEstimate,
}

impl Default for DenoiseOptions {
    fn default() -> Self {
        DenoiseOptions {
            level: None,
            extension: Extension::Symmetric,
            rule: ThresholdRule::Universal,
            kind: ThresholdKind::Soft,
            noise: NoiseEstimate::FinestLevel,
        }
    }
}

/// Result of [`denoise`].
#[derive(Debug, Clone, PartialEq)]
pub struct Denoised {
    /// The reconstructed (denoised) signal.
    pub signal: Vec<f64>,
    /// The decomposition before thresholding.
    pub decomposition: Decomposition,
    /// Threshold λ per detail level, coarsest first (as `details`).
    pub thresholds: Vec<f64>,
    /// Noise level σ̂ per detail level, coarsest first.
    pub noise_sd: Vec<f64>,
    /// Share of detail coefficients set to zero.
    pub share_zeroed: f64,
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { 0.5 * (v[n / 2 - 1] + v[n / 2]) }
}

/// Noise level `median(|d|)/0.6745` (Donoho & Johnstone 1994).
pub fn mad_sigma(detail: &[f64]) -> f64 {
    let mut a: Vec<f64> = detail.iter().map(|v| v.abs()).collect();
    median(&mut a) / 0.6745
}

/// SureShrink threshold (in units of σ) for standardized coefficients `x`.
pub fn sure_threshold(x: &[f64]) -> f64 {
    let m = x.len();
    if m == 0 {
        return 0.0;
    }
    let mf = m as f64;
    let universal = (2.0 * mf.ln()).sqrt();
    let energy: f64 = x.iter().map(|v| v * v).sum();
    let sparsity = (energy - mf) / mf;
    let critical = mf.log2().powf(1.5) / mf.sqrt();
    if sparsity <= critical {
        return universal;
    }
    let mut sq: Vec<f64> = x.iter().map(|v| v * v).collect();
    sq.sort_by(f64::total_cmp);
    // SURE(t_k) with t_k² = sq[k]: m − 2(k + 1) + Σ_{i≤k} sq[i] + (m − k − 1) sq[k].
    let mut best = (f64::INFINITY, 0.0);
    let mut cum = 0.0;
    for (k, &s) in sq.iter().enumerate() {
        cum += s;
        let risk = mf - 2.0 * (k as f64 + 1.0) + cum + (mf - k as f64 - 1.0) * s;
        if risk < best.0 {
            best = (risk, s.sqrt());
        }
    }
    best.1.min(universal)
}

/// Wavelet shrinkage denoising.
pub fn denoise(x: &[f64], wavelet: &Wavelet, opts: &DenoiseOptions) -> Result<Denoised> {
    if let ThresholdRule::Fixed(l) = opts.rule
        && !(l.is_finite() && l >= 0.0)
    {
        return Err(StatError::InvalidArgument { message: "the threshold must be zero or positive".into() });
    }
    let dec = wavedec(x, wavelet, opts.extension, opts.level)?;
    let finest = mad_sigma(dec.details.last().expect("at least one level"));
    let n = x.len() as f64;
    let mut thresholds = Vec::new();
    let mut noise_sd = Vec::new();
    let mut zeroed = 0usize;
    let mut total = 0usize;
    let mut shrunk = dec.clone();
    for d in shrunk.details.iter_mut() {
        let sigma = match opts.noise {
            NoiseEstimate::FinestLevel => finest,
            NoiseEstimate::EachLevel => mad_sigma(d),
        };
        let lambda = match opts.rule {
            ThresholdRule::Universal => sigma * (2.0 * n.ln()).sqrt(),
            ThresholdRule::Sure => {
                if sigma > 0.0 {
                    let xs: Vec<f64> = d.iter().map(|v| v / sigma).collect();
                    sigma * sure_threshold(&xs)
                } else {
                    0.0
                }
            }
            ThresholdRule::Fixed(l) => l,
        };
        for v in d.iter_mut() {
            *v = threshold(*v, lambda, opts.kind);
            zeroed += usize::from(*v == 0.0);
            total += 1;
        }
        thresholds.push(lambda);
        noise_sd.push(sigma);
    }
    let signal = waverec(&shrunk, wavelet, opts.extension)?;
    Ok(Denoised { signal, decomposition: dec, thresholds, noise_sd, share_zeroed: zeroed as f64 / total.max(1) as f64 })
}

/// Mother wavelet of the continuous transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuousWavelet {
    /// Real Morlet `e^{−t²/2} cos 5t`.
    Morlet,
    /// Mexican hat (Ricker), the negative normalised second derivative of a Gaussian.
    MexicanHat,
}

impl ContinuousWavelet {
    fn psi(&self, t: f64) -> f64 {
        match self {
            ContinuousWavelet::Morlet => (-t * t / 2.0).exp() * (5.0 * t).cos(),
            ContinuousWavelet::MexicanHat => 2.0 / (3f64.sqrt() * PI.powf(0.25)) * (1.0 - t * t) * (-t * t / 2.0).exp(),
        }
    }

    /// Peak frequency of the wavelet's Fourier transform, in cycles per unit
    /// of `t`: `√2/2π` for the Mexican hat (spectrum `∝ ω² e^{−ω²/2}`) and
    /// `5/2π` for the Morlet (spectrum `∝ e^{−(ω−5)²/2} + e^{−(ω+5)²/2}`,
    /// whose peak differs from `ω = 5` by less than `e^{−50}`).
    pub fn peak_frequency(&self) -> f64 {
        match self {
            ContinuousWavelet::Morlet => 5.0 / (2.0 * PI),
            ContinuousWavelet::MexicanHat => 2f64.sqrt() / (2.0 * PI),
        }
    }

    /// Display name.
    pub fn label(&self) -> &'static str {
        match self {
            ContinuousWavelet::Morlet => "Morlet",
            ContinuousWavelet::MexicanHat => "Mexican hat",
        }
    }
}

/// Result of [`cwt`].
#[derive(Debug, Clone, PartialEq)]
pub struct Scalogram {
    /// The scales.
    pub scales: Vec<f64>,
    /// Frequency of each scale, `f_c/(s Δt)`.
    pub frequency: Vec<f64>,
    /// `coefficients[i][t]`: `W(sᵢ, t)`.
    pub coefficients: Vec<Vec<f64>>,
}

/// `count` scales spaced evenly on a log scale from `min` to `max`.
pub fn log_scales(min: f64, max: f64, count: usize) -> Vec<f64> {
    if count <= 1 {
        return vec![min];
    }
    let (a, b) = (min.ln(), max.ln());
    (0..count).map(|i| (a + (b - a) * i as f64 / (count - 1) as f64).exp()).collect()
}

/// Continuous wavelet transform at the given scales (`> 0`), with sample
/// spacing `dt`.
pub fn cwt(x: &[f64], scales: &[f64], wavelet: ContinuousWavelet, dt: f64) -> Result<Scalogram> {
    check_signal(x, 2)?;
    if scales.is_empty() || scales.iter().any(|s| !(s.is_finite() && *s > 0.0)) {
        return Err(StatError::InvalidArgument { message: "scales must be positive numbers".into() });
    }
    if !(dt.is_finite() && dt > 0.0) {
        return Err(StatError::InvalidArgument { message: "the sample spacing must be positive".into() });
    }
    const POINTS: usize = 4096;
    let (lo, hi) = (-8.0f64, 8.0f64);
    let grid: Vec<f64> = (0..POINTS).map(|i| lo + (hi - lo) * i as f64 / (POINTS - 1) as f64).collect();
    let step = grid[1] - grid[0];
    let mut int_psi = Vec::with_capacity(POINTS);
    let mut acc = 0.0;
    for &t in &grid {
        acc += wavelet.psi(t);
        int_psi.push(acc * step);
    }
    let n = x.len();
    let mut coefficients = Vec::with_capacity(scales.len());
    for &s in scales {
        let count = (s * (grid[POINTS - 1] - grid[0]) + 1.0).ceil() as usize;
        let mut kernel: Vec<f64> =
            (0..count).map(|k| (k as f64 / (s * step)) as usize).filter(|&j| j < POINTS).map(|j| int_psi[j]).collect();
        kernel.reverse();
        if kernel.is_empty() {
            return Err(StatError::InvalidArgument { message: "scale too small for the wavelet grid".into() });
        }
        let conv = crate::convolve::convolve(
            x,
            &kernel,
            crate::convolve::ConvolutionMode::Full,
            crate::convolve::ConvolutionMethod::Auto,
        )?;
        let coef: Vec<f64> = conv.windows(2).map(|w| -s.sqrt() * (w[1] - w[0])).collect();
        let extra = coef.len() as f64 - n as f64;
        let row = if extra > 0.0 {
            let start = (extra / 2.0).floor() as usize;
            coef[start..start + n].to_vec()
        } else if extra == 0.0 {
            coef
        } else {
            return Err(StatError::InvalidArgument { message: "scale too small for the signal".into() });
        };
        coefficients.push(row);
    }
    let fc = wavelet.peak_frequency();
    Ok(Scalogram { scales: scales.to_vec(), frequency: scales.iter().map(|s| fc / (s * dt)).collect(), coefficients })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_orthonormal_with_vanishing_moments() {
        for name in Wavelet::names() {
            let w = Wavelet::new(name).unwrap();
            let h = w.dec_lo();
            let f = h.len();
            let s: f64 = h.iter().sum();
            assert!((s - 2f64.sqrt()).abs() < 1e-10, "{name} sum");
            for shift in (0..f).step_by(2) {
                let dot: f64 = (0..f - shift).map(|k| h[k] * h[k + shift]).sum();
                let want = if shift == 0 { 1.0 } else { 0.0 };
                assert!((dot - want).abs() < 1e-10, "{name} shift {shift}: {dot}");
            }
            // Published tables carry about 12 digits for some symlets.
            // The high-pass filter kills constants (and, for dbN, polynomials
            // of degree < N).
            let g = w.dec_hi();
            assert!(g.iter().sum::<f64>().abs() < 1e-10, "{name} high-pass");
            if let Some(nm) = name.strip_prefix("db").and_then(|v| v.parse::<i32>().ok()) {
                for p in 0..nm {
                    let moment: f64 = g.iter().enumerate().map(|(k, v)| v * (k as f64).powi(p)).sum();
                    assert!(moment.abs() < 1e-6 * (f as f64).powi(p), "{name} moment {p}: {moment}");
                }
            }
        }
    }

    #[test]
    fn perfect_reconstruction() {
        let x: Vec<f64> = (0..37).map(|t| ((t * 13) % 7) as f64 + 0.1 * t as f64).collect();
        for name in ["haar", "db4", "sym5", "coif2"] {
            let w = Wavelet::new(name).unwrap();
            for mode in [Extension::Symmetric, Extension::Periodic, Extension::Zero] {
                let d = wavedec(&x, &w, mode, Some(3)).unwrap();
                let r = waverec(&d, &w, mode).unwrap();
                for (a, b) in r.iter().zip(&x) {
                    assert!((a - b).abs() < 1e-10, "{name} {mode:?}");
                }
            }
        }
    }

    #[test]
    fn denoising_reduces_error() {
        let n = 512;
        let clean: Vec<f64> = (0..n)
            .map(|t| {
                if t < 200 {
                    0.0
                } else if t < 350 {
                    3.0
                } else {
                    -1.0
                }
            })
            .collect();
        let mut seed = 12345u64;
        let noisy: Vec<f64> = clean
            .iter()
            .map(|c| {
                let mut s = 0.0;
                for _ in 0..12 {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    s += (seed >> 11) as f64 / (1u64 << 53) as f64;
                }
                c + 0.5 * (s - 6.0)
            })
            .collect();
        let w = Wavelet::new("haar").unwrap();
        for rule in [ThresholdRule::Universal, ThresholdRule::Sure] {
            let out = denoise(&noisy, &w, &DenoiseOptions { rule, ..Default::default() }).unwrap();
            let before: f64 = noisy.iter().zip(&clean).map(|(a, b)| (a - b).powi(2)).sum();
            let after: f64 = out.signal.iter().zip(&clean).map(|(a, b)| (a - b).powi(2)).sum();
            assert!(after < 0.5 * before, "{rule:?}: {after} vs {before}");
            assert!((out.noise_sd[0] - 0.5).abs() < 0.1);
        }
    }
}
