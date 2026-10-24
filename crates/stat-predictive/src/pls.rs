//! Partial least squares (PLS) regression.
//!
//! **Model.** PLS finds a few latent factors (scores) `t = X r`, linear
//! combinations of the centred (and usually standardized) predictors chosen
//! to have large covariance with the responses, and regresses the responses
//! on them (Wold, Sjöström & Eriksson, "PLS-regression: a basic tool of
//! chemometrics", *Chemometrics and Intelligent Laboratory Systems* 58,
//! 2001). With `a` factors, `X ≈ T Pᵀ` and `Y ≈ T Qᵀ`, so the coefficients
//! are `B = R Qᵀ` with `T = X R`. It suits many correlated predictors and
//! fewer rows than predictors, where least squares fails.
//!
//! **Algorithms.**
//! * *NIPALS*: each factor's weight vector `w` is the dominant direction of
//!   `Xᵀ Y` on the deflated matrices (`X ← X − t pᵀ`, `Y ← Y − t qᵀ`),
//!   found by alternating `w ∝ Xᵀu`, `t = Xw`, `q = Yᵀt/tᵀt`, `u = Yq/qᵀq`
//!   (one step for a single response). The rotations follow from
//!   `r_a = w_a − Σ_{i<a} r_i (p_iᵀ w_a)`, so `t_a = X₀ r_a`.
//! * *SIMPLS* (de Jong, *Chemometrics and Intelligent Laboratory Systems*
//!   18, 1993): `r` is the dominant left singular vector of the
//!   cross-product `S = X₀ᵀY₀`, which is deflated by projecting out the
//!   loadings found so far; `X` itself is never deflated. For a single
//!   response both algorithms give the same model.
//!
//! **Summaries.** Variance of `X` and `Y` explained by each factor
//! (`tᵀt ‖p‖²` and `tᵀt ‖q‖²` over the total sums of squares, scores being
//! orthogonal); variable importance in projection
//! `VIP_j = √(p Σ_a SSY_a (w_aj/‖w_a‖)² / Σ_a SSY_a)` (Wold; Chong & Jun,
//! *Chemometrics and Intelligent Laboratory Systems* 78, 2005), where
//! values above about 1 mark influential predictors.

use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// How the factors are extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlsAlgorithm {
    /// Nonlinear iterative partial least squares (deflating X and Y).
    Nipals,
    /// Straightforward implementation of PLS (de Jong 1993).
    Simpls,
}

