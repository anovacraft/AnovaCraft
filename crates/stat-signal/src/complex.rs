//! A minimal double-precision complex number for the transforms in this
//! crate (no external dependency; only the operations the transforms need).

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub};

/// A complex number `re + i·im` in double precision.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl Complex {
    /// Zero.
    pub const ZERO: Complex = Complex { re: 0.0, im: 0.0 };
    /// One.
    pub const ONE: Complex = Complex { re: 1.0, im: 0.0 };
    /// The imaginary unit.
    pub const I: Complex = Complex { re: 0.0, im: 1.0 };

    /// `re + i·im`.
    pub const fn new(re: f64, im: f64) -> Self {
        Complex { re, im }
    }

    /// A real number.
    pub const fn real(re: f64) -> Self {
        Complex { re, im: 0.0 }
    }

    /// `e^{iθ} = cos θ + i sin θ`.
    pub fn cis(theta: f64) -> Self {
        let (s, c) = theta.sin_cos();
        Complex { re: c, im: s }
    }

    /// `r e^{iθ}`.
    pub fn from_polar(r: f64, theta: f64) -> Self {
        Complex::cis(theta).scale(r)
    }

    /// Complex conjugate.
    pub fn conj(self) -> Self {
        Complex { re: self.re, im: -self.im }
    }

    /// Modulus `|z|` (computed with `hypot`, so it neither overflows nor
    /// underflows needlessly).
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// Squared modulus `|z|²`.
    pub fn norm_sqr(self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    /// Argument in `(−π, π]`.
    pub fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    /// Multiply by a real number.
    pub fn scale(self, s: f64) -> Self {
        Complex { re: self.re * s, im: self.im * s }
    }

    /// Reciprocal `1/z` (Smith's algorithm, which avoids overflow).
    pub fn recip(self) -> Self {
        Complex::ONE / self
    }

    /// Principal square root (branch cut along the negative real axis).
    pub fn sqrt(self) -> Self {
        if self.im == 0.0 {
            return if self.re >= 0.0 { Complex::real(self.re.sqrt()) } else { Complex::new(0.0, (-self.re).sqrt()) };
        }
        let r = self.abs();
        let re = ((r + self.re) / 2.0).sqrt();
        let im = ((r - self.re) / 2.0).sqrt().copysign(self.im);
        Complex { re, im }
    }

    /// Whether both parts are finite.
    pub fn is_finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}

impl Add for Complex {
    type Output = Complex;
    fn add(self, o: Complex) -> Complex {
        Complex::new(self.re + o.re, self.im + o.im)
    }
}

impl AddAssign for Complex {
    fn add_assign(&mut self, o: Complex) {
        self.re += o.re;
        self.im += o.im;
    }
}

impl Sub for Complex {
    type Output = Complex;
    fn sub(self, o: Complex) -> Complex {
        Complex::new(self.re - o.re, self.im - o.im)
    }
}

impl Neg for Complex {
    type Output = Complex;
    fn neg(self) -> Complex {
        Complex::new(-self.re, -self.im)
    }
}

impl Mul for Complex {
    type Output = Complex;
    fn mul(self, o: Complex) -> Complex {
        Complex::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
}

impl MulAssign for Complex {
    fn mul_assign(&mut self, o: Complex) {
        *self = *self * o;
    }
}

impl Mul<f64> for Complex {
    type Output = Complex;
    fn mul(self, s: f64) -> Complex {
        self.scale(s)
    }
}

impl Div for Complex {
    type Output = Complex;
    fn div(self, o: Complex) -> Complex {
        // Smith (1962), Algorithm 116.
        if o.re.abs() >= o.im.abs() {
            let r = o.im / o.re;
            let d = o.re + o.im * r;
            Complex::new((self.re + self.im * r) / d, (self.im - self.re * r) / d)
        } else {
            let r = o.re / o.im;
            let d = o.re * r + o.im;
            Complex::new((self.re * r + self.im) / d, (self.im * r - self.re) / d)
        }
    }
}

impl Div<f64> for Complex {
    type Output = Complex;
    fn div(self, s: f64) -> Complex {
        Complex::new(self.re / s, self.im / s)
    }
}

impl From<f64> for Complex {
    fn from(re: f64) -> Self {
        Complex::real(re)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_roots() {
        let a = Complex::new(3.0, 4.0);
        assert_eq!(a.abs(), 5.0);
        let q = a / Complex::new(1.0, -2.0);
        assert!((q.re + 1.0).abs() < 1e-15 && (q.im - 2.0).abs() < 1e-15);
        let s = Complex::new(-4.0, 0.0).sqrt();
        assert_eq!(s, Complex::new(0.0, 2.0));
        let r = Complex::new(-3.0, 4.0).sqrt();
        let back = r * r;
        assert!((back.re + 3.0).abs() < 1e-14 && (back.im - 4.0).abs() < 1e-14);
    }
}
