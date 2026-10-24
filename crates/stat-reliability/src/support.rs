//! Small numerical helpers shared by the regression models of this crate:
//! symmetric positive-definite solves and inverses (Cholesky), the normal
//! quantile, and the outward root search used for profile-likelihood limits.

use stat_core::{ContinuousDistribution, Normal};

/// A chi-square test statistic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChiSquareTest {
    /// Statistic.
    pub statistic: f64,
    /// Degrees of freedom.
    pub df: usize,
    /// Upper-tail p-value.
    pub p_value: f64,
}

impl ChiSquareTest {
    pub(crate) fn new(statistic: f64, df: usize) -> Self {
        let statistic = statistic.max(0.0);
        Self { statistic, df, p_value: chi_square_sf(statistic, df as f64) }
    }
}

/// `A⁻¹` of a symmetric positive-definite matrix, or `None` when its
/// Cholesky factorization fails (not positive definite or nearly singular).
pub(crate) fn spd_inverse(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let p = a.len();
    if p == 0 {
        return Some(Vec::new());
    }
    let l = stat_core::eigen::cholesky(a)?;
    // Columns of the inverse: solve L Lᵀ x = eⱼ.
    let mut inv = vec![vec![0.0; p]; p];
    for j in 0..p {
        let mut e = vec![0.0; p];
        e[j] = 1.0;
        let x = cholesky_solve(&l, &e);
        for i in 0..p {
            inv[i][j] = x[i];
        }
    }
    // Symmetrize against rounding.
    for i in 0..p {
        for j in 0..i {
            let m = 0.5 * (inv[i][j] + inv[j][i]);
            inv[i][j] = m;
            inv[j][i] = m;
        }
    }
    Some(inv)
}

/// Solve `A x = b` for symmetric positive-definite `A`.
pub(crate) fn spd_solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    if a.is_empty() {
        return Some(Vec::new());
    }
    let l = stat_core::eigen::cholesky(a)?;
    Some(cholesky_solve(&l, b))
}

fn cholesky_solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let p = b.len();
    let mut z = vec![0.0; p];
    for i in 0..p {
        z[i] = (b[i] - (0..i).map(|k| l[i][k] * z[k]).sum::<f64>()) / l[i][i];
    }
    let mut x = vec![0.0; p];
    for i in (0..p).rev() {
        x[i] = (z[i] - (i + 1..p).map(|k| l[k][i] * x[k]).sum::<f64>()) / l[i][i];
    }
    x
}

/// `aᵀ M b`.
pub(crate) fn quadratic(m: &[Vec<f64>], a: &[f64], b: &[f64]) -> f64 {
    a.iter().enumerate().map(|(i, ai)| ai * m[i].iter().zip(b).map(|(mij, bj)| mij * bj).sum::<f64>()).sum()
}

/// The standard normal quantile `z_{p}`.
pub(crate) fn normal_quantile(p: f64) -> f64 {
    Normal::standard().quantile(p).unwrap_or(f64::NAN)
}

/// Two-sided standard normal p-value of `z`.
pub(crate) fn normal_two_sided(z: f64) -> f64 {
    if z.is_nan() {
        return f64::NAN;
    }
    2.0 * Normal::standard().sf(z.abs()).unwrap_or(f64::NAN)
}

/// Upper-tail chi-square probability.
pub(crate) fn chi_square_sf(x: f64, df: f64) -> f64 {
    if !(x.is_finite() && df > 0.0) {
        return f64::NAN;
    }
    stat_core::ChiSquared::new(df).and_then(|c| c.sf(x.max(0.0))).unwrap_or(f64::NAN)
}

/// The value `c` beyond `start` (in the direction of `step`'s sign) where
/// `g(c)` changes from negative to non-negative, with `g(start) < 0`: the
/// end of a profile-likelihood interval when `g(c) = 2[ℓ̂ − ℓₚ(c)] − χ²`.
///
/// The bracket is found by stepping outward from `start + step`, the step
/// growing by 1.6 each time (at most 60 steps), and the crossing located by
/// the Illinois variant of regula falsi to a relative width of `1e-10`.
/// `None` when no crossing is found (the likelihood stays within the
/// cut-off, so that end of the interval is unbounded) or `g` fails.
pub(crate) fn find_crossing(mut g: impl FnMut(f64) -> Option<f64>, start: f64, step: f64) -> Option<f64> {
    if !(step.is_finite() && step != 0.0) {
        return None;
    }
    let (mut a, mut fa) = (start, g(start)?);
    if fa >= 0.0 {
        return None;
    }
    let mut width = step;
    let (mut b, mut fb);
    let mut tries = 0;
    loop {
        b = a + width;
        fb = g(b)?;
        if fb >= 0.0 {
            break;
        }
        a = b;
        fa = fb;
        width *= 1.6;
        tries += 1;
        if tries > 60 {
            return None;
        }
    }
    // Illinois regula falsi on [a, b] with fa < 0 ≤ fb.
    let mut side = 0;
    for _ in 0..200 {
        let c = if (fb - fa).abs() > 0.0 { b - fb * (b - a) / (fb - fa) } else { 0.5 * (a + b) };
        let c = if c.is_finite() && (c - a) * (c - b) <= 0.0 { c } else { 0.5 * (a + b) };
        let fc = g(c)?;
        if fc.abs() < 1e-12 || (b - a).abs() <= 1e-10 * (1.0 + c.abs()) {
            return Some(c);
        }
        if fc < 0.0 {
            a = c;
            fa = fc;
            if side == -1 {
                fb *= 0.5;
            }
            side = -1;
        } else {
            b = c;
            fb = fc;
            if side == 1 {
                fa *= 0.5;
            }
            side = 1;
        }
    }
    Some(0.5 * (a + b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_and_solve() {
        let a = vec![vec![4.0, 1.0, 0.5], vec![1.0, 3.0, 0.2], vec![0.5, 0.2, 2.0]];
        let inv = spd_inverse(&a).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                let v: f64 = (0..3).map(|k| a[i][k] * inv[k][j]).sum();
                assert!((v - f64::from(u8::from(i == j))).abs() < 1e-12);
            }
        }
        let x = spd_solve(&a, &[1.0, 2.0, 3.0]).unwrap();
        let back: Vec<f64> = (0..3).map(|i| (0..3).map(|k| a[i][k] * x[k]).sum()).collect();
        assert!((back[2] - 3.0).abs() < 1e-12);
        assert!(spd_inverse(&[vec![1.0, 2.0], vec![2.0, 1.0]]).is_none());
    }

    #[test]
    fn crossing() {
        let c = find_crossing(|x| Some(x * x - 4.0), 0.0, 0.5).unwrap();
        assert!((c - 2.0).abs() < 1e-9);
        let c = find_crossing(|x| Some(x * x - 4.0), 0.0, -0.5).unwrap();
        assert!((c + 2.0).abs() < 1e-9);
        assert!(find_crossing(|_| Some(-1.0), 0.0, 1.0).is_none());
    }
}
