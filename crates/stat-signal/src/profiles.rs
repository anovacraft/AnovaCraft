//! Peak shapes for spectra.
//!
//! Every shape is written with its **height** `h` (value at the top) and
//! **centre** `c` first, followed by width parameters (full widths at half
//! maximum, FWHM):
//!
//! | Shape | Parameters | Profile |
//! |---|---|---|
//! | Gaussian | h, c, w | `h exp(−4 ln 2 (x − c)²/w²)` |
//! | Lorentzian | h, c, w | `h / (1 + 4(x − c)²/w²)` |
//! | Pseudo-Voigt | h, c, w, η | `∝ η L(x) + (1 − η) G(x)`, `L`, `G` of unit area and FWHM `w`; η is the Lorentzian share of the area |
//! | Voigt | h, c, w_G, w_L | Gaussian (FWHM `w_G`) convolved with a Lorentzian (FWHM `w_L`), scaled to height `h` |
//! | Split Gaussian | h, c, w_left, w_right | Gaussian with FWHM `w_left` left of `c` and `w_right` right of it |
//! | Split Lorentzian | h, c, w_left, w_right | the same with Lorentzian halves |
//!
//! The pseudo-Voigt as an area-weighted sum of a Gaussian and a Lorentzian of
//! common width is the form of Thompson, Cox & Hastings ("Rietveld
//! refinement of Debye–Scherrer synchrotron X-ray data from Al₂O₃",
//! *J. Appl. Cryst.* 20, 1987).
//!
//! # Voigt profile
//! With `σ = w_G / (2√(2 ln 2))` and `γ = w_L / 2`, the unit-area Voigt
//! profile is `V(x) = Re w(z) / (σ√(2π))`, `z = (x + iγ)/(σ√2)`, where `w` is
//! the Faddeeva function `w(z) = e^{−z²} erfc(−iz)`. Humlíček ("Optimized
//! computation of the Voigt and complex probability functions", *J. Quant.
//! Spectrosc. Radiat. Transfer* 27, 1982) gives four-region rational
//! approximations to about 10⁻⁴; here `w` is computed for `Im z ≥ 0` by
//! Weideman's rational expansion ("Computation of the complex error
//! function", *SIAM J. Numer. Anal.* 31, 1994) with `N = 32` terms,
//!
//! ```text
//! w(z) ≈ 2 Σₙ₌₁ᴺ aₙ Zⁿ⁻¹ / (L − iz)² + 1 / (√π (L − iz)),   Z = (L + iz)/(L − iz),
//! L = 2^{−1/4} √N,
//! ```
//!
//! whose coefficients `aₙ` are the Fourier coefficients of
//! `e^{−t²}(L² + t²)` at `t = L tan(θ/2)` (absolute error below 10⁻¹³ on the
//! real line and above it). The Voigt FWHM is found by bisection.
//!
//! # Areas and widths
//! Areas are integrals over the whole line: Gaussian `h w √(π/(4 ln 2))`,
//! Lorentzian `h π w / 2`, Voigt `h / V(0)`, the pseudo-Voigt
//! `h / (η L(0) + (1 − η) G(0))`, split shapes the mean of the two halves'
//! full-profile areas. The FWHM of a split shape is `(w_left + w_right)/2`.

use std::sync::OnceLock;

/// A peak profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeakShape {
    /// Gaussian.
    Gaussian,
    /// Lorentzian (Cauchy).
    Lorentzian,
    /// Pseudo-Voigt: a Gaussian and a Lorentzian of common width.
    PseudoVoigt,
    /// Voigt: a Gaussian convolved with a Lorentzian.
    Voigt,
    /// Gaussian with different widths on the two sides.
    SplitGaussian,
    /// Lorentzian with different widths on the two sides.
    SplitLorentzian,
}

const LN2: f64 = std::f64::consts::LN_2;

impl PeakShape {
    /// Every shape.
    pub const ALL: [Self; 6] =
        [Self::Gaussian, Self::Lorentzian, Self::PseudoVoigt, Self::Voigt, Self::SplitGaussian, Self::SplitLorentzian];