/// A fitted PLS model with every factor extracted; predictions and
/// coefficients may use any number of leading factors.
#[derive(Debug, Clone, PartialEq)]
pub struct PlsFit {
    /// Algorithm used.
    pub algorithm: PlsAlgorithm,
    /// Predictor means.
    pub x_mean: Vec<f64>,
    /// Predictor scales (standard deviations, or 1 when not scaled).
    pub x_scale: Vec<f64>,
    /// Response means.
    pub y_mean: Vec<f64>,
    /// Response scales.
    pub y_scale: Vec<f64>,
    /// `weights[a]`: factor `a`'s weight vector (NIPALS: unit length, applied
    /// to the deflated predictors; SIMPLS: the same as the rotation).
    pub weights: Vec<Vec<f64>>,
    /// `rotations[a]`: `r_a` with `t_a = X₀ r_a` (X centred and scaled).
    pub rotations: Vec<Vec<f64>>,
    /// `x_loadings[a]`: `p_a = X₀ᵀ t_a / t_aᵀt_a`.
    pub x_loadings: Vec<Vec<f64>>,
    /// `y_loadings[a]`: `q_a = Y₀ᵀ t_a / t_aᵀt_a`.
    pub y_loadings: Vec<Vec<f64>>,
    /// `scores[a]`: factor `a`'s scores on the fitted rows.
    pub scores: Vec<Vec<f64>>,
    /// Share of the scaled predictors' sum of squares each factor explains.
    pub x_explained: Vec<f64>,
    /// Share of the scaled responses' sum of squares each factor explains.
    pub y_explained: Vec<f64>,
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

/// `Mᵀ v` for a matrix given by columns.
fn t_mul(cols: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    cols.iter().map(|c| dot(c, v)).collect()
}

/// `M v` for a matrix given by columns of length `n`.
fn mul(cols: &[Vec<f64>], v: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![0.0; n];
    for (c, &vj) in cols.iter().zip(v) {
        if vj != 0.0 {
            for (o, x) in out.iter_mut().zip(c) {
                *o += vj * x;
            }
        }
    }
    out
}

/// Centre (and scale) columns; returns them with their means and scales.
fn standardize(cols: &[Vec<f64>], scale: bool) -> (Vec<Vec<f64>>, Vec<f64>, Vec<f64>) {
    let mut means = Vec::with_capacity(cols.len());
    let mut scales = Vec::with_capacity(cols.len());
    let out = cols
        .iter()
        .map(|c| {
            let n = c.len() as f64;
            let m = c.iter().sum::<f64>() / n;
            let sd = (c.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt();
            let s = if scale && sd > 0.0 { sd } else { 1.0 };
            means.push(m);
            scales.push(s);
            c.iter().map(|v| (v - m) / s).collect()
        })
        .collect();
    (out, means, scales)
}

/// Fit PLS with up to `factors` factors (fewer if the predictors run out
/// of variation), predictors `x` and responses `y` given as columns of the
/// same length.
///
/// # Errors
/// Fewer than two rows, no predictors or responses, columns of different
/// lengths, non-finite values, or zero factors.
pub fn fit_pls(
    x: &[Vec<f64>],
    y: &[Vec<f64>],
    factors: usize,
    scale_x: bool,
    scale_y: bool,
    algorithm: PlsAlgorithm,
) -> Result<PlsFit> {
    let n = x.first().map_or(0, Vec::len);
    if x.is_empty() || y.is_empty() {
        return Err(invalid("PLS needs at least one predictor and one response"));
    }
    if n < 2 {
        return Err(invalid("PLS needs at least two rows"));
    }
    if x.iter().chain(y).any(|c| c.len() != n) {
        return Err(invalid("every column must have one value per row"));
    }
    if x.iter().chain(y).flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "PLS data" });
    }
    if factors == 0 {
        return Err(invalid("PLS needs at least one factor"));
    }
    let (x0, x_mean, x_scale) = standardize(x, scale_x);
    let (y0, y_mean, y_scale) = standardize(y, scale_y);
    let x_total: f64 = x0.iter().map(|c| dot(c, c)).sum();
    let y_total: f64 = y0.iter().map(|c| dot(c, c)).sum();
    let factors = factors.min(n - 1).min(x.len());
    let mut fit = PlsFit {
        algorithm,
        x_mean,
        x_scale,
        y_mean,
        y_scale,
        weights: Vec::new(),
        rotations: Vec::new(),
        x_loadings: Vec::new(),
        y_loadings: Vec::new(),
        scores: Vec::new(),
        x_explained: Vec::new(),
        y_explained: Vec::new(),
    };
    // Factors explaining less than this share of X are numerical noise.
    let tiny = 1e-12 * x_total.max(f64::MIN_POSITIVE);
    match algorithm {
        PlsAlgorithm::Nipals => {
            let (mut xd, mut yd) = (x0, y0);
            for _ in 0..factors {
                let Some(w) = nipals_weight(&xd, &yd, n) else { break };
                let t = mul(&xd, &w, n);
                let tt = dot(&t, &t);
                if tt <= tiny {
                    break;
                }
                let p: Vec<f64> = t_mul(&xd, &t).iter().map(|v| v / tt).collect();
                let q: Vec<f64> = t_mul(&yd, &t).iter().map(|v| v / tt).collect();
                for (c, &pj) in xd.iter_mut().zip(&p) {
                    for (v, ti) in c.iter_mut().zip(&t) {
                        *v -= ti * pj;
                    }
                }
                for (c, &qk) in yd.iter_mut().zip(&q) {
                    for (v, ti) in c.iter_mut().zip(&t) {
                        *v -= ti * qk;
                    }
                }
                // r_a = w_a − Σ r_i (p_iᵀ w_a).
                let mut r = w.clone();
                for (ri, pi) in fit.rotations.iter().zip(&fit.x_loadings) {
                    let c = dot(pi, &w);
                    for (rv, v) in r.iter_mut().zip(ri) {
                        *rv -= c * v;
                    }
                }
                push_factor(&mut fit, w, r, p, q, t, tt, x_total, y_total);
            }
        }
        PlsAlgorithm::Simpls => {
            // S = X₀ᵀ Y₀, as p rows of m.
            let mut s: Vec<Vec<f64>> = (0..x0.len()).map(|j| y0.iter().map(|c| dot(&x0[j], c)).collect()).collect();
            let mut basis: Vec<Vec<f64>> = Vec::new();
            for _ in 0..factors {
                let Some(mut r) = dominant_left(&s) else { break };
                let mut t = mul(&x0, &r, n);
                // Keep the scores orthogonal to rounding: remove what t
                // shares with earlier scores (r adjusted so t = X₀ r).
                for (ti, ri) in fit.scores.iter().zip(&fit.rotations) {
                    let c = dot(ti, &t) / dot(ti, ti);
                    for (a, b) in t.iter_mut().zip(ti) {
                        *a -= c * b;
                    }
                    for (a, b) in r.iter_mut().zip(ri) {
                        *a -= c * b;
                    }
                }
                let tt = dot(&t, &t);
                if tt <= tiny {
                    break;
                }
                let p: Vec<f64> = t_mul(&x0, &t).iter().map(|v| v / tt).collect();
                let q: Vec<f64> = t_mul(&y0, &t).iter().map(|v| v / tt).collect();
                // v: p made orthogonal to earlier loadings (twice, for accuracy).
                let mut v = p.clone();
                for _ in 0..2 {
                    for b in &basis {
                        let c = dot(b, &v);
                        for (vi, bi) in v.iter_mut().zip(b) {
                            *vi -= c * bi;
                        }
                    }
                }
                let nv = norm(&v);
                if nv == 0.0 {
                    break;
                }
                v.iter_mut().for_each(|vi| *vi /= nv);
                // S ← S − v (vᵀ S).
                let m = s.first().map_or(0, Vec::len);
                let vs: Vec<f64> = (0..m).map(|k| s.iter().zip(&v).map(|(row, vi)| vi * row[k]).sum()).collect();
                for (row, vi) in s.iter_mut().zip(&v) {
                    for (e, c) in row.iter_mut().zip(&vs) {
                        *e -= vi * c;
                    }
                }
                basis.push(v);
                push_factor(&mut fit, r.clone(), r, p, q, t, tt, x_total, y_total);
            }
        }
    }
    if fit.scores.is_empty() {
        return Err(invalid("the predictors have no variation to model"));
    }
    Ok(fit)
}

