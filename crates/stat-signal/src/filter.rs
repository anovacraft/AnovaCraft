//! Digital filters: IIR design (Butterworth, Chebyshev I and II, elliptic)
//! in second-order sections, windowed-sinc FIR design, notch filters,
//! forward and forward–backward (zero-phase) filtering, frequency response,
//! and FFT (frequency-domain) filters.
//!
//! # IIR design
//! Each design starts from the normalised analog low-pass prototype's zeros,
//! poles and gain (cut-off 1 rad/s; Oppenheim & Schafer 2010,
//! *Discrete-Time Signal Processing*, 3rd ed., §7.1–7.2; Parks & Burrus
//! 1987, *Digital Filter Design*, ch. 7):
//!
//! * Butterworth: poles `−e^{iπ m/(2N)}`, `m = −N+1, −N+3, …, N−1`.
//! * Chebyshev I (pass-band ripple `r_p` dB): `ε = √(10^{r_p/10} − 1)`,
//!   `μ = asinh(1/ε)/N`, poles `−sinh(μ + iπ m/(2N))`; even orders have gain
//!   `1/√(1 + ε²)` at zero frequency (the ripple's lower edge).
//! * Chebyshev II (stop-band attenuation `r_s` dB): zeros `i/sin(π m/(2N))`
//!   (`m` odd, skipping 0) and poles the reciprocals of the scaled
//!   Butterworth poles; the cut-off is the stop-band edge.
//! * Elliptic (`r_p`, `r_s`): zeros and poles from Jacobi elliptic functions
//!   of modulus `k` solving the degree equation `N K'(k)/K(k) = K'(k₁)/K(k₁)`,
//!   `k₁ = ε/√(10^{r_s/10} − 1)` (Orfanidis 2006, *Lecture Notes on Elliptic
//!   Filter Design*, Rutgers; the inverse `sn` by Landen's transformation).
//!
//! The prototype is moved to the requested band (low-, high-, band-pass or
//! band-stop) by the standard analog frequency transformations at the
//! pre-warped edges `ω = 2 f_s tan(π f/f_s)` and then to the z-plane by the
//! bilinear transform `z = (2f_s + s)/(2f_s − s)`. Zeros and poles are
//! grouped into second-order sections (each complex-conjugate pole pair with
//! the nearest remaining zeros; poles nearest the unit circle last), which
//! keeps high-order filters numerically stable.
//!
//! # Applying filters
//! Forward filtering runs each section in direct form II transposed from a
//! zero state. Forward–backward filtering (zero phase, squared magnitude)
//! pads the signal at each end with `3·(2·sections + 1)` samples of its odd
//! reflection `2x₀ − x_{k}`, starts each pass from the steady state of a step
//! of the edge value, filters forwards, then backwards, and trims the
//! padding (Gustafsson 1996, *IEEE Trans. Signal Processing* 44(4); the
//! conventions of SciPy's `sosfiltfilt` and `filtfilt`, which
//! `tests/filter_reference.rs` checks).
//!
//! # FIR design
//! The windowed-sinc method: the ideal impulse response
//! `h_m = Σ_bands (f_hi sinc(f_hi m) − f_lo sinc(f_lo m))`, `m = t − (L−1)/2`,
//! frequencies relative to Nyquist, multiplied by a symmetric window and
//! scaled to unit gain at the centre of the first pass band (Oppenheim &
//! Schafer 2010, §7.5).
//!
//! # FFT filters
//! The signal's transform is multiplied by a real, zero-phase gain and
//! transformed back. *Ideal* gains are 1 in the pass band and 0 outside;
//! *smooth* gains use the Butterworth magnitude `1/√(1 + (f/f_c)^{2n})`
//! (low pass), its mirror `1/√(1 + (f_c/f)^{2n})` (high pass), their product
//! (band pass) or one minus the band-pass gain (band stop). The transform
//! treats the signal as periodic, so its ends should match (detrend first
//! when they do not).

use crate::complex::Complex;
use crate::fft::{irfft, rfft, rfft_frequencies};
use crate::spectrum::{check_signal, unwrap_phase};
use crate::window::Window;
use crate::{Result, StatError};
use std::f64::consts::PI;

/// The band a filter passes, with edge frequencies in the same units as the
/// sampling rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Band {
    /// Pass frequencies below the cut-off.
    Lowpass(f64),
    /// Pass frequencies above the cut-off.
    Highpass(f64),
    /// Pass frequencies between the two edges.
    Bandpass(f64, f64),
    /// Reject frequencies between the two edges.
    Bandstop(f64, f64),
}

impl Band {
    fn edges(&self) -> Vec<f64> {
        match *self {
            Band::Lowpass(f) | Band::Highpass(f) => vec![f],
            Band::Bandpass(a, b) | Band::Bandstop(a, b) => vec![a, b],
        }
    }

    fn check(&self, fs: f64) -> Result<()> {
        if !(fs.is_finite() && fs > 0.0) {
            return Err(StatError::InvalidArgument { message: "the sampling rate must be a positive number".into() });
        }
        let e = self.edges();
        let nyq = fs / 2.0;
        if e.iter().any(|&f| !(f.is_finite() && f > 0.0 && f < nyq)) {
            return Err(StatError::InvalidArgument {
                message: format!("cut-off frequencies must lie strictly between 0 and the Nyquist frequency {nyq}"),
            });
        }
        if e.len() == 2 && e[0] >= e[1] {
            return Err(StatError::InvalidArgument {
                message: "the lower band edge must be below the upper edge".into(),
            });
        }
        Ok(())
    }
}