    /// Name for menus and reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Gaussian => "Gaussian",
            Self::Lorentzian => "Lorentzian",
            Self::PseudoVoigt => "Pseudo-Voigt",
            Self::Voigt => "Voigt",
            Self::SplitGaussian => "Split Gaussian (asymmetric)",
            Self::SplitLorentzian => "Split Lorentzian (asymmetric)",
        }
    }

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Gaussian => "gaussian",
            Self::Lorentzian => "lorentzian",
            Self::PseudoVoigt => "pseudo-voigt",
            Self::Voigt => "voigt",
            Self::SplitGaussian => "split-gaussian",
            Self::SplitLorentzian => "split-lorentzian",
        }
    }

    /// The shape with identifier `key` (underscores accepted).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key.replace('_', "-"))
    }

    /// Number of parameters per peak.
    #[must_use]
    pub fn n_params(self) -> usize {
        match self {
            Self::Gaussian | Self::Lorentzian => 3,
            _ => 4,
        }
    }

    /// Parameter names, e.g. `["Height", "Centre", "FWHM"]`.
    #[must_use]
    pub fn param_names(self) -> &'static [&'static str] {
        match self {
            Self::Gaussian | Self::Lorentzian => &["Height", "Centre", "FWHM"],
            Self::PseudoVoigt => &["Height", "Centre", "FWHM", "Lorentzian share η"],
            Self::Voigt => &["Height", "Centre", "Gaussian FWHM", "Lorentzian FWHM"],
            Self::SplitGaussian | Self::SplitLorentzian => &["Height", "Centre", "Left FWHM", "Right FWHM"],
        }
    }

    /// Indices (within a peak's parameters) of the width parameters.
    #[must_use]
    pub fn width_params(self) -> &'static [usize] {
        match self {
            Self::Gaussian | Self::Lorentzian | Self::PseudoVoigt => &[2],
            _ => &[2, 3],
        }
    }

    /// Value at `x` of the peak with parameters `p`.
    #[must_use]
    pub fn eval(self, x: f64, p: &[f64]) -> f64 {
        let (h, c) = (p[0], p[1]);
        let d = x - c;
        match self {
            Self::Gaussian => h * gauss(d, p[2]),
            Self::Lorentzian => h * lorentz(d, p[2]),
            Self::PseudoVoigt => {
                let (w, eta) = (p[2], p[3]);
                let (g0, l0) = (gauss_area_density(w), 2.0 / (std::f64::consts::PI * w));
                let top = eta * l0 + (1.0 - eta) * g0;
                h * (eta * l0 * lorentz(d, w) + (1.0 - eta) * g0 * gauss(d, w)) / top
            }
            Self::Voigt => {
                let (sigma, gamma) = voigt_sigma_gamma(p[2], p[3]);
                h * voigt_profile(d, sigma, gamma) / voigt_profile(0.0, sigma, gamma)
            }
            Self::SplitGaussian => h * gauss(d, if d < 0.0 { p[2] } else { p[3] }),
            Self::SplitLorentzian => h * lorentz(d, if d < 0.0 { p[2] } else { p[3] }),
        }
    }

    /// Full width at half maximum.
    #[must_use]
    pub fn fwhm(self, p: &[f64]) -> f64 {
        match self {
            Self::Gaussian | Self::Lorentzian | Self::PseudoVoigt => p[2],
            Self::SplitGaussian | Self::SplitLorentzian => 0.5 * (p[2] + p[3]),
            Self::Voigt => {
                let (sigma, gamma) = voigt_sigma_gamma(p[2], p[3]);
                let half = 0.5 * voigt_profile(0.0, sigma, gamma);
                // The half width lies between 0 and (w_G + w_L)/2.
                let (mut lo, mut hi) = (0.0, 0.5 * (p[2].abs() + p[3].abs()) + f64::MIN_POSITIVE);
                for _ in 0..200 {
                    let mid = 0.5 * (lo + hi);
                    if voigt_profile(mid, sigma, gamma) > half {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                    if hi - lo <= 1e-15 * hi {
                        break;
                    }
                }
                lo + hi
            }
        }
    }

    /// Area under the peak.
    #[must_use]
    pub fn area(self, p: &[f64]) -> f64 {
        let h = p[0];
        let g = (std::f64::consts::PI / (4.0 * LN2)).sqrt();
        match self {
            Self::Gaussian => h * p[2] * g,
            Self::Lorentzian => h * std::f64::consts::PI * p[2] / 2.0,
            Self::PseudoVoigt => {
                let (w, eta) = (p[2], p[3]);
                h / (eta * 2.0 / (std::f64::consts::PI * w) + (1.0 - eta) * gauss_area_density(w))
            }
            Self::Voigt => {
                let (sigma, gamma) = voigt_sigma_gamma(p[2], p[3]);
                h / voigt_profile(0.0, sigma, gamma)
            }
            Self::SplitGaussian => h * g * 0.5 * (p[2] + p[3]),
            Self::SplitLorentzian => h * std::f64::consts::PI * (p[2] + p[3]) / 4.0,
        }
    }
}

