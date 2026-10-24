//! Locally weighted regression (LOESS; Cleveland, "Robust Locally Weighted
//! Regression and Smoothing Scatterplots", *JASA* 74(368), 1979; Cleveland &
//! Devlin, *JASA* 83(403), 1988).
//!
//! At each x₀ a polynomial of degree 1 or 2 is fitted by weighted least
//! squares to the nearest `⌈α·n⌉` observations, with tricube weights
//! `(1 − (d/h)³)³` (`h` the distance to the farthest of them) times each
//! observation's own weight. The fit is linear in y, `ŷ(x₀) = Σ lᵢ(x₀) yᵢ`, so
//! `Var ŷ(x₀) = σ² Σ lᵢ²/wᵢ`; `σ̂² = Σ wᵢ rᵢ² / (n − ν)` with `ν = tr L`, the
//! equivalent number of parameters, and the band uses `t` with `n − ν`
//! degrees of freedom (Cleveland & Grosse's lookup degrees approximated by
//! `n − ν`).

use stat_core::{ContinuousDistribution, StudentsT};

use crate::stats::SmoothFit;

/// Distance from `x0` to its `q`-th nearest observation, counting each
/// point `freq` times (`freq` empty: once each), widened in proportion when
/// `q` exceeds the number of observations (a span over 1; Cleveland &
/// Grosse).
fn window(dist: &[f64], freq: &[f64], q: usize) -> f64 {
    let n = dist.len();
    if freq.is_empty() {
        let mut sorted = dist.to_vec();
        let k = q.clamp(1, n) - 1;
        sorted.select_nth_unstable_by(k, f64::total_cmp);
        let h = sorted[k];
        return if q > n { h * q as f64 / n as f64 } else { h };
    }
    let total: f64 = freq.iter().sum();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| dist[a].total_cmp(&dist[b]));
    let target = q as f64;
    let mut cumulative = 0.0;
    for &i in &order {
        cumulative += freq[i];
        if cumulative >= target * (1.0 - 1e-12) {
            return dist[i];
        }
    }
    dist[order[n - 1]] * target / total
}

/// Fitted value at `x0` and its weight vector `l` (over `pts`); `None` if the
/// local system is singular even at degree 0. `freq` holds each point's
/// frequency (empty: 1 each), which counts towards the `q` nearest; the
/// points' third component is their frequency times weight.
fn local_fit(
    pts: &[(f64, f64, f64)],
    freq: &[f64],
    x0: f64,
    q: usize,
    degree: usize,
    scale: f64,
) -> Option<(f64, Vec<f64>)> {
    let mut dist: Vec<f64> = pts.iter().map(|p| (p.0 - x0).abs()).collect();
    let mut h = window(&dist, freq, q);
    if h <= 0.0 {
        h = f64::MIN_POSITIVE;
    }
    // Tricube weights times prior weights; a hair over h keeps the farthest point.
    let a: Vec<f64> = dist
        .iter_mut()
        .zip(pts)
        .map(|(d, p)| {
            let u = (*d / (h * (1.0 + 1e-10))).min(1.0);
            let k = (1.0 - u * u * u).powi(3);
            k * p.2
        })
        .collect();
    for deg in (0..=degree).rev() {
        let p = deg + 1;
        let basis = |x: f64| -> Vec<f64> { (0..p).map(|j| ((x - x0) / scale).powi(j as i32)).collect() };
        // Normal equations M c = e₁, M = Xᵀ A X.
        let mut m = vec![vec![0.0; p]; p];
        for (pt, &ai) in pts.iter().zip(&a) {
            if ai <= 0.0 {
                continue;
            }
            let b = basis(pt.0);
            for r in 0..p {
                for c in 0..p {
                    m[r][c] += ai * b[r] * b[c];
                }
            }
        }
        let mut e1 = vec![0.0; p];
        e1[0] = 1.0;
        let Some(c) = solve_small(m, e1) else { continue };
        let l: Vec<f64> = pts
            .iter()
            .zip(&a)
            .map(|(pt, &ai)| ai * basis(pt.0).iter().zip(&c).map(|(b, ci)| b * ci).sum::<f64>())
            .collect();
        let fit: f64 = l.iter().zip(pts).map(|(li, pt)| li * pt.1).sum();
        if fit.is_finite() {
            return Some((fit, l));
        }
    }
    None
}

