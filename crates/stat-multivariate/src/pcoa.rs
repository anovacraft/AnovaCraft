//! Principal coordinates analysis (PCoA, metric multidimensional scaling of
//! any distance matrix; Gower 1966, “Some distance properties of latent
//! root and vector methods used in multivariate analysis”, *Biometrika* 53).
//!
//! From distances `dᵢⱼ` form `A = [−½ dᵢⱼ²]` and Gower's centred matrix
//! `Δ₁ = J A J` with `J = I − 11ᵀ/n`. Its eigenvalues `λ₁ ≥ λ₂ ≥ …` and
//! unit eigenvectors `vₖ` give the coordinates `√λₖ vₖ` on every axis with
//! `λₖ > 0`. One eigenvalue always belongs to the constant vector `1` and
//! is zero; the other `n − 1` are reported. When the distances are not
//! Euclidean some eigenvalues are negative: the axes they would define are
//! imaginary and carry no coordinates.
//!
//! Two classical corrections add a constant to the distances so that every
//! eigenvalue becomes non-negative (Legendre & Legendre 2012, *Numerical
//! Ecology*, 3rd ed., §9.3.4; Legendre & Anderson 1999, *Ecological
//! Monographs* 69):
//!
//! * **Lingoes** (1971): `d′ᵢⱼ = √(dᵢⱼ² + 2c)` for `i ≠ j`, with
//!   `c = |λₘᵢₙ|` the magnitude of the most negative eigenvalue of `Δ₁`;
//! * **Cailliez** (1983, “The analytical solution of the additive constant
//!   problem”, *Psychometrika* 48): `d′ᵢⱼ = dᵢⱼ + c` for `i ≠ j`, with `c`
//!   the largest (real) eigenvalue of the `2n × 2n` matrix
//!   `[[0, 2Δ₁], [−I, −4Δ₂]]`, where `Δ₂ = J [−½ dᵢⱼ] J`.
//!
//! The corrected matrix is analysed in the same way; its eigenvalues
//! include one more zero. A correction is only applied when there is a
//! negative eigenvalue.
//!
//! Eigenvalues within `10⁻¹⁰ · max |λ|` of zero are set to zero. The
//! relative eigenvalues are `λₖ / Σ λ⁺` (the sum over positive eigenvalues),
//! and the broken-stick expectations for `k` positive axes are
//! `bⱼ = (1/k) Σᵢ₌ⱼᵏ 1/i` (Frontier 1976; Jackson 1993, *Ecology* 74).
//! Eigenvector signs are arbitrary: each axis is flipped so that its
//! largest absolute coordinate is positive.
//!
//! The Cailliez constant needs eigenvalues of a non-symmetric matrix; they
//! are found by Householder reduction to Hessenberg form and the shifted
//! double-step QR algorithm (Wilkinson & Reinsch 1971, *Handbook for
//! Automatic Computation* vol. II, procedures `orthes` and `hqr`, as in the
//! public-domain JAMA translation).

use stat_core::{Result, StatError};

use crate::distance::DistanceMatrix;
use crate::sym_eigen::symmetric_eigen;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Correction for negative eigenvalues of non-Euclidean distances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NegativeCorrection {
    /// Analyse the distances as given; negative eigenvalues are reported.
    None,
    /// Lingoes' correction, `√(d² + 2c)`.
    Lingoes,
    /// Cailliez's additive constant, `d + c`.
    Cailliez,
}

impl NegativeCorrection {
    /// Every option, in menu order.
    pub const ALL: [Self; 3] = [Self::None, Self::Lingoes, Self::Cailliez];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Lingoes => "Lingoes (add to squared distances)",
            Self::Cailliez => "Cailliez (add to distances)",
        }
    }

    /// Stable key used in saved requests and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lingoes => "lingoes",
            Self::Cailliez => "cailliez",
        }
    }

    /// The option with this [`key`](Self::key), if any.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }
}