/// Unit-height Gaussian of FWHM `w` at distance `d` from the centre.
fn gauss(d: f64, w: f64) -> f64 {
    (-4.0 * LN2 * d * d / (w * w)).exp()
}

/// Unit-height Lorentzian of FWHM `w`.
fn lorentz(d: f64, w: f64) -> f64 {
    1.0 / (1.0 + 4.0 * d * d / (w * w))
}

/// Peak density of a unit-area Gaussian of FWHM `w`.
fn gauss_area_density(w: f64) -> f64 {
    2.0 / w * (LN2 / std::f64::consts::PI).sqrt()
}

/// `(σ, γ)` of a Voigt profile from its Gaussian and Lorentzian FWHMs.
#[must_use]
pub fn voigt_sigma_gamma(w_gauss: f64, w_lorentz: f64) -> (f64, f64) {
    (w_gauss / (2.0 * (2.0 * LN2).sqrt()), 0.5 * w_lorentz)
}

#[derive(Debug, Clone, Copy)]
struct C(f64, f64);

impl C {
    fn mul(self, o: Self) -> Self {
        Self(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn div(self, o: Self) -> Self {
        let d = o.0 * o.0 + o.1 * o.1;
        Self((self.0 * o.0 + self.1 * o.1) / d, (self.1 * o.0 - self.0 * o.1) / d)
    }
    fn add(self, o: Self) -> Self {
        Self(self.0 + o.0, self.1 + o.1)
    }
}

const WEIDEMAN_N: usize = 32;

fn weideman() -> &'static (f64, [f64; WEIDEMAN_N]) {
    static COEF: OnceLock<(f64, [f64; WEIDEMAN_N])> = OnceLock::new();
    COEF.get_or_init(|| {
        let n = WEIDEMAN_N;
        let m = 2 * n;
        let l = (n as f64 / std::f64::consts::SQRT_2).sqrt();
        // F(k) = e^{−t²}(L² + t²), t = L tan(kπ/2M), for k = −M … M − 1 (0 at k = −M).
        let f = |k: i64| {
            if k == -(m as i64) {
                return 0.0;
            }
            let t = l * (k as f64 * std::f64::consts::PI / (2 * m) as f64).tan();
            (-t * t).exp() * (l * l + t * t)
        };
        let mut a = [0.0; WEIDEMAN_N];
        for (idx, coef) in a.iter_mut().enumerate() {
            let j = idx + 1;
            let mut s = 0.0;
            for k in -(m as i64)..(m as i64) {
                s += f(k) * (std::f64::consts::PI * (k * j as i64) as f64 / m as f64).cos();
            }
            *coef = s / (2 * m) as f64;
        }
        (l, a)
    })
}

/// Faddeeva function `w(z)` for `Im z ≥ 0` (Weideman's expansion).
fn faddeeva(z: C) -> C {
    let (l, a) = weideman();
    // iz = (−Im z, Re z).
    let iz = C(-z.1, z.0);
    let lm = C(l - iz.0, -iz.1);
    let lp = C(l + iz.0, iz.1);
    let zz = lp.div(lm);
    // Horner: Σₙ aₙ Zⁿ⁻¹.
    let mut p = C(0.0, 0.0);
    for &c in a.iter().rev() {
        p = p.mul(zz).add(C(c, 0.0));
    }
    let two = C(2.0, 0.0);
    two.mul(p).div(lm.mul(lm)).add(C(1.0 / std::f64::consts::PI.sqrt(), 0.0).div(lm))
}

/// Real part of the Faddeeva function, `Re w(x + iy)`, for `y ≥ 0` (the
/// Voigt function `K(x, y)`).
#[must_use]
pub fn faddeeva_real(x: f64, y: f64) -> f64 {
    faddeeva(C(x, y.max(0.0))).0
}

/// Unit-area Voigt profile at distance `x` from the centre, with Gaussian
/// standard deviation `sigma` and Lorentzian half width `gamma` (either may be
/// 0, not both).
#[must_use]
pub fn voigt_profile(x: f64, sigma: f64, gamma: f64) -> f64 {
    if sigma <= 0.0 {
        return gamma / (std::f64::consts::PI * (x * x + gamma * gamma));
    }
    let s2 = sigma * std::f64::consts::SQRT_2;
    faddeeva_real(x / s2, gamma / s2) / (sigma * (2.0 * std::f64::consts::PI).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faddeeva_known_values() {
        // w(0) = 1; Re w(iy) = erfcx(y): erfcx(1) = 0.427583576155807.
        assert!((faddeeva_real(0.0, 0.0) - 1.0).abs() < 1e-12);
        assert!((faddeeva_real(0.0, 1.0) - 0.427_583_576_155_807).abs() < 1e-12);
        // Re w(x) = exp(−x²) on the real line.
        for x in [0.3, 1.0, 2.5, 4.0] {
            assert!((faddeeva_real(x, 0.0) - (-x * x).exp()).abs() < 1e-12, "{x}");
        }
    }

    #[test]
    fn areas_match_numerical_integrals() {
        let params: [(PeakShape, &[f64]); 6] = [
            (PeakShape::Gaussian, &[2.0, 1.0, 0.7]),
            (PeakShape::Lorentzian, &[2.0, 1.0, 0.7]),
            (PeakShape::PseudoVoigt, &[2.0, 1.0, 0.7, 0.3]),
            (PeakShape::Voigt, &[2.0, 1.0, 0.5, 0.4]),
            (PeakShape::SplitGaussian, &[2.0, 1.0, 0.5, 1.1]),
            (PeakShape::SplitLorentzian, &[2.0, 1.0, 0.5, 1.1]),
        ];
        for (shape, p) in params {
            // Trapezoid over ±2000 widths with a fine step, plus the
            // Lorentzian tails beyond it analytically (≈ h w²/(4·L) each side).
            let (lo, hi, n) = (-2000.0, 2000.0, 4_000_000);
            let step = (hi - lo) / n as f64;
            let mut s = 0.0;
            for i in 0..=n {
                let x = lo + step * i as f64;
                let wgt = if i == 0 || i == n { 0.5 } else { 1.0 };
                s += wgt * shape.eval(x, p);
            }
            s *= step;
            let area = shape.area(p);
            assert!((s - area).abs() / area < 4e-4, "{shape:?}: {s} vs {area}");
            assert!((shape.eval(p[1], p) - p[0]).abs() < 1e-12);
            let half = shape.fwhm(p);
            if !matches!(shape, PeakShape::SplitGaussian | PeakShape::SplitLorentzian) {
                assert!((shape.eval(p[1] + half / 2.0, p) - p[0] / 2.0).abs() < 1e-9, "{shape:?}");
            }
        }
    }
}
