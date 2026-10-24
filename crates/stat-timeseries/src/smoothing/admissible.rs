//! Admissibility (forecastability) of seasonal models: every eigenvalue of
//! the discount matrix `D = F − g w′` in the closed unit disc (Hyndman et
//! al. 2008, ch. 10).

use super::recursion::Coefficients;
use super::{EtsSpec, SeasonalType, TrendType};

/// Tolerance on the spectral radius; an unnormalised seasonal model always
/// has the eigenvalue 1 (adding `c` to the level and `−c` to every seasonal
/// state changes no forecast), which must pass.
const TOLERANCE: f64 = 1e-6;

/// Coefficients `θ₀ … θ_K` of `θ(B) = det(I − D B)`, `K` the state
/// dimension. By the matrix determinant lemma,
/// `θ(B) = det(I − F B) · (1 + Σ_{j≥1} c_j B^j)` with `c_j = w′F^{j−1}g =
/// α + β(φ + … + φʲ) + γ·[m divides j]` and
/// `det(I − F B) = (1 − B)(1 − φB)(1 − Bᵐ)` (no `(1 − φB)` without a trend);
/// the product is a polynomial of degree `K`.
fn discount_polynomial(spec: &EtsSpec, c: &Coefficients, m: usize) -> Vec<f64> {
    let has_trend = spec.trend != TrendType::None;
    let k = m + 1 + usize::from(has_trend);
    let mut psi = vec![1.0; k + 1];
    let mut damped_sum = 0.0;
    let mut power = 1.0;
    for (j, v) in psi.iter_mut().enumerate().skip(1) {
        power *= c.phi;
        damped_sum += power;
        *v = c.alpha + if has_trend { c.beta * damped_sum } else { 0.0 } + if j % m == 0 { c.gamma } else { 0.0 };
    }
    // det(I − F B).
    let mut a = vec![0.0; k + 1];
    a[0] = 1.0;
    a[m] = -1.0;
    let mut multiply = |root: f64| {
        for i in (1..=k).rev() {
            a[i] -= root * a[i - 1];
        }
    };
    multiply(1.0);
    if has_trend {
        multiply(c.phi);
    }
    (0..=k).map(|i| (0..=i).map(|j| a[j] * psi[i - j]).sum()).collect()
}

/// Schur–Cohn test: every root of `Σ p_k z^k` strictly inside the unit
/// circle (Schur 1917; Henrici, *Applied and Computational Complex
/// Analysis* I, 1974, §6.8). Each step replaces `p` by
/// `(p_n p(z) − p_0 p*(z))/z`, which keeps the count of roots inside the
/// disc when `|p_0| < |p_n|`.
fn roots_inside_unit_circle(p: &[f64]) -> bool {
    let mut p: Vec<f64> = p.to_vec();
    while p.len() > 1 {
        let n = p.len() - 1;
        let scale = p.iter().fold(0.0_f64, |a, v| a.max(v.abs()));
        if scale == 0.0 || !scale.is_finite() {
            return false;
        }
        for v in &mut p {
            *v /= scale;
        }
        let (p0, pn) = (p[0], p[n]);
        if p0.abs() >= pn.abs() {
            return false;
        }
        p = (0..n).map(|j| pn * p[j + 1] - p0 * p[n - 1 - j]).collect();
    }
    true
}