/// A principal coordinates solution.
#[derive(Debug, Clone, PartialEq)]
pub struct PrincipalCoordinates {
    /// The `n − 1` eigenvalues of Gower's centred matrix (the constant
    /// vector's zero left out), largest first, negatives kept; after the
    /// correction when one was applied.
    pub eigenvalues: Vec<f64>,
    /// Each eigenvalue over the sum of the positive eigenvalues.
    pub relative: Vec<f64>,
    /// Running sums of [`relative`](Self::relative).
    pub cumulative: Vec<f64>,
    /// Broken-stick expected shares, one per positive eigenvalue.
    pub broken_stick: Vec<f64>,
    /// Coordinates: `n` rows, one column per positive eigenvalue up to the
    /// requested maximum.
    pub coordinates: Vec<Vec<f64>>,
    /// The constant `c` added by the correction, when one was applied.
    pub correction_constant: Option<f64>,
    /// The `n − 1` eigenvalues before any correction.
    pub raw_eigenvalues: Vec<f64>,
}

impl PrincipalCoordinates {
    /// Number of axes with a positive eigenvalue.
    #[must_use]
    pub fn positive_axes(&self) -> usize {
        self.broken_stick.len()
    }

    /// Whether the (uncorrected) distances had negative eigenvalues,
    /// meaning they are not Euclidean.
    #[must_use]
    pub fn has_negative_eigenvalues(&self) -> bool {
        self.raw_eigenvalues.iter().any(|v| *v < 0.0)
    }
}

/// Principal coordinates of the distances `d`, with an optional correction
/// for negative eigenvalues, keeping at most `max_axes` axes.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two objects,
/// `max_axes == 0`, or distances that are all zero;
/// [`StatError::ConvergenceFailure`] if an eigenvalue iteration fails (not
/// expected for finite input).
pub fn principal_coordinates(
    d: &DistanceMatrix,
    correction: NegativeCorrection,
    max_axes: usize,
) -> Result<PrincipalCoordinates> {
    let n = d.n();
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    if max_axes == 0 {
        return Err(invalid("keep at least one axis"));
    }
    let raw = gower_eigen(n, |i, j| d.get(i, j).powi(2))?;
    let min = raw.values.last().copied().unwrap_or(0.0);
    let (fit, constant) = if min < 0.0 && correction != NegativeCorrection::None {
        let c = match correction {
            NegativeCorrection::Lingoes => -min,
            NegativeCorrection::Cailliez => cailliez_constant(d)?,
            NegativeCorrection::None => unreachable!("handled above"),
        };
        let corrected = match correction {
            NegativeCorrection::Lingoes => {
                gower_eigen(n, |i, j| if i == j { 0.0 } else { d.get(i, j).powi(2) + 2.0 * c })?
            }
            _ => gower_eigen(n, |i, j| if i == j { 0.0 } else { (d.get(i, j) + c).powi(2) })?,
        };
        (corrected, Some(c))
    } else {
        (raw.clone(), None)
    };
    let positive: f64 = fit.values.iter().filter(|v| **v > 0.0).sum();
    let relative: Vec<f64> = fit.values.iter().map(|v| v / positive).collect();
    let cumulative = relative
        .iter()
        .scan(0.0, |s, v| {
            *s += v;
            Some(*s)
        })
        .collect();
    let k = fit.values.iter().filter(|v| **v > 0.0).count();
    let broken_stick = (1..=k).map(|j| (j..=k).map(|i| 1.0 / i as f64).sum::<f64>() / k as f64).collect::<Vec<_>>();
    let axes = k.min(max_axes);
    let mut coordinates: Vec<Vec<f64>> =
        (0..n).map(|i| (0..axes).map(|a| fit.vectors[i][a] * fit.values[a].sqrt()).collect()).collect();
    for a in 0..axes {
        let big = coordinates.iter().map(|r| r[a]).fold(0.0_f64, |m, v| if v.abs() > m.abs() { v } else { m });
        if big < 0.0 {
            coordinates.iter_mut().for_each(|r| r[a] = -r[a]);
        }
    }
    Ok(PrincipalCoordinates {
        eigenvalues: fit.values,
        relative,
        cumulative,
        broken_stick,
        coordinates,
        correction_constant: constant,
        raw_eigenvalues: raw.values,
    })
}