/// IIR filter family.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IirFamily {
    /// Maximally flat pass band.
    Butterworth,
    /// Equiripple pass band with the given ripple (dB); the cut-off is the
    /// pass-band edge.
    ChebyshevI {
        /// Pass-band ripple in dB.
        ripple_db: f64,
    },
    /// Equiripple stop band with the given minimum attenuation (dB); the
    /// cut-off is the stop-band edge.
    ChebyshevII {
        /// Stop-band attenuation in dB.
        attenuation_db: f64,
    },
    /// Equiripple pass and stop bands; the cut-off is the pass-band edge.
    Elliptic {
        /// Pass-band ripple in dB.
        ripple_db: f64,
        /// Stop-band attenuation in dB.
        attenuation_db: f64,
    },
}

impl IirFamily {
    /// Display name.
    pub fn label(&self) -> &'static str {
        match self {
            IirFamily::Butterworth => "Butterworth",
            IirFamily::ChebyshevI { .. } => "Chebyshev type I",
            IirFamily::ChebyshevII { .. } => "Chebyshev type II",
            IirFamily::Elliptic { .. } => "Elliptic",
        }
    }
}

/// A filter as a cascade of second-order sections
/// `H(z) = Π (b₀ + b₁z⁻¹ + b₂z⁻²)/(1 + a₁z⁻¹ + a₂z⁻²)`, each row
/// `[b₀, b₁, b₂, 1, a₁, a₂]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sos {
    /// The sections.
    pub sections: Vec<[f64; 6]>,
}

/// Magnitude and phase of a filter at a grid of frequencies.
#[derive(Debug, Clone, PartialEq)]
pub struct FrequencyResponse {
    /// Frequencies `0 ≤ f < f_s/2`.
    pub frequency: Vec<f64>,
    /// Complex response `H(e^{2πi f/f_s})`.
    pub response: Vec<Complex>,
    /// `|H|`.
    pub magnitude: Vec<f64>,
    /// `20 log₁₀ |H|` (−∞ where `H = 0`).
    pub magnitude_db: Vec<f64>,
    /// Unwrapped phase in radians.
    pub phase: Vec<f64>,
}

impl FrequencyResponse {
    fn from_response(frequency: Vec<f64>, response: Vec<Complex>) -> Self {
        let magnitude: Vec<f64> = response.iter().map(|h| h.abs()).collect();
        let magnitude_db = magnitude.iter().map(|m| 20.0 * m.log10()).collect();
        let phase = unwrap_phase(&response.iter().map(|h| h.arg()).collect::<Vec<_>>());
        FrequencyResponse { frequency, response, magnitude, magnitude_db, phase }
    }
}

fn response_grid(points: usize, fs: f64) -> (Vec<f64>, Vec<Complex>) {
    let freq: Vec<f64> = (0..points).map(|k| fs / 2.0 * k as f64 / points as f64).collect();
    let zinv: Vec<Complex> = (0..points).map(|k| Complex::cis(-PI * k as f64 / points as f64)).collect();
    (freq, zinv)
}

impl Sos {
    /// Number of sections.
    pub fn len(&self) -> usize {
        self.sections.len()
    }

    /// Whether there are no sections.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Filter forwards from a zero initial state (direct form II
    /// transposed per section; as SciPy's `sosfilt`).
    pub fn filter(&self, x: &[f64]) -> Vec<f64> {
        let zeros = vec![[0.0; 2]; self.sections.len()];
        self.filter_from(x, &zeros)
    }

    fn filter_from(&self, x: &[f64], state: &[[f64; 2]]) -> Vec<f64> {
        let mut y = x.to_vec();
        for (s, z0) in self.sections.iter().zip(state) {
            let [b0, b1, b2, _, a1, a2] = *s;
            let (mut z1, mut z2) = (z0[0], z0[1]);
            for v in y.iter_mut() {
                let xi = *v;
                let yi = b0 * xi + z1;
                z1 = b1 * xi - a1 * yi + z2;
                z2 = b2 * xi - a2 * yi;
                *v = yi;
            }
        }
        y
    }

    /// Steady-state section states for a unit step input (SciPy's
    /// `sosfilt_zi`): each section's `lfilter_zi`, scaled by the DC gain of
    /// the sections before it.
    fn step_states(&self) -> Vec<[f64; 2]> {
        let mut scale = 1.0;
        let mut out = Vec::with_capacity(self.sections.len());
        for s in &self.sections {
            let [b0, b1, b2, _, a1, a2] = *s;
            // (I − Aᵀ) zi = b[1:] − a[1:] b₀ with companion A of (1, a₁, a₂):
            // [[1 + a₁, −1], [a₂, 1]] zi = [b₁ − a₁ b₀, b₂ − a₂ b₀].
            let r1 = b1 - a1 * b0;
            let r2 = b2 - a2 * b0;
            let det = (1.0 + a1) + a2;
            let z1 = (r1 + r2) / det;
            let z2 = r2 - a2 * z1;
            out.push([z1 * scale, z2 * scale]);
            scale *= (b0 + b1 + b2) / (1.0 + a1 + a2);
        }
        out
    }