/// Whether the seasonal model with these coefficients is admissible: the
/// eigenvalues of `D` are the roots of `z^K θ(1/z)`; after dividing out the
/// root `z = 1` the rest must lie in the disc `|z| ≤ 1 + 10⁻⁶`.
pub(crate) fn is_admissible(spec: &EtsSpec, c: &Coefficients, m: usize) -> bool {
    if spec.seasonal == SeasonalType::None {
        return true;
    }
    let theta = discount_polynomial(spec, c, m);
    // p_k = θ_{K−k}: the characteristic polynomial of D, highest power first
    // in `theta`. Synthetic division by (z − 1), highest power first.
    let mut quotient = Vec::with_capacity(theta.len() - 1);
    let mut carry = 0.0;
    for &t in &theta[..theta.len() - 1] {
        carry += t;
        quotient.push(carry);
    }
    let remainder = carry + theta[theta.len() - 1];
    let size = theta.iter().fold(0.0_f64, |a, v| a.max(v.abs()));
    let highest_first = if remainder.abs() <= 1e-9 * size { quotient } else { theta };
    // Ascending powers, scaled so the test is for |z| < 1 + tolerance.
    let r = 1.0 + TOLERANCE;
    let n = highest_first.len() - 1;
    let ascending: Vec<f64> = (0..=n).map(|k| highest_first[n - k] * r.powi(k as i32)).collect();
    roots_inside_unit_circle(&ascending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smoothing::recursion::State;
    use crate::smoothing::{ErrorType, StateVector};

    /// Squarings in the spectral-radius estimate `‖D^k‖^{1/k}`, `k = 2^40`.
    const SQUARINGS: usize = 40;

    /// The discount matrix of the linear (additive-error, additive-season)
    /// analogue of `spec`, acting on `(ℓ, b, s_next, …)`: `D x` is one step of
    /// the recursion with observation 0.
    fn discount_matrix(spec: &EtsSpec, c: &Coefficients, m: usize) -> Vec<Vec<f64>> {
        let linear = EtsSpec::new(ErrorType::Additive, spec.trend, SeasonalType::Additive);
        let has_trend = spec.trend != TrendType::None;
        let dim = 1 + usize::from(has_trend) + m;
        let mut columns = Vec::with_capacity(dim);
        for k in 0..dim {
            let mut x = vec![0.0; dim];
            x[k] = 1.0;
            let (level, rest) = (x[0], &x[1..]);
            let (slope, seasonal) = if has_trend { (rest[0], rest[1..].to_vec()) } else { (0.0, rest.to_vec()) };
            let mut state = State::from_vector(&StateVector { level, slope, seasonal }, &linear);
            let step = state.one_step(c);
            state.update(c, step, -step.mu);
            let v = state.to_vector();
            let mut col = vec![v.level];
            if has_trend {
                col.push(v.slope);
            }
            col.extend(v.seasonal);
            columns.push(col);
        }
        // Transpose the columns into rows.
        (0..dim).map(|i| (0..dim).map(|k| columns[k][i]).collect()).collect()
    }

    fn max_abs(a: &[Vec<f64>]) -> f64 {
        a.iter().flatten().fold(0.0_f64, |acc, v| acc.max(v.abs()))
    }

    /// Spectral radius estimate from repeated squaring with rescaling: after
    /// `J` squarings `‖D^(2^J)‖ = exp(L)` and `ρ ≈ exp(L / 2^J)`.
    fn spectral_radius(d: &[Vec<f64>]) -> f64 {
        let n = d.len();
        let mut scale = max_abs(d);
        if scale == 0.0 || !scale.is_finite() {
            return if scale == 0.0 { 0.0 } else { f64::INFINITY };
        }
        let mut a: Vec<Vec<f64>> = d.iter().map(|r| r.iter().map(|v| v / scale).collect()).collect();
        let mut log_norm = scale.ln();
        let mut power = 1.0_f64;
        for _ in 0..SQUARINGS {
            let mut b = vec![vec![0.0; n]; n];
            for i in 0..n {
                for k in 0..n {
                    let aik = a[i][k];
                    if aik != 0.0 {
                        for j in 0..n {
                            b[i][j] += aik * a[k][j];
                        }
                    }
                }
            }
            scale = max_abs(&b);
            log_norm *= 2.0;
            power *= 2.0;
            if scale == 0.0 {
                return 0.0;
            }
            log_norm += scale.ln();
            a = b.into_iter().map(|r| r.into_iter().map(|v| v / scale).collect()).collect();
        }
        (log_norm / power).exp()
    }

    #[test]
    fn spectral_radius_of_simple_matrices() {
        let d = vec![vec![0.5, 1.0], vec![0.0, 0.25]];
        assert!((spectral_radius(&d) - 0.5).abs() < 1e-6);
        let rot = vec![vec![0.0, -0.9], vec![0.9, 0.0]];
        assert!((spectral_radius(&rot) - 0.9).abs() < 1e-6);
        // Projection-like matrix with eigenvalue 1.
        let p = vec![vec![1.0, 3.0], vec![0.0, 0.2]];
        assert!((spectral_radius(&p) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn usual_holt_winters_parameters_are_admissible_and_extreme_ones_are_not() {
        let spec = EtsSpec::holt_winters_additive();
        let ok = Coefficients { alpha: 0.3, beta: 0.05, gamma: 0.1, phi: 1.0 };
        assert!(is_admissible(&spec, &ok, 12));
        // Very large seasonal smoothing together with a large alpha leaves
        // the admissible region (Hyndman et al. 2008, ch. 10).
        let bad = Coefficients { alpha: 0.9, beta: 0.9, gamma: 0.9, phi: 1.0 };
        assert!(!is_admissible(&spec, &bad, 12));
    }

    fn determinant(mut a: Vec<Vec<f64>>) -> f64 {
        let n = a.len();
        let mut det = 1.0;
        for k in 0..n {
            let p = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs())).unwrap();
            if a[p][k] == 0.0 {
                return 0.0;
            }
            if p != k {
                a.swap(p, k);
                det = -det;
            }
            det *= a[k][k];
            for i in k + 1..n {
                let f = a[i][k] / a[k][k];
                for j in k..n {
                    a[i][j] -= f * a[k][j];
                }
            }
        }
        det
    }

    #[test]
    fn discount_polynomial_is_the_determinant_of_i_minus_d_b() {
        for trend in [TrendType::None, TrendType::Additive, TrendType::DampedAdditive] {
            let spec = EtsSpec::new(ErrorType::Additive, trend, SeasonalType::Additive);
            for m in [2, 4, 7] {
                let c = Coefficients { alpha: 0.4, beta: 0.15, gamma: 0.3, phi: 0.9 };
                let c = if trend == TrendType::DampedAdditive { c } else { Coefficients { phi: 1.0, ..c } };
                let d = discount_matrix(&spec, &c, m);
                let theta = discount_polynomial(&spec, &c, m);
                assert_eq!(theta.len(), d.len() + 1);
                for b in [-1.3, -0.4, 0.3, 0.8, 1.7] {
                    let mat: Vec<Vec<f64>> = (0..d.len())
                        .map(|i| (0..d.len()).map(|j| f64::from(u8::from(i == j)) - b * d[i][j]).collect())
                        .collect();
                    let want = determinant(mat);
                    let got: f64 = theta.iter().enumerate().map(|(k, t)| t * b.powi(k as i32)).sum();
                    assert!((got - want).abs() < 1e-9 * (1.0 + want.abs()), "{trend:?} m={m} B={b}: {got} vs {want}");
                }
                // The unit eigenvalue of every unnormalised seasonal model.
                assert!(theta.iter().sum::<f64>().abs() < 1e-12);
            }
        }
    }

    #[test]
    fn schur_cohn_counts_roots_inside_the_unit_circle() {
        // (z − 0.5)(z + 0.9) = z² + 0.4z − 0.45, ascending powers.
        assert!(roots_inside_unit_circle(&[-0.45, 0.4, 1.0]));
        // (z − 1.1)(z − 0.2) = z² − 1.3z + 0.22.
        assert!(!roots_inside_unit_circle(&[0.22, -1.3, 1.0]));
        // Complex pair of modulus r: z² − 2r cos(1) z + r².
        for (r, inside) in [(0.95, true), (1.05, false)] {
            assert_eq!(roots_inside_unit_circle(&[r * r, -2.0 * r * 1f64.cos(), 1.0]), inside);
        }
    }
}