/// The `n − 1` eigenpairs of a Gower-centred matrix.
#[derive(Debug, Clone)]
struct GowerEigen {
    values: Vec<f64>,
    /// `vectors[i][k]`: component `i` of the eigenvector of `values[k]`.
    vectors: Vec<Vec<f64>>,
}

/// `J m J` for the symmetric `n × n` matrix `m`.
fn centre(m: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = m.len();
    let row: Vec<f64> = m.iter().map(|r| r.iter().sum::<f64>() / n as f64).collect();
    let grand = row.iter().sum::<f64>() / n as f64;
    (0..n).map(|i| (0..n).map(|j| m[i][j] - row[i] - row[j] + grand).collect()).collect()
}

/// Eigen-decomposition of `J [−½ sq(i, j)] J`, leaving out the eigenvalue
/// of the constant vector and zeroing negligible eigenvalues.
fn gower_eigen(n: usize, sq: impl Fn(usize, usize) -> f64) -> Result<GowerEigen> {
    let a: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| -0.5 * sq(i, j)).collect()).collect();
    let (mut values, vectors) = symmetric_eigen(&centre(&a))?;
    let scale = values.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    if scale == 0.0 {
        return Err(invalid("all distances are zero, so there are no principal coordinates"));
    }
    let tol = 1e-10 * scale;
    for v in &mut values {
        if v.abs() <= tol {
            *v = 0.0;
        }
    }
    // The constant vector's eigenvalue: among the zero eigenvalues, the one
    // whose eigenvector is most nearly constant.
    let sum = |k: usize| vectors.iter().map(|r| r[k]).sum::<f64>().abs();
    let candidates: Vec<usize> = (0..n).filter(|&k| values[k] == 0.0).collect();
    let pool = if candidates.is_empty() { (0..n).collect() } else { candidates };
    let drop = pool.into_iter().fold(None, |best: Option<usize>, k| match best {
        Some(b) if sum(b) >= sum(k) => Some(b),
        _ => Some(k),
    });
    let drop = drop.unwrap_or(n - 1);
    let keep: Vec<usize> = (0..n).filter(|&k| k != drop).collect();
    Ok(GowerEigen {
        values: keep.iter().map(|&k| values[k]).collect(),
        vectors: vectors.iter().map(|r| keep.iter().map(|&k| r[k]).collect()).collect(),
    })
}

/// Cailliez's additive constant: the largest real part among the
/// eigenvalues of `[[0, 2Δ₁], [−I, −4Δ₂]]`.
fn cailliez_constant(d: &DistanceMatrix) -> Result<f64> {
    let n = d.n();
    let half = |p: i32| -> Vec<Vec<f64>> {
        centre(&(0..n).map(|i| (0..n).map(|j| -0.5 * d.get(i, j).powi(p)).collect()).collect::<Vec<_>>())
    };
    let (d1, d2) = (half(2), half(1));
    let mut m = vec![vec![0.0; 2 * n]; 2 * n];
    for i in 0..n {
        for j in 0..n {
            m[i][n + j] = 2.0 * d1[i][j];
            m[n + i][n + j] = -4.0 * d2[i][j];
        }
        m[n + i][i] = -1.0;
    }
    let eig = real_eigenvalues(m)?;
    Ok(eig.iter().map(|e| e.0).fold(f64::NEG_INFINITY, f64::max))
}