    fn pad_length(&self) -> usize {
        let n = self.sections.len();
        let b2_zero = self.sections.iter().filter(|s| s[2] == 0.0).count();
        let a2_zero = self.sections.iter().filter(|s| s[5] == 0.0).count();
        3 * (2 * n + 1 - b2_zero.min(a2_zero))
    }

    /// Zero-phase forward–backward filtering (as SciPy's `sosfiltfilt`
    /// with odd padding). Needs more samples than the padding length
    /// `3·(2·sections + 1)`.
    pub fn filtfilt(&self, x: &[f64]) -> Result<Vec<f64>> {
        check_signal(x, 1)?;
        let edge = self.pad_length();
        let zi = self.step_states();
        forward_backward(x, edge, |sig, x0| {
            let st: Vec<[f64; 2]> = zi.iter().map(|z| [z[0] * x0, z[1] * x0]).collect();
            self.filter_from(sig, &st)
        })
    }

    /// Frequency response at `points` frequencies `k f_s/(2·points)`,
    /// `k = 0, …, points − 1` (as SciPy's `sosfreqz`).
    pub fn frequency_response(&self, points: usize, fs: f64) -> FrequencyResponse {
        let (freq, zinv) = response_grid(points, fs);
        let response = zinv
            .iter()
            .map(|&zi| {
                let z2 = zi * zi;
                self.sections.iter().fold(Complex::ONE, |acc, s| {
                    let num = Complex::real(s[0]) + zi * s[1] + z2 * s[2];
                    let den = Complex::real(s[3]) + zi * s[4] + z2 * s[5];
                    acc * (num / den)
                })
            })
            .collect();
        FrequencyResponse::from_response(freq, response)
    }

    /// Transfer-function numerator and denominator coefficients (powers of
    /// `z⁻¹`), the product of the sections.
    pub fn to_transfer_function(&self) -> (Vec<f64>, Vec<f64>) {
        let mut b = vec![1.0];
        let mut a = vec![1.0];
        for s in &self.sections {
            b = poly_mul(&b, &s[..3]);
            a = poly_mul(&a, &s[3..]);
        }
        (b, a)
    }
}

fn poly_mul(p: &[f64], q: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; p.len() + q.len() - 1];
    for (i, &a) in p.iter().enumerate() {
        for (j, &b) in q.iter().enumerate() {
            out[i + j] += a * b;
        }
    }
    out
}

/// Pad by odd reflection, run `pass` forwards and backwards (each from a
/// state scaled by its first input value), and trim.
fn forward_backward(x: &[f64], edge: usize, pass: impl Fn(&[f64], f64) -> Vec<f64>) -> Result<Vec<f64>> {
    let n = x.len();
    if n <= edge {
        return Err(StatError::InsufficientData { required: edge + 1, found: n });
    }
    let mut ext = Vec::with_capacity(n + 2 * edge);
    for k in (1..=edge).rev() {
        ext.push(2.0 * x[0] - x[k]);
    }
    ext.extend_from_slice(x);
    for k in 1..=edge {
        ext.push(2.0 * x[n - 1] - x[n - 1 - k]);
    }
    let y = pass(&ext, ext[0]);
    let mut rev: Vec<f64> = y.into_iter().rev().collect();
    let y0 = rev[0];
    rev = pass(&rev, y0);
    rev.reverse();
    Ok(rev[edge..edge + n].to_vec())
}

// ---------------------------------------------------------------------------
// Analog prototypes in zero–pole–gain form.

#[derive(Debug, Clone)]
struct Zpk {
    z: Vec<Complex>,
    p: Vec<Complex>,
    k: f64,
}

fn prod_neg(v: &[Complex]) -> Complex {
    v.iter().fold(Complex::ONE, |acc, &x| acc * (-x))
}

fn butterworth_prototype(n: usize) -> Zpk {
    let p = (0..n)
        .map(|i| {
            let m = -(n as f64) + 1.0 + 2.0 * i as f64;
            -Complex::cis(PI * m / (2.0 * n as f64))
        })
        .collect();
    Zpk { z: vec![], p, k: 1.0 }
}

fn chebyshev1_prototype(n: usize, rp: f64) -> Zpk {
    let eps = (10f64.powf(0.1 * rp) - 1.0).sqrt();
    let mu = (1.0 / eps).asinh() / n as f64;
    let p: Vec<Complex> = (0..n)
        .map(|i| {
            let m = -(n as f64) + 1.0 + 2.0 * i as f64;
            let theta = PI * m / (2.0 * n as f64);
            // −sinh(μ + iθ) = −(sinh μ cos θ + i cosh μ sin θ)
            Complex::new(-mu.sinh() * theta.cos(), -mu.cosh() * theta.sin())
        })
        .collect();
    let mut k = prod_neg(&p).re;
    if n % 2 == 0 {
        k /= (1.0 + eps * eps).sqrt();
    }
    Zpk { z: vec![], p, k }
}