/// Solve a small dense system by Gaussian elimination with partial pivoting;
/// `None` if it is (numerically) singular.
fn solve_small(mut m: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let p = b.len();
    let scale = m.iter().flatten().fold(0.0_f64, |s, v| s.max(v.abs()));
    if scale <= 0.0 || !scale.is_finite() {
        return None;
    }
    for col in 0..p {
        let pivot = (col..p).max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))?;
        if m[pivot][col].abs() <= 1e-12 * scale {
            return None;
        }
        m.swap(col, pivot);
        b.swap(col, pivot);
        for r in col + 1..p {
            let f = m[r][col] / m[col][col];
            for c in col..p {
                m[r][c] -= f * m[col][c];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = vec![0.0; p];
    for r in (0..p).rev() {
        let s: f64 = (r + 1..p).map(|c| m[r][c] * x[c]).sum();
        x[r] = (b[r] - s) / m[r][r];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Most observations at which the equivalent number of parameters is
/// evaluated exactly; beyond this it is estimated from an even subsample.
const TRACE_POINTS: usize = 1500;

/// LOESS curve of `y` on `x` at `points` x across the data, with smoothing
/// span `span` (fraction of the observations in each local fit, at least
/// enough for the local polynomial), local `degree` 1 or 2, observation
/// weights `weight` (empty: 1 each), and a `level` confidence band when
/// there are residual degrees of freedom. `None` with fewer than two
/// distinct x. See [`loess_with_frequencies`] for frequency counts.
#[must_use]
pub fn loess(
    x: &[f64],
    y: &[f64],
    weight: &[f64],
    span: f64,
    degree: usize,
    level: f64,
    points: usize,
) -> Option<SmoothFit> {
    loess_with_frequencies(x, y, &[], weight, span, degree, level, points)
}

/// [`loess`] where row `i` also stands for `freq[i]` identical observations
/// (empty: 1 each; rows whose frequency is not positive are left out), so a
/// row of frequency 2 gives the same curve and band as the row listed twice:
/// the span counts observations, `σ̂² = Σ fᵢwᵢrᵢ² / (Σfᵢ − ν)` and the band
/// has `Σfᵢ − ν` degrees of freedom.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn loess_with_frequencies(
    x: &[f64],
    y: &[f64],
    freq: &[f64],
    weight: &[f64],
    span: f64,
    degree: usize,
    level: f64,
    points: usize,
) -> Option<SmoothFit> {
    let wt = |i: usize| weight.get(i).copied().unwrap_or(1.0);
    let fr = |i: usize| freq.get(i).copied().unwrap_or(1.0);
    // (x, y, f·w) and f.
    let mut rows: Vec<((f64, f64, f64), f64)> = x
        .iter()
        .zip(y)
        .enumerate()
        .filter(|&(i, (a, b))| {
            a.is_finite() && b.is_finite() && wt(i).is_finite() && wt(i) > 0.0 && fr(i).is_finite() && fr(i) > 0.0
        })
        .map(|(i, (&a, &b))| ((a, b, fr(i) * wt(i)), fr(i)))
        .collect();
    let n = rows.len();
    if n < 2 {
        return None;
    }
    rows.sort_by(|a, b| a.0.0.total_cmp(&b.0.0));
    let total_f: f64 = rows.iter().map(|r| r.1).sum();
    let mean_w = rows.iter().map(|r| r.0.2).sum::<f64>() / total_f;
    let pts: Vec<(f64, f64, f64)> = rows.iter().map(|r| (r.0.0, r.0.1, r.0.2 / mean_w)).collect();
    let counts: Vec<f64> = if freq.is_empty() { Vec::new() } else { rows.iter().map(|r| r.1).collect() };
    let (lo, hi) = (pts[0].0, pts[n - 1].0);
    if hi <= lo {
        return None;
    }
    let degree = degree.clamp(1, 2);
    let span = if span.is_finite() { span.clamp(0.05, 4.0) } else { 0.75 };
    let q = ((span * total_f).ceil() as usize).max(degree + 2);
    let scale = hi - lo;
    // Residuals and the trace of L, at the observations (or a subsample).
    let stride = n.div_ceil(TRACE_POINTS).max(1);
    let (mut rss, mut trace, mut used) = (0.0, 0.0, 0usize);
    for i in (0..n).step_by(stride) {
        let (fit, l) = local_fit(&pts, &counts, pts[i].0, q, degree, scale)?;
        rss += pts[i].2 * (pts[i].1 - fit).powi(2);
        trace += l[i];
        used += 1;
    }
    let factor = n as f64 / used as f64;
    let (rss, trace) = (rss * factor, trace * factor);
    let df = total_f - trace;
    let points = points.max(2);
    let mut curve = Vec::with_capacity(points);
    let mut se = Vec::with_capacity(points);
    for k in 0..points {
        let x0 = lo + scale * k as f64 / (points - 1) as f64;
        let (fit, l) = local_fit(&pts, &counts, x0, q, degree, scale)?;
        curve.push([x0, fit]);
        se.push(l.iter().zip(&pts).map(|(li, p)| li * li / p.2).sum::<f64>());
    }
    let band = if df > 0.5 && rss.is_finite() {
        let sigma2 = rss / df;
        let t = StudentsT::new(df).ok().and_then(|d| d.quantile(0.5 + 0.5 * level.clamp(0.5, 0.999)).ok());
        t.and_then(|t| {
            let lower: Vec<[f64; 2]> =
                curve.iter().zip(&se).map(|(c, s)| [c[0], c[1] - t * (sigma2 * s).max(0.0).sqrt()]).collect();
            let upper: Vec<[f64; 2]> =
                curve.iter().zip(&se).map(|(c, s)| [c[0], c[1] + t * (sigma2 * s).max(0.0).sqrt()]).collect();
            lower.iter().chain(&upper).all(|p| p[1].is_finite()).then_some((lower, upper))
        })
    } else {
        None
    };
    curve.iter().all(|p| p[1].is_finite()).then_some(SmoothFit { curve, band, equivalent_df: trace })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loess_reproduces_low_degree_polynomials() {
        let x: Vec<f64> = (0..40).map(|i| f64::from(i) * 0.25).collect();
        // Local linear fits reproduce a line exactly; local quadratics a parabola.
        let line: Vec<f64> = x.iter().map(|v| 2.0 - 0.5 * v).collect();
        let f = loess(&x, &line, &[], 0.3, 1, 0.95, 30).unwrap();
        assert!(f.curve.iter().all(|p| (p[1] - (2.0 - 0.5 * p[0])).abs() < 1e-9));
        let parabola: Vec<f64> = x.iter().map(|v| v * v - 3.0 * v).collect();
        let g = loess(&x, &parabola, &[], 0.3, 2, 0.95, 30).unwrap();
        assert!(g.curve.iter().all(|p| (p[1] - (p[0] * p[0] - 3.0 * p[0])).abs() < 1e-8));
        // An exact fit has (numerically) zero residual variance: a band of zero width.
        let (lo, hi) = g.band.unwrap();
        assert!(lo.iter().zip(&hi).all(|(a, b)| (b[1] - a[1]).abs() < 1e-6));
    }

    #[test]
    fn loess_band_and_span() {
        let x: Vec<f64> = (0..60).map(f64::from).collect();
        let y: Vec<f64> =
            x.iter().enumerate().map(|(i, v)| (v / 8.0).sin() + if i % 3 == 0 { 0.3 } else { -0.15 }).collect();
        let narrow = loess(&x, &y, &[], 0.2, 1, 0.95, 50).unwrap();
        let wide = loess(&x, &y, &[], 0.9, 1, 0.95, 50).unwrap();
        assert!(narrow.equivalent_df > wide.equivalent_df && wide.equivalent_df > 1.5);
        let (lo, hi) = narrow.band.as_ref().unwrap();
        assert!(narrow.curve.iter().zip(lo.iter().zip(hi)).all(|(c, (l, h))| l[1] < c[1] && c[1] < h[1]));
        // Weights: a zero-weight point is ignored.
        let mut w = vec![1.0; 60];
        w[10] = 0.0;
        let mut y2 = y.clone();
        y2[10] = 100.0;
        let a = loess(&x, &y2, &w, 0.3, 1, 0.95, 20).unwrap();
        let mut x3 = x.clone();
        let mut y3 = y.clone();
        x3.remove(10);
        y3.remove(10);
        let b = loess(&x3, &y3, &[], 0.3 * 60.0 / 59.0, 1, 0.95, 20).unwrap();
        assert!(a.curve.iter().zip(&b.curve).all(|(p, q)| (p[1] - q[1]).abs() < 1e-6));
    }

    #[test]
    fn loess_degenerate_inputs() {
        assert!(loess(&[], &[], &[], 0.5, 1, 0.95, 10).is_none());
        assert!(loess(&[1.0], &[2.0], &[], 0.5, 1, 0.95, 10).is_none());
        assert!(loess(&[1.0; 5], &[1.0, 2.0, 3.0, 4.0, 5.0], &[], 0.5, 1, 0.95, 10).is_none());
        // Two distinct x with replicates: a line through the means, finite.
        let f = loess(&[0.0, 0.0, 1.0, 1.0], &[1.0, 3.0, 2.0, 4.0], &[], 0.5, 2, 0.95, 5).unwrap();
        assert!(f.curve.iter().all(|p| p[1].is_finite()));
        assert!((f.curve[0][1] - 2.0).abs() < 1e-9 && (f.curve[4][1] - 3.0).abs() < 1e-9);
        // Constant y.
        let c = loess(&[1.0, 2.0, 3.0, 4.0], &[5.0; 4], &[], 1.0, 1, 0.95, 5).unwrap();
        assert!(c.curve.iter().all(|p| (p[1] - 5.0).abs() < 1e-12));
    }
}