#[allow(clippy::too_many_arguments)]
fn push_factor(
    fit: &mut PlsFit,
    w: Vec<f64>,
    r: Vec<f64>,
    p: Vec<f64>,
    q: Vec<f64>,
    t: Vec<f64>,
    tt: f64,
    x_total: f64,
    y_total: f64,
) {
    fit.x_explained.push(if x_total > 0.0 { tt * dot(&p, &p) / x_total } else { 0.0 });
    fit.y_explained.push(if y_total > 0.0 { tt * dot(&q, &q) / y_total } else { 0.0 });
    fit.weights.push(w);
    fit.rotations.push(r);
    fit.x_loadings.push(p);
    fit.y_loadings.push(q);
    fit.scores.push(t);
}

/// NIPALS weight vector (unit length) for deflated `x` and `y`, or `None`
/// when `Xᵀ Y` has vanished.
fn nipals_weight(x: &[Vec<f64>], y: &[Vec<f64>], n: usize) -> Option<Vec<f64>> {
    let unit = |mut w: Vec<f64>| {
        let nw = norm(&w);
        if nw > 0.0 && nw.is_finite() {
            w.iter_mut().for_each(|v| *v /= nw);
            Some(w)
        } else {
            None
        }
    };
    // Start from the response with the largest sum of squares.
    let start = (0..y.len()).max_by(|&a, &b| dot(&y[a], &y[a]).total_cmp(&dot(&y[b], &y[b])))?;
    let mut u = y[start].clone();
    let mut w = unit(t_mul(x, &u))?;
    if y.len() == 1 {
        return Some(w);
    }
    let mut t = mul(x, &w, n);
    for _ in 0..500 {
        let tt = dot(&t, &t);
        if tt == 0.0 {
            return None;
        }
        let q: Vec<f64> = t_mul(y, &t).iter().map(|v| v / tt).collect();
        let qq = dot(&q, &q);
        if qq == 0.0 {
            return Some(w);
        }
        u = mul(y, &q, n).iter().map(|v| v / qq).collect();
        w = unit(t_mul(x, &u))?;
        let t_new = mul(x, &w, n);
        let change = t_new.iter().zip(&t).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt();
        let size = norm(&t_new);
        t = t_new;
        if change <= 1e-13 * size.max(f64::MIN_POSITIVE) {
            break;
        }
    }
    Some(w)
}