fn chebyshev2_prototype(n: usize, rs: f64) -> Zpk {
    let de = 1.0 / (10f64.powf(0.1 * rs) - 1.0).sqrt();
    let mu = (1.0 / de).asinh() / n as f64;
    let ms: Vec<f64> = if n % 2 == 1 {
        (0..n).map(|i| -(n as f64) + 1.0 + 2.0 * i as f64).filter(|&m| m != 0.0).collect()
    } else {
        (0..n).map(|i| -(n as f64) + 1.0 + 2.0 * i as f64).collect()
    };
    // z = −conj(i / sin(mπ/(2N))) = i / sin(mπ/(2N))
    let z: Vec<Complex> = ms.iter().map(|&m| Complex::new(0.0, 1.0 / (m * PI / (2.0 * n as f64)).sin())).collect();
    let p: Vec<Complex> = (0..n)
        .map(|i| {
            let m = -(n as f64) + 1.0 + 2.0 * i as f64;
            let b = -Complex::cis(PI * m / (2.0 * n as f64));
            Complex::new(mu.sinh() * b.re, mu.cosh() * b.im).recip()
        })
        .collect();
    let k = (prod_neg(&p) / prod_neg(&z)).re;
    Zpk { z, p, k }
}

/// Arithmetic–geometric mean.
fn agm(mut a: f64, mut b: f64) -> f64 {
    for _ in 0..64 {
        let an = 0.5 * (a + b);
        let bn = (a * b).sqrt();
        if (an - bn).abs() <= 1e-16 * an {
            return an;
        }
        a = an;
        b = bn;
    }
    a
}

/// Complete elliptic integral of the first kind `K(m)` (parameter `m = k²`).
fn ellipk(m: f64) -> f64 {
    PI / (2.0 * agm(1.0, (1.0 - m).sqrt()))
}

/// `K(1 − p)`, accurate for small `p`.
fn ellipk_complement(p: f64) -> f64 {
    PI / (2.0 * agm(1.0, p.sqrt()))
}

/// Jacobi elliptic functions `(sn, cn, dn)` of `u` with parameter `m`, by the
/// descending AGM (Abramowitz & Stegun 1964, §16.4).
fn ellipj(u: f64, m: f64) -> (f64, f64, f64) {
    if m < 1e-12 {
        let (s, c) = u.sin_cos();
        return (s, c, 1.0);
    }
    if m > 1.0 - 1e-12 {
        let t = u.tanh();
        let sech = 1.0 / u.cosh();
        return (t, sech, sech);
    }
    let mut a = vec![1.0];
    let mut c = vec![m.sqrt()];
    let mut b = (1.0 - m).sqrt();
    let mut i = 0;
    while c[i].abs() > 1e-16 && i < 40 {
        let ai = a[i];
        a.push(0.5 * (ai + b));
        c.push(0.5 * (ai - b));
        b = (ai * b).sqrt();
        i += 1;
    }
    let mut phi = 2f64.powi(i as i32) * a[i] * u;
    while i > 0 {
        phi = 0.5 * (phi + (c[i] / a[i] * phi.sin()).asin());
        i -= 1;
    }
    let sn = phi.sin();
    let cn = phi.cos();
    let dn = (1.0 - m * sn * sn).sqrt();
    (sn, cn, dn)
}

/// Solve the degree equation for the elliptic modulus parameter `m` given
/// order `n` and `m₁` (Orfanidis 2006, eq. 49, by nome series).
fn ellip_degree(n: usize, m1: f64) -> f64 {
    let k1 = ellipk(m1);
    let k1p = ellipk_complement(m1);
    let q1 = (-PI * k1p / k1).exp();
    let q = q1.powf(1.0 / n as f64);
    let num: f64 = (0..=7).map(|m| q.powi(m * (m + 1))).sum();
    let den: f64 = 1.0 + 2.0 * (1..=8).map(|m| q.powi(m * m)).sum::<f64>();
    16.0 * q * (num / den).powi(4)
}

/// Imaginary part of the inverse `sn` at the imaginary point `i w`, i.e. the
/// real `v` with `sc(v, 1 − m) = w`, by Landen's descending transformation
/// (Orfanidis 2006, `asne`).
fn arc_jac_sc1(w: f64, m: f64) -> f64 {
    let complement = |kx: f64| ((1.0 - kx) * (1.0 + kx)).sqrt();
    let mut ks = vec![m.sqrt()];
    while *ks.last().expect("non-empty") != 0.0 && ks.len() < 12 {
        let kp = complement(*ks.last().expect("non-empty"));
        ks.push((1.0 - kp) / (1.0 + kp));
    }
    let kk: f64 = ks[1..].iter().map(|k| 1.0 + k).product::<f64>() * PI / 2.0;
    // All iterates stay on the imaginary axis: w_{j+1} = 2w/((1 + k_{j+1})(1 + √(1 + (k_j w)²))).
    let mut y = w;
    for j in 0..ks.len() - 1 {
        y = 2.0 * y / ((1.0 + ks[j + 1]) * (1.0 + (1.0 + (ks[j] * y).powi(2)).sqrt()));
    }
    kk * 2.0 / PI * y.asinh()
}

