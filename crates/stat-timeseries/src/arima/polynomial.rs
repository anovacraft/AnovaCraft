//! Lag polynomials: products of the regular and seasonal factors, the
//! differencing operator, complex roots, root reflection for invertibility,
//! and the partial-autocorrelation reparametrization of an autoregression
//! (Jones 1980; Barndorff-Nielsen & Schou 1973).

/// A complex number, just enough arithmetic for polynomial roots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
    fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
    fn mul(self, o: Self) -> Self {
        Self::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    fn div(self, o: Self) -> Self {
        let d = o.re * o.re + o.im * o.im;
        Self::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d)
    }
    fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }
}

/// Product of two polynomials given by their coefficients in increasing powers.
pub(crate) fn multiply(a: &[f64], b: &[f64]) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0.0; a.len() + b.len() - 1];
    for (i, &x) in a.iter().enumerate() {
        if x == 0.0 {
            continue;
        }
        for (j, &y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    out
}

/// The polynomial `1 + sign·Σ c_k z^{k·step}` in increasing powers.
fn lag_polynomial(coefs: &[f64], sign: f64, step: usize) -> Vec<f64> {
    let mut out = vec![0.0; coefs.len() * step + 1];
    out[0] = 1.0;
    for (k, &c) in coefs.iter().enumerate() {
        out[(k + 1) * step] = sign * c;
    }
    out
}

/// Coefficients `φ*₁…φ*ₚ*` of the expanded autoregressive operator
/// `φ(B) Φ(Bᵐ) = 1 − Σ φ*ⱼ Bʲ`, with `φ(B) = 1 − Σ φᵢ Bⁱ` and
/// `Φ(Bᵐ) = 1 − Σ Φᵢ B^{im}`.
pub(crate) fn expand_ar(ar: &[f64], seasonal_ar: &[f64], period: usize) -> Vec<f64> {
    let poly = multiply(&lag_polynomial(ar, -1.0, 1), &lag_polynomial(seasonal_ar, -1.0, period.max(1)));
    poly[1..].iter().map(|c| -c).collect()
}

/// Coefficients `θ*₁…θ*_q*` of the expanded moving-average operator
/// `θ(B) Θ(Bᵐ) = 1 + Σ θ*ⱼ Bʲ`.
pub(crate) fn expand_ma(ma: &[f64], seasonal_ma: &[f64], period: usize) -> Vec<f64> {
    let poly = multiply(&lag_polynomial(ma, 1.0, 1), &lag_polynomial(seasonal_ma, 1.0, period.max(1)));
    poly[1..].to_vec()
}

/// Coefficients `δ₁…δ_{d+mD}` of `(1 − B)^d (1 − Bᵐ)^D = 1 − Σ δⱼ Bʲ`, so that
/// an integrated series satisfies `uₜ = wₜ + Σ δⱼ u_{t−j}` with `w` the
/// differenced series.
pub(crate) fn differencing_operator(d: usize, seasonal_d: usize, period: usize) -> Vec<f64> {
    let mut poly = vec![1.0];
    for _ in 0..d {
        poly = multiply(&poly, &[1.0, -1.0]);
    }
    let m = period.max(1);
    for _ in 0..seasonal_d {
        let mut s = vec![0.0; m + 1];
        s[0] = 1.0;
        s[m] = -1.0;
        poly = multiply(&poly, &s);
    }
    poly[1..].iter().map(|c| -c).collect()
}

/// Apply the differencing operator: `wₜ = uₜ − Σ δⱼ u_{t−j}` for
/// `t = nd, …, n−1` (`nd = δ.len()`).
pub(crate) fn difference(u: &[f64], delta: &[f64]) -> Vec<f64> {
    let nd = delta.len();
    (nd..u.len()).map(|t| u[t] - delta.iter().enumerate().map(|(j, &c)| c * u[t - 1 - j]).sum::<f64>()).collect()
}

/// All complex roots of `Σ cₖ zᵏ` (`c` in increasing powers, trailing zeros
/// dropped) by the Aberth–Ehrlich simultaneous iteration (Aberth 1973,
/// *Math. Comp.* 27), followed by Newton polishing on the original polynomial.
/// Returns an empty vector for a constant polynomial.
pub(crate) fn roots(c: &[f64]) -> Vec<Complex> {
    let mut deg = c.len();
    while deg > 0 && c[deg - 1] == 0.0 {
        deg -= 1;
    }
    if deg <= 1 {
        return Vec::new();
    }
    let n = deg - 1;
    let lead = c[n];
    let monic: Vec<Complex> = c[..=n].iter().map(|&x| Complex::new(x / lead, 0.0)).collect();
    let eval = |z: Complex| -> (Complex, Complex) {
        // Horner for p and p'.
        let mut p = monic[n];
        let mut dp = Complex::new(0.0, 0.0);
        for k in (0..n).rev() {
            dp = dp.mul(z).add(p);
            p = p.mul(z).add(monic[k]);
        }
        (p, dp)
    };
    // Starting points on a circle of the Cauchy-bound radius, rotated off the axes.
    let radius = 1.0 + monic[..n].iter().map(|z| z.abs()).fold(0.0_f64, f64::max);
    let start_r = radius.min(radius.powf(1.0 / n as f64).max(0.5));
    let mut z: Vec<Complex> = (0..n)
        .map(|k| {
            let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64 + 0.4;
            Complex::new(start_r * a.cos(), start_r * a.sin())
        })
        .collect();
    for _ in 0..500 {
        let mut moved = 0.0_f64;
        for i in 0..n {
            let (p, dp) = eval(z[i]);
            if p.abs() == 0.0 {
                continue;
            }
            let ratio = p.div(dp);
            let mut s = Complex::new(0.0, 0.0);
            for (j, &zj) in z.iter().enumerate() {
                if j != i {
                    let diff = z[i].sub(zj);
                    if diff.abs() > 0.0 {
                        s = s.add(Complex::new(1.0, 0.0).div(diff));
                    }
                }
            }
            let denom = Complex::new(1.0, 0.0).sub(ratio.mul(s));
            let step = if denom.abs() > 0.0 { ratio.div(denom) } else { ratio };
            if step.re.is_finite() && step.im.is_finite() {
                z[i] = z[i].sub(step);
                moved = moved.max(step.abs() / (1.0 + z[i].abs()));
            }
        }
        if moved < 1e-15 {
            break;
        }
    }
    for zi in &mut z {
        for _ in 0..3 {
            let (p, dp) = eval(*zi);
            if dp.abs() > 0.0 {
                let step = p.div(dp);
                if step.re.is_finite() && step.im.is_finite() {
                    *zi = zi.sub(step);
                }
            }
        }
    }
    z
}

/// Smallest root modulus of `1 + sign·Σ cₖ zᵏ` (∞ when all `cₖ` are zero).
pub(crate) fn min_root_modulus(coefs: &[f64], sign: f64) -> f64 {
    let poly = lag_polynomial(coefs, sign, 1);
    roots(&poly).iter().map(|z| z.abs()).fold(f64::INFINITY, f64::min)
}

/// Make the moving-average operator `1 + Σ θₖ zᵏ` invertible by replacing
/// every root `z` inside the unit circle by `1/z̄`. The autocovariances of the
/// process are unchanged up to a constant factor (absorbed by σ²), so the
/// concentrated likelihood is the same. Returns the new coefficients and
/// whether any root was reflected.
pub(crate) fn make_invertible(theta: &[f64]) -> (Vec<f64>, bool) {
    let poly = lag_polynomial(theta, 1.0, 1);
    let rts = roots(&poly);
    if rts.len() != theta.iter().rposition(|&c| c != 0.0).map_or(0, |k| k + 1) || rts.iter().all(|z| z.abs() >= 1.0) {
        return (theta.to_vec(), false);
    }
    // Rebuild ∏(1 − z/rᵢ) with complex arithmetic; imaginary parts cancel.
    let mut acc = vec![Complex::new(1.0, 0.0)];
    for r in rts {
        let r = if r.abs() < 1.0 { Complex::new(1.0, 0.0).div(r.conj()) } else { r };
        let factor = Complex::new(-1.0, 0.0).div(r);
        let mut next = vec![Complex::new(0.0, 0.0); acc.len() + 1];
        for (k, &a) in acc.iter().enumerate() {
            next[k] = next[k].add(a);
            next[k + 1] = next[k + 1].add(a.mul(factor));
        }
        acc = next;
    }
    let mut out: Vec<f64> = acc[1..].iter().map(|c| c.re).collect();
    out.resize(theta.len(), 0.0);
    (out, true)
}

/// Partial autocorrelations of a stationary AR(p) operator by the inverse
/// Durbin–Levinson recursion; `None` when the operator is not stationary
/// (some partial autocorrelation has modulus ≥ 1).
pub(crate) fn ar_to_partial(phi: &[f64]) -> Option<Vec<f64>> {
    let p = phi.len();
    let mut a = phi.to_vec();
    let mut r = vec![0.0; p];
    for k in (0..p).rev() {
        let rk = a[k];
        if !rk.is_finite() || rk.abs() >= 1.0 {
            return None;
        }
        r[k] = rk;
        let denom = 1.0 - rk * rk;
        let prev: Vec<f64> = (0..k).map(|j| (a[j] + rk * a[k - 1 - j]) / denom).collect();
        a[..k].copy_from_slice(&prev);
    }
    Some(r)
}

/// AR coefficients from partial autocorrelations by the Durbin–Levinson
/// recursion `φ⁽ᵏ⁾ⱼ = φ⁽ᵏ⁻¹⁾ⱼ − rₖ φ⁽ᵏ⁻¹⁾_{k−j}`, `φ⁽ᵏ⁾ₖ = rₖ`.
pub(crate) fn partial_to_ar(r: &[f64]) -> Vec<f64> {
    let mut a: Vec<f64> = Vec::with_capacity(r.len());
    for (k, &rk) in r.iter().enumerate() {
        let next: Vec<f64> = (0..k).map(|j| a[j] - rk * a[k - 1 - j]).collect();
        a = next;
        a.push(rk);
    }
    a
}

/// Unconstrained parameters `u` ↦ stationary AR coefficients through
/// `rₖ = tanh(uₖ)` (Jones 1980).
pub(crate) fn unconstrained_to_ar(u: &[f64]) -> Vec<f64> {
    let r: Vec<f64> = u.iter().map(|x| x.tanh()).collect();
    partial_to_ar(&r)
}

/// Inverse of [`unconstrained_to_ar`]; `None` for a non-stationary operator.
/// Partial autocorrelations are clipped just inside ±1 so the result is finite.
pub(crate) fn ar_to_unconstrained(phi: &[f64]) -> Option<Vec<f64>> {
    ar_to_partial(phi).map(|r| r.iter().map(|x| x.clamp(-0.999_999, 0.999_999).atanh()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_of_airline_operator() {
        let theta = expand_ma(&[-0.4], &[-0.6], 12);
        assert_eq!(theta.len(), 13);
        assert!((theta[0] + 0.4).abs() < 1e-15);
        assert!((theta[11] + 0.6).abs() < 1e-15);
        assert!((theta[12] - 0.24).abs() < 1e-15);
        assert!(theta[1..11].iter().all(|&c| c == 0.0));
        let phi = expand_ar(&[0.5], &[0.3], 4);
        // (1 − .5B)(1 − .3B⁴) = 1 − .5B − .3B⁴ + .15B⁵
        assert_eq!(phi, vec![0.5, 0.0, 0.0, 0.3, -0.15]);
    }

    #[test]
    fn differencing_operator_and_inverse() {
        let delta = differencing_operator(1, 1, 4);
        // (1 − B)(1 − B⁴) = 1 − B − B⁴ + B⁵
        assert_eq!(delta, vec![1.0, 0.0, 0.0, 1.0, -1.0]);
        let u: Vec<f64> = (0..20).map(|t| (t as f64 * 0.7).sin() * 5.0 + t as f64).collect();
        let w = difference(&u, &delta);
        // Integrate back from the first five values.
        let mut back = u[..5].to_vec();
        for (k, &wk) in w.iter().enumerate() {
            let t = k + 5;
            let v = wk + delta.iter().enumerate().map(|(j, &c)| c * back[t - 1 - j]).sum::<f64>();
            back.push(v);
        }
        for (a, b) in back.iter().zip(&u) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn partial_autocorrelation_round_trip() {
        let u = [0.3, -1.2, 0.8, 0.1];
        let phi = unconstrained_to_ar(&u);
        let back = ar_to_unconstrained(&phi).unwrap();
        for (a, b) in u.iter().zip(&back) {
            assert!((a - b).abs() < 1e-12);
        }
        assert!(min_root_modulus(&phi, -1.0) > 1.0);
        assert!(ar_to_partial(&[1.2]).is_none());
        assert!(ar_to_partial(&[0.5, 0.6]).is_none()); // root inside the unit circle
    }

    #[test]
    fn roots_of_known_polynomial() {
        // (1 − z/2)(1 + z/4)(1 + z²) has roots 2, −4, ±i.
        let p = multiply(&multiply(&[1.0, -0.5], &[1.0, 0.25]), &[1.0, 0.0, 1.0]);
        let mut m: Vec<f64> = roots(&p).iter().map(|z| z.abs()).collect();
        m.sort_by(f64::total_cmp);
        let want = [1.0, 1.0, 2.0, 4.0];
        for (a, b) in m.iter().zip(want) {
            assert!((a - b).abs() < 1e-10, "{m:?}");
        }
    }

    #[test]
    fn reflection_makes_ma_invertible() {
        // θ(z) = 1 + 2z has root −0.5; reflected: 1 + 0.5z.
        let (t, changed) = make_invertible(&[2.0]);
        assert!(changed);
        assert!((t[0] - 0.5).abs() < 1e-12);
        let (t, changed) = make_invertible(&[0.3, 0.2]);
        assert!(!changed);
        assert_eq!(t, vec![0.3, 0.2]);
        let (t, changed) = make_invertible(&[-2.5, 1.0]);
        assert!(changed);
        assert!(min_root_modulus(&t, 1.0) >= 1.0 - 1e-12);
    }
}