/// Dominant left singular vector (unit length) of `s` (rows of equal
/// length), by power iteration on `S Sᵀ` through the small `Sᵀ S`; `None`
/// when `s` is zero.
fn dominant_left(s: &[Vec<f64>]) -> Option<Vec<f64>> {
    let m = s.first().map_or(0, Vec::len);
    if m == 0 {
        return None;
    }
    let column = |k: usize| -> Vec<f64> { s.iter().map(|row| row[k]).collect() };
    let r = if m == 1 {
        column(0)
    } else {
        // Dominant eigenvector c of SᵀS (m × m); r = S c.
        let cols: Vec<Vec<f64>> = (0..m).map(column).collect();
        let sts: Vec<Vec<f64>> = (0..m).map(|a| (0..m).map(|b| dot(&cols[a], &cols[b])).collect()).collect();
        let start = (0..m).max_by(|&a, &b| sts[a][a].total_cmp(&sts[b][b]))?;
        let mut c = vec![0.0; m];
        c[start] = 1.0;
        for _ in 0..1000 {
            let next: Vec<f64> = (0..m).map(|a| dot(&sts[a], &c)).collect();
            let nn = norm(&next);
            if nn == 0.0 {
                return None;
            }
            let next: Vec<f64> = next.iter().map(|v| v / nn).collect();
            let change = next.iter().zip(&c).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
            c = next;
            if change < 1e-14 {
                break;
            }
        }
        (0..s.len()).map(|j| dot(&s[j], &c)).collect()
    };
    let nr = norm(&r);
    (nr > 0.0 && nr.is_finite()).then(|| r.iter().map(|v| v / nr).collect())
}

impl PlsFit {
    /// Number of factors extracted.
    #[must_use]
    pub fn factors(&self) -> usize {
        self.scores.len()
    }

    /// Coefficients in the original units with the first `a` factors:
    /// `(intercepts, slopes)` where `slopes[j][k]` multiplies predictor `j`
    /// in the prediction of response `k`.
    #[must_use]
    pub fn coefficients(&self, a: usize) -> (Vec<f64>, Vec<Vec<f64>>) {
        let a = a.min(self.factors());
        let p = self.x_mean.len();
        let m = self.y_mean.len();
        let mut slopes = vec![vec![0.0; m]; p];
        for f in 0..a {
            for (j, row) in slopes.iter_mut().enumerate() {
                for (k, b) in row.iter_mut().enumerate() {
                    *b += self.rotations[f][j] * self.y_loadings[f][k];
                }
            }
        }
        for (j, row) in slopes.iter_mut().enumerate() {
            for (k, b) in row.iter_mut().enumerate() {
                *b *= self.y_scale[k] / self.x_scale[j];
            }
        }
        let intercepts =
            (0..m).map(|k| self.y_mean[k] - (0..p).map(|j| slopes[j][k] * self.x_mean[j]).sum::<f64>()).collect();
        (intercepts, slopes)
    }