fn elliptic_prototype(n: usize, rp: f64, rs: f64) -> Zpk {
    let eps_sq = 10f64.powf(0.1 * rp) - 1.0;
    if n == 1 {
        let p = -(1.0 / eps_sq).sqrt();
        return Zpk { z: vec![], p: vec![Complex::real(p)], k: -p };
    }
    let eps = eps_sq.sqrt();
    let ck1_sq = eps_sq / (10f64.powf(0.1 * rs) - 1.0);
    let m = ellip_degree(n, ck1_sq);
    let capk = ellipk(m);
    let js: Vec<f64> = ((1 - n % 2)..n).step_by(2).map(|j| j as f64).collect();
    let sncndn: Vec<(f64, f64, f64)> = js.iter().map(|&j| ellipj(j * capk / n as f64, m)).collect();
    let mut z: Vec<Complex> = sncndn
        .iter()
        .filter(|(s, _, _)| s.abs() > f64::EPSILON)
        .map(|(s, _, _)| Complex::new(0.0, 1.0 / (m.sqrt() * s)))
        .collect();
    let zc: Vec<Complex> = z.iter().map(|v| v.conj()).collect();
    z.extend(zc);
    let r = arc_jac_sc1(1.0 / eps, ck1_sq);
    let v0 = capk * r / (n as f64 * ellipk(ck1_sq));
    let (sv, cv, dv) = ellipj(v0, 1.0 - m);
    let mut p: Vec<Complex> = sncndn
        .iter()
        .map(|&(s, c, d)| {
            let den = 1.0 - (d * sv).powi(2);
            Complex::new(-(c * d * sv * cv) / den, -(s * dv) / den)
        })
        .collect();
    if n % 2 == 1 {
        let norm: f64 = p.iter().map(|v| v.norm_sqr()).sum::<f64>().sqrt();
        let newp: Vec<Complex> = p.iter().filter(|v| v.im.abs() > f64::EPSILON * norm).map(|v| v.conj()).collect();
        p.extend(newp);
    } else {
        let pc: Vec<Complex> = p.iter().map(|v| v.conj()).collect();
        p.extend(pc);
    }
    let mut k = (prod_neg(&p) / prod_neg(&z)).re;
    if n % 2 == 0 {
        k /= (1.0 + eps_sq).sqrt();
    }
    Zpk { z, p, k }
}

// ---------------------------------------------------------------------------
// Frequency transformations and the bilinear transform.

fn lp2lp(f: Zpk, wo: f64) -> Zpk {
    let degree = f.p.len() as i32 - f.z.len() as i32;
    Zpk { z: f.z.iter().map(|&v| v * wo).collect(), p: f.p.iter().map(|&v| v * wo).collect(), k: f.k * wo.powi(degree) }
}

fn lp2hp(f: Zpk, wo: f64) -> Zpk {
    let degree = f.p.len() - f.z.len();
    let k = f.k * (prod_neg(&f.z) / prod_neg(&f.p)).re;
    let mut z: Vec<Complex> = f.z.iter().map(|&v| Complex::real(wo) / v).collect();
    z.extend(std::iter::repeat_n(Complex::ZERO, degree));
    Zpk { z, p: f.p.iter().map(|&v| Complex::real(wo) / v).collect(), k }
}

fn split_band(v: &[Complex], wo: f64) -> Vec<Complex> {
    let mut out: Vec<Complex> = v.iter().map(|&x| x + (x * x - Complex::real(wo * wo)).sqrt()).collect();
    out.extend(v.iter().map(|&x| x - (x * x - Complex::real(wo * wo)).sqrt()));
    out
}

fn lp2bp(f: Zpk, wo: f64, bw: f64) -> Zpk {
    let degree = f.p.len() - f.z.len();
    let zl: Vec<Complex> = f.z.iter().map(|&v| v * (bw / 2.0)).collect();
    let pl: Vec<Complex> = f.p.iter().map(|&v| v * (bw / 2.0)).collect();
    let mut z = split_band(&zl, wo);
    z.extend(std::iter::repeat_n(Complex::ZERO, degree));
    Zpk { z, p: split_band(&pl, wo), k: f.k * bw.powi(degree as i32) }
}

fn lp2bs(f: Zpk, wo: f64, bw: f64) -> Zpk {
    let degree = f.p.len() - f.z.len();
    let k = f.k * (prod_neg(&f.z) / prod_neg(&f.p)).re;
    let zh: Vec<Complex> = f.z.iter().map(|&v| Complex::real(bw / 2.0) / v).collect();
    let ph: Vec<Complex> = f.p.iter().map(|&v| Complex::real(bw / 2.0) / v).collect();
    let mut z = split_band(&zh, wo);
    z.extend(std::iter::repeat_n(Complex::new(0.0, wo), degree));
    z.extend(std::iter::repeat_n(Complex::new(0.0, -wo), degree));
    Zpk { z, p: split_band(&ph, wo), k }
}

fn bilinear(f: Zpk, fs: f64) -> Zpk {
    let fs2 = Complex::real(2.0 * fs);
    let degree = f.p.len() - f.z.len();
    let mut z: Vec<Complex> = f.z.iter().map(|&v| (fs2 + v) / (fs2 - v)).collect();
    z.extend(std::iter::repeat_n(Complex::real(-1.0), degree));
    let p = f.p.iter().map(|&v| (fs2 + v) / (fs2 - v)).collect();
    let num = f.z.iter().fold(Complex::ONE, |acc, &v| acc * (fs2 - v));
    let den = f.p.iter().fold(Complex::ONE, |acc, &v| acc * (fs2 - v));
    Zpk { z, p, k: f.k * (num / den).re }
}

// ---------------------------------------------------------------------------
// Second-order sections from zeros and poles.