/// Eigenvalues `(real, imaginary)` of a general real square matrix, by
/// Householder reduction to upper Hessenberg form (`orthes`) and the
/// shifted double-step QR algorithm (`hqr`).
#[allow(clippy::many_single_char_names, clippy::too_many_lines, unused_assignments)]
fn real_eigenvalues(mut h: Vec<Vec<f64>>) -> Result<Vec<(f64, f64)>> {
    let nn = h.len();
    if nn == 0 {
        return Ok(Vec::new());
    }
    // Hessenberg reduction.
    let high = nn - 1;
    let mut ort = vec![0.0; nn];
    for m in 1..high {
        let scale: f64 = (m..=high).map(|i| h[i][m - 1].abs()).sum();
        if scale == 0.0 {
            continue;
        }
        let mut hh = 0.0;
        for i in (m..=high).rev() {
            ort[i] = h[i][m - 1] / scale;
            hh += ort[i] * ort[i];
        }
        let g = if ort[m] > 0.0 { -hh.sqrt() } else { hh.sqrt() };
        hh -= ort[m] * g;
        ort[m] -= g;
        for j in m..nn {
            let f = (m..=high).rev().map(|i| ort[i] * h[i][j]).sum::<f64>() / hh;
            for i in m..=high {
                h[i][j] -= f * ort[i];
            }
        }
        for row in h.iter_mut().take(high + 1) {
            let f = (m..=high).rev().map(|j| ort[j] * row[j]).sum::<f64>() / hh;
            for j in m..=high {
                row[j] -= f * ort[j];
            }
        }
        ort[m] *= scale;
        h[m][m - 1] = scale * g;
    }

    // QR iterations on the Hessenberg matrix.
    let mut d = vec![0.0; nn];
    let mut e = vec![0.0; nn];
    let eps = f64::EPSILON;
    let mut exshift = 0.0;
    let (mut p, mut q, mut r, mut s, mut z) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut norm = 0.0;
    for i in 0..nn {
        for j in i.saturating_sub(1)..nn {
            norm += h[i][j].abs();
        }
    }
    let mut n = nn as isize - 1;
    let mut iter = 0usize;
    let mut total = 0usize;
    while n >= 0 {
        let nu = n as usize;
        // Look for a single small sub-diagonal element.
        let mut l = nu;
        while l > 0 {
            s = h[l - 1][l - 1].abs() + h[l][l].abs();
            if s == 0.0 {
                s = norm;
            }
            if h[l][l - 1].abs() < eps * s {
                break;
            }
            l -= 1;
        }
        if l == nu {
            // One root found.
            h[nu][nu] += exshift;
            d[nu] = h[nu][nu];
            e[nu] = 0.0;
            n -= 1;
            iter = 0;
        } else if l + 1 == nu {
            // Two roots found.
            let w = h[nu][nu - 1] * h[nu - 1][nu];
            p = (h[nu - 1][nu - 1] - h[nu][nu]) / 2.0;
            q = p * p + w;
            z = q.abs().sqrt();
            h[nu][nu] += exshift;
            h[nu - 1][nu - 1] += exshift;
            let x = h[nu][nu];
            if q >= 0.0 {
                z = if p >= 0.0 { p + z } else { p - z };
                d[nu - 1] = x + z;
                d[nu] = d[nu - 1];
                if z != 0.0 {
                    d[nu] = x - w / z;
                }
                e[nu - 1] = 0.0;
                e[nu] = 0.0;
            } else {
                d[nu - 1] = x + p;
                d[nu] = x + p;
                e[nu - 1] = z;
                e[nu] = -z;
            }
            n -= 2;
            iter = 0;
        } else {
            // No convergence yet: form the shift.
            let mut x = h[nu][nu];
            let mut y = 0.0;
            let mut w = 0.0;
            if l < nu {
                y = h[nu - 1][nu - 1];
                w = h[nu][nu - 1] * h[nu - 1][nu];
            }
            if iter == 10 {
                // Wilkinson's exceptional shift.
                exshift += x;
                for i in 0..=nu {
                    h[i][i] -= x;
                }
                s = h[nu][nu - 1].abs() + h[nu - 1][nu - 2].abs();
                x = 0.75 * s;
                y = x;
                w = -0.4375 * s * s;
            }
            if iter == 30 {
                // A second exceptional shift.
                s = (y - x) / 2.0;
                s = s * s + w;
                if s > 0.0 {
                    s = s.sqrt();
                    if y < x {
                        s = -s;
                    }
                    s = x - w / ((y - x) / 2.0 + s);
                    for i in 0..=nu {
                        h[i][i] -= s;
                    }
                    exshift += s;
                    x = 0.964;
                    y = x;
                    w = x;
                }
            }
            iter += 1;
            total += 1;
            if total > 60 * nn {
                return Err(StatError::ConvergenceFailure { routine: "Hessenberg QR eigenvalues", iterations: total });
            }
            // Look for two consecutive small sub-diagonal elements.
            let mut m = nu - 2;
            loop {
                z = h[m][m];
                r = x - z;
                s = y - z;
                p = (r * s - w) / h[m + 1][m] + h[m][m + 1];
                q = h[m + 1][m + 1] - z - r - s;
                r = h[m + 2][m + 1];
                s = p.abs() + q.abs() + r.abs();
                p /= s;
                q /= s;
                r /= s;
                if m == l {
                    break;
                }
                if h[m][m - 1].abs() * (q.abs() + r.abs())
                    < eps * (p.abs() * (h[m - 1][m - 1].abs() + z.abs() + h[m + 1][m + 1].abs()))
                {
                    break;
                }
                m -= 1;
            }
            for i in m + 2..=nu {
                h[i][i - 2] = 0.0;
                if i > m + 2 {
                    h[i][i - 3] = 0.0;
                }
            }
            // Double QR step on rows l..=n and columns m..=n.
            for k in m..nu {
                let notlast = k + 1 != nu;
                let mut xk = 0.0;
                if k != m {
                    p = h[k][k - 1];
                    q = h[k + 1][k - 1];
                    r = if notlast { h[k + 2][k - 1] } else { 0.0 };
                    xk = p.abs() + q.abs() + r.abs();
                    if xk == 0.0 {
                        continue;
                    }
                    p /= xk;
                    q /= xk;
                    r /= xk;
                }
                s = (p * p + q * q + r * r).sqrt();
                if p < 0.0 {
                    s = -s;
                }
                if s != 0.0 {
                    if k != m {
                        h[k][k - 1] = -s * xk;
                    } else if l != m {
                        h[k][k - 1] = -h[k][k - 1];
                    }
                    p += s;
                    let xs = p / s;
                    let ys = q / s;
                    z = r / s;
                    q /= p;
                    r /= p;
                    for j in k..nn {
                        p = h[k][j] + q * h[k + 1][j];
                        if notlast {
                            p += r * h[k + 2][j];
                            h[k + 2][j] -= p * z;
                        }
                        h[k][j] -= p * xs;
                        h[k + 1][j] -= p * ys;
                    }
                    for i in 0..=nu.min(k + 3) {
                        p = xs * h[i][k] + ys * h[i][k + 1];
                        if notlast {
                            p += z * h[i][k + 2];
                            h[i][k + 2] -= p * r;
                        }
                        h[i][k] -= p;
                        h[i][k + 1] -= p * q;
                    }
                }
            }
        }
    }
    Ok(d.into_iter().zip(e).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distance::{DistanceColumn, DistanceMetric, VariableKind, distance_matrix};
    use crate::embedding::Proximity;
    use crate::mds::classical_scaling;

    fn points() -> Vec<Vec<f64>> {
        let m = crate::pca::tests::data();
        (0..m.nrows()).map(|i| m.row(i)).collect()
    }

    fn columns(points: &[Vec<f64>]) -> Vec<DistanceColumn> {
        (0..points[0].len())
            .map(|k| DistanceColumn { values: points.iter().map(|r| r[k]).collect(), kind: VariableKind::Numeric })
            .collect()
    }

    #[test]
    fn keys_round_trip() {
        for c in NegativeCorrection::ALL {
            assert_eq!(NegativeCorrection::from_key(c.key()), Some(c));
        }
        assert_eq!(NegativeCorrection::from_key("other"), None);
    }

    /// On Euclidean distances PCoA equals classical scaling (and so the
    /// principal component scores), up to the sign of each axis.
    #[test]
    fn euclidean_distances_give_classical_scaling() {
        let pts = points();
        let d = distance_matrix(&columns(&pts), DistanceMetric::Euclidean).unwrap();
        let p = principal_coordinates(&d, NegativeCorrection::Lingoes, 2).unwrap();
        assert!(p.correction_constant.is_none(), "Euclidean distances need no correction");
        assert!(!p.has_negative_eigenvalues());
        let c = classical_scaling(&Proximity::Points(&pts), 2).unwrap();
        for k in 0..2 {
            assert!((p.eigenvalues[k] - c.eigenvalues[k]).abs() < 1e-10 * c.eigenvalues[0]);
            let sign = if p.coordinates[0][k] * c.coordinates[0][k] < 0.0 { -1.0 } else { 1.0 };
            for (a, b) in p.coordinates.iter().zip(&c.coordinates) {
                assert!((a[k] - sign * b[k]).abs() < 1e-9, "axis {k}");
            }
        }
        assert_eq!(p.eigenvalues.len(), pts.len() - 1);
        assert_eq!(p.positive_axes(), pts[0].len().min(pts.len() - 1));
        assert!((p.broken_stick.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert!((p.cumulative.last().unwrap() - 1.0).abs() < 1e-12);
    }

    /// Both corrections remove the negative eigenvalues, and the corrected
    /// distances are Euclidean with (at least) one more zero eigenvalue.
    #[test]
    fn corrections_make_the_distances_euclidean() {
        let pts = points();
        let d = distance_matrix(&columns(&pts), DistanceMetric::BrayCurtis).unwrap();
        let none = principal_coordinates(&d, NegativeCorrection::None, usize::MAX).unwrap();
        assert!(none.has_negative_eigenvalues());
        assert!(none.correction_constant.is_none());
        assert_eq!(none.coordinates[0].len(), none.positive_axes());
        for corr in [NegativeCorrection::Lingoes, NegativeCorrection::Cailliez] {
            let p = principal_coordinates(&d, corr, usize::MAX).unwrap();
            let c = p.correction_constant.unwrap();
            assert!(c > 0.0);
            assert!(p.eigenvalues.iter().all(|v| *v >= 0.0), "{corr:?}: {:?}", p.eigenvalues);
            assert_eq!(*p.eigenvalues.last().unwrap(), 0.0, "{corr:?}");
            assert_eq!(p.raw_eigenvalues, none.eigenvalues);
            // A slightly smaller constant leaves a negative eigenvalue.
            let n = d.n();
            let shrunk: Vec<f64> = (0..n * n)
                .map(|ij| {
                    let (i, j) = (ij / n, ij % n);
                    if i == j {
                        0.0
                    } else if corr == NegativeCorrection::Lingoes {
                        (d.get(i, j).powi(2) + 2.0 * 0.99 * c).sqrt()
                    } else {
                        d.get(i, j) + 0.99 * c
                    }
                })
                .collect();
            let s =
                principal_coordinates(&DistanceMatrix::new(n, shrunk).unwrap(), NegativeCorrection::None, 1).unwrap();
            assert!(s.has_negative_eigenvalues(), "{corr:?}");
        }
    }

    /// The general eigenvalue routine on a companion matrix with known
    /// roots 1, 2, 3 and ±2i: x⁵ − 6x⁴ + 15x³ − 30x² + 44x − 24.
    #[test]
    fn hessenberg_qr_finds_real_and_complex_roots() {
        let coef = [-6.0, 15.0, -30.0, 44.0, -24.0];
        let mut m = vec![vec![0.0; 5]; 5];
        for (j, c) in coef.iter().enumerate() {
            m[0][j] = -c;
        }
        for i in 1..5 {
            m[i][i - 1] = 1.0;
        }
        let mut eig = real_eigenvalues(m).unwrap();
        eig.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let want = [(0.0, -2.0), (0.0, 2.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)];
        for (g, w) in eig.iter().zip(want) {
            assert!((g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9, "{eig:?}");
        }
    }

    #[test]
    fn rejects_bad_input() {
        let d = DistanceMatrix::new(1, vec![0.0]).unwrap();
        assert!(principal_coordinates(&d, NegativeCorrection::None, 2).is_err());
        let z = DistanceMatrix::new(3, vec![0.0; 9]).unwrap();
        assert!(principal_coordinates(&z, NegativeCorrection::None, 2).is_err());
        let ok = DistanceMatrix::new(2, vec![0.0, 1.0, 1.0, 0.0]).unwrap();
        assert!(principal_coordinates(&ok, NegativeCorrection::None, 0).is_err());
        let p = principal_coordinates(&ok, NegativeCorrection::None, 3).unwrap();
        assert_eq!(p.eigenvalues.len(), 1);
        assert!((p.eigenvalues[0] - 0.5).abs() < 1e-15);
        assert!((p.coordinates[0][0].abs() - 0.5).abs() < 1e-15);
    }
}