    /// Predicted responses for one row of predictor values `x` (original
    /// units) with the first `a` factors.
    #[must_use]
    pub fn predict(&self, a: usize, x: &[f64]) -> Vec<f64> {
        let (b0, b) = self.coefficients(a);
        (0..b0.len()).map(|k| b0[k] + x.iter().zip(&b).map(|(xj, row)| xj * row[k]).sum::<f64>()).collect()
    }

    /// Scores of one row of predictor values (original units) on the first
    /// `a` factors.
    #[must_use]
    pub fn transform(&self, a: usize, x: &[f64]) -> Vec<f64> {
        let z: Vec<f64> = x.iter().zip(&self.x_mean).zip(&self.x_scale).map(|((v, m), s)| (v - m) / s).collect();
        self.rotations.iter().take(a).map(|r| dot(r, &z)).collect()
    }

    /// Variable importance in projection of each predictor with the first
    /// `a` factors.
    #[must_use]
    pub fn vip(&self, a: usize) -> Vec<f64> {
        let a = a.min(self.factors());
        let p = self.x_mean.len();
        let ssy: Vec<f64> = (0..a)
            .map(|f| dot(&self.scores[f], &self.scores[f]) * dot(&self.y_loadings[f], &self.y_loadings[f]))
            .collect();
        let total: f64 = ssy.iter().sum();
        (0..p)
            .map(|j| {
                if total <= 0.0 {
                    return 0.0;
                }
                let s: f64 = (0..a)
                    .map(|f| {
                        let w = &self.weights[f];
                        ssy[f] * (w[j] / norm(w)).powi(2)
                    })
                    .sum();
                (p as f64 * s / total).sqrt()
            })
            .collect()
    }
}

/// Squared prediction errors (summed over responses, original units) of
/// each row for 0 to `max_factors` factors by k-fold cross-validation:
/// `errors[a][i]` for row `i` with `a` factors, so `Σᵢ errors[a][i]` is the
/// predicted residual sum of squares (PRESS). `fold_of[i]` is row `i`'s
/// fold. With 0 factors each response is predicted by its training mean. A
/// fold whose model extracts fewer factors repeats its last prediction for
/// larger numbers.
///
/// # Errors
/// As [`fit_pls`], for any fold.
pub fn cross_validate_pls(
    x: &[Vec<f64>],
    y: &[Vec<f64>],
    fold_of: &[usize],
    max_factors: usize,
    scale_x: bool,
    scale_y: bool,
    algorithm: PlsAlgorithm,
) -> Result<Vec<Vec<f64>>> {
    let n = fold_of.len();
    let folds = fold_of.iter().copied().max().map_or(0, |m| m + 1);
    let mut errors = vec![vec![0.0; n]; max_factors + 1];
    for f in 0..folds {
        let fit_rows: Vec<usize> = (0..n).filter(|&i| fold_of[i] != f).collect();
        let held: Vec<usize> = (0..n).filter(|&i| fold_of[i] == f).collect();
        if held.is_empty() || fit_rows.len() < 2 {
            continue;
        }
        let pick = |cols: &[Vec<f64>], rows: &[usize]| -> Vec<Vec<f64>> {
            cols.iter().map(|c| rows.iter().map(|&i| c[i]).collect()).collect()
        };
        let model = fit_pls(&pick(x, &fit_rows), &pick(y, &fit_rows), max_factors.max(1), scale_x, scale_y, algorithm)?;
        for &i in &held {
            let row: Vec<f64> = x.iter().map(|c| c[i]).collect();
            for (a, e) in errors.iter_mut().enumerate() {
                let pred = model.predict(a, &row);
                e[i] = y.iter().zip(&pred).map(|(c, p)| (c[i] - p).powi(2)).sum::<f64>();
            }
        }
    }
    Ok(errors)
}