/// Split roots into conjugate pairs (one representative with positive
/// imaginary part) and real roots.
fn split_roots(v: &[Complex]) -> (Vec<Complex>, Vec<f64>) {
    let scale = v.iter().map(|x| x.abs()).fold(1.0, f64::max);
    let tol = 1e-10 * scale;
    let mut complex = Vec::new();
    let mut real = Vec::new();
    for &x in v {
        if x.im.abs() <= tol {
            real.push(x.re);
        } else if x.im > 0.0 {
            complex.push(x);
        }
    }
    (complex, real)
}

/// A pair of roots as quadratic coefficients `[1, c₁, c₂]` in `z⁻¹`.
#[derive(Debug, Clone, Copy)]
enum RootGroup {
    Pair(Complex),
    Reals(f64, Option<f64>),
}

impl RootGroup {
    fn coefficients(&self) -> [f64; 3] {
        match *self {
            RootGroup::Pair(r) => [1.0, -2.0 * r.re, r.norm_sqr()],
            RootGroup::Reals(a, Some(b)) => [1.0, -(a + b), a * b],
            RootGroup::Reals(a, None) => [1.0, -a, 0.0],
        }
    }

    fn location(&self) -> Complex {
        match *self {
            RootGroup::Pair(r) => r,
            RootGroup::Reals(a, Some(b)) => Complex::real(if a.abs() > b.abs() { a } else { b }),
            RootGroup::Reals(a, None) => Complex::real(a),
        }
    }

    fn order(&self) -> usize {
        match self {
            RootGroup::Pair(_) | RootGroup::Reals(_, Some(_)) => 2,
            RootGroup::Reals(_, None) => 1,
        }
    }
}

fn group_roots(v: &[Complex]) -> Vec<RootGroup> {
    let (complex, mut real) = split_roots(v);
    let mut out: Vec<RootGroup> = complex.into_iter().map(RootGroup::Pair).collect();
    real.sort_by(|a, b| b.abs().total_cmp(&a.abs()));
    let mut it = real.chunks(2);
    for c in &mut it {
        out.push(RootGroup::Reals(c[0], c.get(1).copied()));
    }
    out
}

fn zpk_to_sos(f: &Zpk) -> Sos {
    let mut poles = group_roots(&f.p);
    let mut zeros = group_roots(&f.z);
    // Poles nearest the unit circle go last (their sections see the
    // smoothest input).
    poles.sort_by(|a, b| (1.0 - a.location().abs()).abs().total_cmp(&(1.0 - b.location().abs()).abs()).reverse());
    let mut sections = Vec::with_capacity(poles.len());
    for pg in &poles {
        let a = pg.coefficients();
        // The remaining zero group nearest this pole group (preferring the
        // same order); none left means no zeros in this section.
        let pick = zeros
            .iter()
            .enumerate()
            .min_by(|(_, x), (_, y)| {
                let dx = (x.location() - pg.location()).abs() + if x.order() == pg.order() { 0.0 } else { 1e6 };
                let dy = (y.location() - pg.location()).abs() + if y.order() == pg.order() { 0.0 } else { 1e6 };
                dx.total_cmp(&dy)
            })
            .map(|(i, _)| i);
        let b = match pick {
            Some(i) => zeros.remove(i).coefficients(),
            None => [1.0, 0.0, 0.0],
        };
        sections.push([b[0], b[1], b[2], a[0], a[1], a[2]]);
    }
    // Leftover zeros (more zeros than pole groups) go into extra sections.
    for zg in zeros {
        let b = zg.coefficients();
        sections.push([b[0], b[1], b[2], 1.0, 0.0, 0.0]);
    }
    if sections.is_empty() {
        sections.push([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }
    for v in &mut sections[0][..3] {
        *v *= f.k;
    }
    Sos { sections }
}

/// Design an IIR filter of the given family, order (1–30), band and
/// sampling rate, returned as second-order sections.
pub fn design_iir(family: IirFamily, order: usize, band: Band, sampling_rate: f64) -> Result<Sos> {
    if !(1..=30).contains(&order) {
        return Err(StatError::InvalidArgument { message: "the filter order must be between 1 and 30".into() });
    }
    band.check(sampling_rate)?;
    let positive = |v: f64, what: &str| -> Result<f64> {
        if v.is_finite() && v > 0.0 {
            Ok(v)
        } else {
            Err(StatError::InvalidArgument { message: format!("the {what} must be a positive number of dB") })
        }
    };
    let proto = match family {
        IirFamily::Butterworth => butterworth_prototype(order),
        IirFamily::ChebyshevI { ripple_db } => chebyshev1_prototype(order, positive(ripple_db, "pass-band ripple")?),
        IirFamily::ChebyshevII { attenuation_db } => {
            chebyshev2_prototype(order, positive(attenuation_db, "stop-band attenuation")?)
        }
        IirFamily::Elliptic { ripple_db, attenuation_db } => {
            let rp = positive(ripple_db, "pass-band ripple")?;
            let rs = positive(attenuation_db, "stop-band attenuation")?;
            if rs <= rp {
                return Err(StatError::InvalidArgument {
                    message: "the stop-band attenuation must exceed the pass-band ripple".into(),
                });
            }
            elliptic_prototype(order, rp, rs)
        }
    };
    let fs = sampling_rate;
    let warp = |f: f64| 2.0 * fs * (PI * f / fs).tan();
    let analog = match band {
        Band::Lowpass(f) => lp2lp(proto, warp(f)),
        Band::Highpass(f) => lp2hp(proto, warp(f)),
        Band::Bandpass(a, b) => {
            let (w1, w2) = (warp(a), warp(b));
            lp2bp(proto, (w1 * w2).sqrt(), w2 - w1)
        }
        Band::Bandstop(a, b) => {
            let (w1, w2) = (warp(a), warp(b));
            lp2bs(proto, (w1 * w2).sqrt(), w2 - w1)
        }
    };
    Ok(zpk_to_sos(&bilinear(analog, fs)))
}

/// Second-order IIR notch filter removing frequency `f0` with quality
/// factor `q = f0 / bandwidth(−3 dB)` (as SciPy's `iirnotch`; Orfanidis
/// 1996, *Introduction to Signal Processing*, §11.3).
pub fn notch(f0: f64, q: f64, sampling_rate: f64) -> Result<Sos> {
    Band::Lowpass(f0).check(sampling_rate)?;
    if !(q.is_finite() && q > 0.0) {
        return Err(StatError::InvalidArgument { message: "the quality factor must be positive".into() });
    }
    let w0 = 2.0 * f0 / sampling_rate * PI;
    let bw = w0 / q;
    let beta = (bw / 2.0).tan();
    let gain = 1.0 / (1.0 + beta);
    let c = w0.cos();
    Ok(Sos { sections: vec![[gain, -2.0 * gain * c, gain, 1.0, -2.0 * gain * c, 2.0 * gain - 1.0]] })
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) }
}