/// The fewest factors (at least 1) whose mean held-out squared error is
/// within one standard error of the smallest (the one-standard-error rule;
/// Hastie, Tibshirani & Friedman, *The Elements of Statistical Learning*,
/// 2009, §7.10), from per-row errors `errors[a][i]` as
/// [`cross_validate_pls`] returns them.
#[must_use]
pub fn choose_factors(errors: &[Vec<f64>]) -> usize {
    let stats: Vec<(f64, f64)> = errors
        .iter()
        .map(|e| {
            let n = e.len().max(1) as f64;
            let m = e.iter().sum::<f64>() / n;
            let var = e.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
            (m, (var / n).sqrt())
        })
        .collect();
    let Some(best) = (1..stats.len()).min_by(|&a, &b| stats[a].0.total_cmp(&stats[b].0)) else { return 1 };
    let limit = stats[best].0 + stats[best].1;
    (1..=best).find(|&a| stats[a].0 <= limit).unwrap_or(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::rng::Rng;

    /// 40 rows, 6 correlated predictors driven by 2 latent variables, and
    /// two responses.
    fn data() -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let mut rng = Rng::new(21);
        let n = 40;
        let z: Vec<[f64; 2]> = (0..n).map(|_| [rng.normal(), rng.normal()]).collect();
        let load = [[1.0, 0.2], [0.8, -0.5], [0.3, 1.0], [-0.6, 0.7], [0.5, 0.5], [0.1, -0.9]];
        let x: Vec<Vec<f64>> = load
            .iter()
            .enumerate()
            .map(|(j, l)| {
                z.iter().map(|zi| 10.0 * j as f64 + l[0] * zi[0] + l[1] * zi[1] + 0.1 * rng.normal()).collect()
            })
            .collect();
        let y1: Vec<f64> = z.iter().map(|zi| 3.0 + 2.0 * zi[0] - zi[1] + 0.2 * rng.normal()).collect();
        let y2: Vec<f64> = z.iter().map(|zi| -1.0 + 0.5 * zi[1] + 0.2 * rng.normal()).collect();
        (x, vec![y1, y2])
    }

    /// Ordinary least squares intercept and slopes of `y` on the columns of
    /// `x`, by pivoted Householder QR.
    fn ols(x: &[Vec<f64>], y: &[f64]) -> (f64, Vec<f64>) {
        let mut cols = vec![vec![1.0; y.len()]];
        cols.extend(x.iter().cloned());
        let qr = stat_core::qr::QrDecomposition::new(stat_core::matrix::Matrix::from_columns(&cols).unwrap()).unwrap();
        let b = qr.solve_least_squares(y).unwrap();
        (b[0], b[1..].to_vec())
    }

    #[test]
    fn all_factors_reproduce_least_squares() {
        let (x, y) = data();
        let (b0, b) = ols(&x, &y[0]);
        for algorithm in [PlsAlgorithm::Nipals, PlsAlgorithm::Simpls] {
            for scale in [false, true] {
                let fit = fit_pls(&x, &y[..1], 6, scale, false, algorithm).unwrap();
                assert_eq!(fit.factors(), 6);
                let (c0, c) = fit.coefficients(6);
                // (The predictors are nearly collinear, so both routes lose
                // a few digits.)
                assert!((c0[0] - b0).abs() < 1e-9 * b0.abs().max(1.0), "{algorithm:?} {} {b0}", c0[0]);
                for (cj, bj) in c.iter().zip(&b) {
                    assert!((cj[0] - bj).abs() < 1e-9 * bj.abs().max(1.0), "{algorithm:?} {} {bj}", cj[0]);
                }
                // Explained shares add up to 1 for X with every factor.
                let total: f64 = fit.x_explained.iter().sum();
                assert!((total - 1.0).abs() < 1e-10, "{total}");
            }
        }
    }

    #[test]
    fn one_factor_matches_closed_form_and_algorithms_agree() {
        let (x, y) = data();
        let (x0, _, _) = standardize(&x, true);
        let (y0, _, _) = standardize(&y[..1], false);
        // One factor: w ∝ X₀ᵀy₀, t = X₀w, b = w (tᵀy₀ / tᵀt).
        let n = y[0].len();
        let mut w = t_mul(&x0, &y0[0]);
        let nw = norm(&w);
        w.iter_mut().for_each(|v| *v /= nw);
        let t = mul(&x0, &w, n);
        let c = dot(&t, &y0[0]) / dot(&t, &t);
        let nip = fit_pls(&x, &y[..1], 3, true, false, PlsAlgorithm::Nipals).unwrap();
        let sim = fit_pls(&x, &y[..1], 3, true, false, PlsAlgorithm::Simpls).unwrap();
        for j in 0..6 {
            assert!((nip.rotations[0][j] * nip.y_loadings[0][0] - w[j] * c).abs() < 1e-12);
        }
        // Single response: the same predictions for every number of factors.
        for a in 1..=3 {
            let (n0, nb) = nip.coefficients(a);
            let (s0, sb) = sim.coefficients(a);
            assert!((n0[0] - s0[0]).abs() < 1e-9);
            for j in 0..6 {
                assert!((nb[j][0] - sb[j][0]).abs() < 1e-9, "a = {a}");
            }
        }
        // Scores are orthogonal and equal X₀ r.
        for a in 0..3 {
            for b in 0..a {
                assert!(dot(&nip.scores[a], &nip.scores[b]).abs() < 1e-9);
                assert!(dot(&sim.scores[a], &sim.scores[b]).abs() < 1e-9);
            }
            let row: Vec<f64> = x.iter().map(|c| c[5]).collect();
            assert!((nip.transform(3, &row)[a] - nip.scores[a][5]).abs() < 1e-10);
        }
    }

    #[test]
    fn two_responses_vip_and_cross_validation() {
        let (x, y) = data();
        for algorithm in [PlsAlgorithm::Nipals, PlsAlgorithm::Simpls] {
            let fit = fit_pls(&x, &y, 4, true, true, algorithm).unwrap();
            // Two latent variables explain nearly all of X and most of Y (the
            // second response is noisier).
            let x2: f64 = fit.x_explained[..2].iter().sum();
            let y2: f64 = fit.y_explained[..2].iter().sum();
            assert!(x2 > 0.97 && y2 > 0.9, "{algorithm:?} {x2} {y2}");
            // VIP: mean square 1 over the predictors.
            let vip = fit.vip(2);
            let ms = vip.iter().map(|v| v * v).sum::<f64>() / 6.0;
            assert!((ms - 1.0).abs() < 1e-10, "{ms}");
            let pred = fit.predict(2, &x.iter().map(|c| c[0]).collect::<Vec<_>>());
            assert!((pred[0] - y[0][0]).abs() < 1.0 && (pred[1] - y[1][0]).abs() < 1.0, "{pred:?}");
        }
        // PRESS falls sharply to two factors and not much after.
        let fold_of: Vec<usize> = (0..40).map(|i| i % 5).collect();
        let errors = cross_validate_pls(&x, &y[..1], &fold_of, 5, true, false, PlsAlgorithm::Simpls).unwrap();
        assert_eq!(errors.len(), 6);
        let press: Vec<f64> = errors.iter().map(|e| e.iter().sum()).collect();
        assert_eq!(choose_factors(&errors), 2);
        assert!(press[2] < 0.1 * press[0], "{press:?}");
        assert!(press[2] < press[1]);
        // Errors.
        assert!(fit_pls(&x, &y, 0, true, false, PlsAlgorithm::Nipals).is_err());
        assert!(fit_pls(&[vec![1.0]], &[vec![2.0]], 1, true, false, PlsAlgorithm::Nipals).is_err());
        let constant = vec![vec![1.0; 10]];
        assert!(fit_pls(&constant, &[vec![0.5; 10]], 1, true, false, PlsAlgorithm::Nipals).is_err());
    }
}