/// Windowed-sinc FIR design with `taps` coefficients (as SciPy's `firwin`
/// with `scale=True`). High-pass and band-stop designs need an odd number of
/// taps (a type I filter), since even-length symmetric filters have a zero
/// at the Nyquist frequency.
pub fn design_fir(taps: usize, band: Band, window: Window, sampling_rate: f64) -> Result<Vec<f64>> {
    band.check(sampling_rate)?;
    if taps < 3 {
        return Err(StatError::InvalidArgument { message: "an FIR filter needs at least 3 taps".into() });
    }
    let nyq = sampling_rate / 2.0;
    let pass_zero = matches!(band, Band::Lowpass(_) | Band::Bandstop(..));
    let cut: Vec<f64> = band.edges().iter().map(|f| f / nyq).collect();
    let pass_nyquist = (cut.len() % 2 == 1) ^ pass_zero;
    if pass_nyquist && taps % 2 == 0 {
        return Err(StatError::InvalidArgument {
            message: "high-pass and band-stop FIR filters need an odd number of taps".into(),
        });
    }
    let mut edges = Vec::new();
    if pass_zero {
        edges.push(0.0);
    }
    edges.extend(cut);
    if pass_nyquist {
        edges.push(1.0);
    }
    let alpha = 0.5 * (taps as f64 - 1.0);
    let m: Vec<f64> = (0..taps).map(|t| t as f64 - alpha).collect();
    let mut h = vec![0.0; taps];
    for pair in edges.chunks(2) {
        let (left, right) = (pair[0], pair[1]);
        for (hv, &mv) in h.iter_mut().zip(&m) {
            *hv += right * sinc(right * mv) - left * sinc(left * mv);
        }
    }
    let w = window.symmetric(taps)?;
    for (hv, wv) in h.iter_mut().zip(&w) {
        *hv *= wv;
    }
    let (left, right) = (edges[0], edges[1]);
    let scale_frequency = if left == 0.0 {
        0.0
    } else if right == 1.0 {
        1.0
    } else {
        0.5 * (left + right)
    };
    let s: f64 = h.iter().zip(&m).map(|(hv, mv)| hv * (PI * mv * scale_frequency).cos()).sum();
    h.iter_mut().for_each(|v| *v /= s);
    Ok(h)
}

/// Apply FIR coefficients forwards from a zero state (causal convolution
/// truncated to the signal length, as SciPy's `lfilter(b, 1, x)`).
pub fn fir_filter(b: &[f64], x: &[f64]) -> Vec<f64> {
    fir_filter_from(b, x, None)
}

fn fir_filter_from(b: &[f64], x: &[f64], zi: Option<(&[f64], f64)>) -> Vec<f64> {
    let n = x.len();
    let mut y = vec![0.0; n];
    for t in 0..n {
        let mut s = 0.0;
        for (j, &bj) in b.iter().enumerate().take(t + 1) {
            s += bj * x[t - j];
        }
        y[t] = s;
    }
    if let Some((z, scale)) = zi {
        // The state of a filter that has seen a constant `scale` forever adds
        // z[t]·scale to the first outputs.
        for (t, &zt) in z.iter().enumerate().take(n) {
            y[t] += zt * scale;
        }
    }
    y
}

/// Zero-phase forward–backward FIR filtering (as SciPy's
/// `filtfilt(b, [1], x)` with odd padding of `3·taps` samples).
pub fn fir_filtfilt(b: &[f64], x: &[f64]) -> Result<Vec<f64>> {
    check_signal(x, 1)?;
    if b.is_empty() {
        return Err(StatError::InvalidArgument { message: "give at least one FIR coefficient".into() });
    }
    // lfilter_zi for a = [1]: zi_k = Σ_{j>k} b_j.
    let zi: Vec<f64> = (0..b.len() - 1).map(|k| b[k + 1..].iter().sum()).collect();
    forward_backward(x, 3 * b.len(), |sig, x0| fir_filter_from(b, sig, Some((&zi, x0))))
}

/// Frequency response of FIR coefficients at `points` frequencies (as
/// SciPy's `freqz(b, 1, worN=points, fs=fs)`).
pub fn fir_frequency_response(b: &[f64], points: usize, fs: f64) -> FrequencyResponse {
    let (freq, zinv) = response_grid(points, fs);
    let response = zinv
        .iter()
        .map(|&zi| {
            let mut acc = Complex::ZERO;
            let mut zp = Complex::ONE;
            for &bj in b {
                acc += zp * bj;
                zp *= zi;
            }
            acc
        })
        .collect();
    FrequencyResponse::from_response(freq, response)
}

/// Shape of an FFT filter's gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FftFilterShape {
    /// Gain exactly 1 in the pass band and 0 outside.
    Ideal,
    /// Butterworth-shaped gain of the given order (1–20).
    Smooth {
        /// Steepness of the transition.
        order: u32,
    },
}

/// The zero-phase gain of an FFT filter at frequency `f` (≥ 0).
pub fn fft_filter_gain(band: Band, shape: FftFilterShape, f: f64) -> f64 {
    let low = |fc: f64| match shape {
        FftFilterShape::Ideal => {
            if f <= fc {
                1.0
            } else {
                0.0
            }
        }
        FftFilterShape::Smooth { order } => 1.0 / (1.0 + (f / fc).powi(2 * order as i32)).sqrt(),
    };
    let high = |fc: f64| match shape {
        FftFilterShape::Ideal => {
            if f >= fc {
                1.0
            } else {
                0.0
            }
        }
        FftFilterShape::Smooth { order } => {
            if f == 0.0 {
                0.0
            } else {
                1.0 / (1.0 + (fc / f).powi(2 * order as i32)).sqrt()
            }
        }
    };
    match band {
        Band::Lowpass(fc) => low(fc),
        Band::Highpass(fc) => high(fc),
        Band::Bandpass(a, b) => low(b) * high(a),
        Band::Bandstop(a, b) => 1.0 - low(b) * high(a),
    }
}

/// Filter by multiplying the signal's transform by a zero-phase gain.
pub fn fft_filter(x: &[f64], band: Band, shape: FftFilterShape, sampling_rate: f64) -> Result<Vec<f64>> {
    check_signal(x, 2)?;
    band.check(sampling_rate)?;
    if let FftFilterShape::Smooth { order } = shape
        && !(1..=20).contains(&order)
    {
        return Err(StatError::InvalidArgument { message: "the smooth cut-off order must be between 1 and 20".into() });
    }
    let n = x.len();
    let mut s = rfft(x, n);
    let freq = rfft_frequencies(n, 1.0 / sampling_rate);
    for (v, &f) in s.iter_mut().zip(&freq) {
        *v = *v * fft_filter_gain(band, shape, f);
    }
    Ok(irfft(&s, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn butterworth_lowpass_has_unit_dc_gain_and_minus_3db_at_cutoff() {
        for order in 1..=8 {
            let sos = design_iir(IirFamily::Butterworth, order, Band::Lowpass(10.0), 100.0).unwrap();
            let r = sos.frequency_response(1000, 100.0);
            assert!((r.magnitude[0] - 1.0).abs() < 1e-12);
            // 10 Hz is grid point 200 of 1000 over 0..50 Hz.
            assert!((r.magnitude_db[200] + 3.0103).abs() < 1e-3, "order {order}");
        }
    }

    #[test]
    fn elliptic_meets_its_ripple_and_attenuation() {
        let sos = design_iir(IirFamily::Elliptic { ripple_db: 1.0, attenuation_db: 40.0 }, 5, Band::Lowpass(0.2), 1.0)
            .unwrap();
        let r = sos.frequency_response(2000, 1.0);
        let pass_min = r.magnitude_db[..800].iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(pass_min > -1.0 - 1e-6, "{pass_min}");
        let stop_max = r.magnitude_db[1000..].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(stop_max < -40.0 + 1e-6, "{stop_max}");
    }

    #[test]
    fn notch_removes_its_frequency() {
        let sos = notch(50.0, 30.0, 1000.0).unwrap();
        let r = sos.frequency_response(100, 1000.0);
        assert!(r.magnitude[10] < 1e-12);
        assert!((r.magnitude[0] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn fft_filter_ideal_lowpass_keeps_low_sinusoid() {
        let n = 128;
        let x: Vec<f64> = (0..n)
            .map(|t| (2.0 * PI * 4.0 * t as f64 / n as f64).sin() + (2.0 * PI * 40.0 * t as f64 / n as f64).sin())
            .collect();
        let y = fft_filter(&x, Band::Lowpass(10.0), FftFilterShape::Ideal, n as f64).unwrap();
        for t in 0..n {
            assert!((y[t] - (2.0 * PI * 4.0 * t as f64 / n as f64).sin()).abs() < 1e-12);
        }
    }

    #[test]
    fn rejects_bad_designs() {
        assert!(design_iir(IirFamily::Butterworth, 0, Band::Lowpass(1.0), 10.0).is_err());
        assert!(design_iir(IirFamily::Butterworth, 2, Band::Lowpass(6.0), 10.0).is_err());
        assert!(design_iir(IirFamily::Butterworth, 2, Band::Bandpass(3.0, 2.0), 10.0).is_err());
        assert!(design_fir(10, Band::Highpass(1.0), Window::Hamming, 10.0).is_err());
    }
}
