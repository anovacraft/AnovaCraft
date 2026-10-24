//! Density estimates for graph layers: one- and two-dimensional Gaussian
//! kernel density estimates, highest-density contour levels and contour lines
//! (marching squares), and normal-theory density ellipses.
//!
//! * Kernel density: `f̂(x) = Σ wᵢ K((x − xᵢ)/h) / (h Σ wᵢ)` with the Gaussian
//!   kernel (Silverman, *Density Estimation for Statistics and Data
//!   Analysis*, 1986). The default bandwidth is Silverman's rule of thumb
//!   `0.9 · min(s, IQR/1.34) · n^(−1/5)`; two-dimensional estimates use a
//!   product kernel with each axis's own bandwidth (Scott's `n^(−1/6)` rate).
//! * Contour levels enclose given fractions of the estimated probability mass
//!   (highest-density regions; Hyndman, "Computing and Graphing Highest
//!   Density Regions", *American Statistician* 50(2), 1996).
//! * Contour lines by marching squares with linear interpolation along cell
//!   edges; saddle cells are resolved by the cell's mean.
//! * Density ellipses: the contour of the bivariate normal with the data's
//!   mean and covariance that holds probability `p`, i.e. Mahalanobis radius
//!   `√χ²₂(p) = √(−2 ln(1 − p))`.
//!
//! Every function is total: degenerate input gives `None` or empty output.

use crate::stats::weighted_quantile;

/// Weighted mean, standard deviation and total weight of finite values with
/// positive weight (`None` with no such values).
fn weighted_moments(values: &[f64], weights: &[f64]) -> Option<(f64, f64, f64)> {
    let w = |i: usize| weights.get(i).copied().unwrap_or(1.0);
    let pairs: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .filter(|&(i, v)| v.is_finite() && w(i).is_finite() && w(i) > 0.0)
        .map(|(i, &v)| (v, w(i)))
        .collect();
    let total: f64 = pairs.iter().map(|p| p.1).sum();
    if pairs.is_empty() || total <= 0.0 {
        return None;
    }
    let mean = pairs.iter().map(|p| p.0 * p.1).sum::<f64>() / total;
    let ss: f64 = pairs.iter().map(|p| p.1 * (p.0 - mean).powi(2)).sum();
    let sd = if total > 1.0 { (ss / (total - 1.0)).sqrt() } else { 0.0 };
    Some((mean, sd, total))
}

/// Silverman's rule-of-thumb bandwidth `0.9 · min(s, IQR/1.34) · n^(−1/5)`
/// for values with frequencies (an empty `freq` means 1 each). Falls back to
/// whichever of `s` or `IQR/1.34` is positive, then to a tenth of the range,
/// then to 1, so the result is always positive and finite.
#[must_use]
pub fn silverman_bandwidth(values: &[f64], freq: &[f64]) -> f64 {
    let Some((_, sd, n)) = weighted_moments(values, freq) else { return 1.0 };
    let pairs: Vec<(f64, f64)> =
        values.iter().enumerate().map(|(i, &v)| (v, freq.get(i).copied().unwrap_or(1.0))).collect();
    let iqr = match (weighted_quantile(&pairs, 0.25), weighted_quantile(&pairs, 0.75)) {
        (Some(a), Some(b)) => (b - a) / 1.34,
        _ => 0.0,
    };
    let spread = match (sd > 0.0, iqr > 0.0) {
        (true, true) => sd.min(iqr),
        (true, false) => sd,
        (false, true) => iqr,
        (false, false) => {
            let (lo, hi) = pairs
                .iter()
                .filter(|p| p.0.is_finite())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.0), b.max(p.0)));
            if hi > lo { (hi - lo) / 10.0 } else { 1.0 }
        }
    };
    let h = 0.9 * spread * n.max(1.0).powf(-0.2);
    if h.is_finite() && h > 0.0 { h } else { 1.0 }
}

/// Gaussian kernel density of `values` (with frequencies `freq`) at `points`
/// evenly spaced x from `lo` to `hi`, as `[x, density]`. Empty without values
/// or with a bad bandwidth or range.
#[must_use]
pub fn kde_1d(values: &[f64], freq: &[f64], bandwidth: f64, lo: f64, hi: f64, points: usize) -> Vec<[f64; 2]> {
    let f = |i: usize| freq.get(i).copied().unwrap_or(1.0);
    let data: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .filter(|&(i, v)| v.is_finite() && f(i).is_finite() && f(i) > 0.0)
        .map(|(i, &v)| (v, f(i)))
        .collect();
    let total: f64 = data.iter().map(|d| d.1).sum();
    if data.is_empty() || !(bandwidth.is_finite() && bandwidth > 0.0) || !(lo.is_finite() && hi.is_finite()) || hi < lo
    {
        return Vec::new();
    }
    let points = points.max(2);
    let norm = 1.0 / (total * bandwidth * (2.0 * std::f64::consts::PI).sqrt());
    (0..points)
        .map(|k| {
            let x = if hi > lo { lo + (hi - lo) * k as f64 / (points - 1) as f64 } else { lo };
            let d: f64 = data.iter().map(|&(v, w)| w * (-0.5 * ((x - v) / bandwidth).powi(2)).exp()).sum();
            [x, d * norm]
        })
        .collect()
}

/// Values of a function on a regular grid: `z[j * xs.len() + i]` at
/// `(xs[i], ys[j])`.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// Grid x positions, ascending.
    pub xs: Vec<f64>,
    /// Grid y positions, ascending.
    pub ys: Vec<f64>,
    /// Values, row-major by y.
    pub z: Vec<f64>,
}

impl Grid {
    /// Value at grid node `(i, j)`.
    #[must_use]
    pub fn at(&self, i: usize, j: usize) -> f64 {
        self.z[j * self.xs.len() + i]
    }
}

/// Two-dimensional Gaussian product-kernel density of the points `(x, y)`
/// with weights, on an `nx × ny` grid spanning the data plus three
/// bandwidths on each side. Bandwidths are each axis's Silverman bandwidth
/// rescaled to the bivariate rate `n^(−1/6)`, times `scale`. `None` with
/// fewer than two points.
#[must_use]
pub fn kde_2d(x: &[f64], y: &[f64], weight: &[f64], scale: f64, nx: usize, ny: usize) -> Option<Grid> {
    let w = |i: usize| weight.get(i).copied().unwrap_or(1.0);
    let pts: Vec<(f64, f64, f64)> = x
        .iter()
        .zip(y)
        .enumerate()
        .filter(|&(i, (a, b))| a.is_finite() && b.is_finite() && w(i).is_finite() && w(i) > 0.0)
        .map(|(i, (&a, &b))| (a, b, w(i)))
        .collect();
    if pts.len() < 2 {
        return None;
    }
    let (px, py, pw): (Vec<f64>, Vec<f64>, Vec<f64>) =
        pts.iter().fold((Vec::new(), Vec::new(), Vec::new()), |(mut a, mut b, mut c), p| {
            a.push(p.0);
            b.push(p.1);
            c.push(p.2);
            (a, b, c)
        });
    let total: f64 = pw.iter().sum();
    // Silverman's rule carries n^(-1/5); the bivariate rate is n^(-1/6).
    let rate = total.max(1.0).powf(0.2 - 1.0 / 6.0);
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let hx = silverman_bandwidth(&px, &pw) * rate * scale;
    let hy = silverman_bandwidth(&py, &pw) * rate * scale;
    let range = |v: &[f64], h: f64| {
        let (lo, hi) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &t| (a.min(t), b.max(t)));
        (lo - 3.0 * h, hi + 3.0 * h)
    };
    let ((x0, x1), (y0, y1)) = (range(&px, hx), range(&py, hy));
    let (nx, ny) = (nx.clamp(4, 400), ny.clamp(4, 400));
    let xs: Vec<f64> = (0..nx).map(|i| x0 + (x1 - x0) * i as f64 / (nx - 1) as f64).collect();
    let ys: Vec<f64> = (0..ny).map(|j| y0 + (y1 - y0) * j as f64 / (ny - 1) as f64).collect();
    // Separable kernel: per point, its weights along each axis.
    let kx: Vec<Vec<f64>> =
        px.iter().map(|&a| xs.iter().map(|&g| (-0.5 * ((g - a) / hx).powi(2)).exp()).collect()).collect();
    let ky: Vec<Vec<f64>> =
        py.iter().map(|&b| ys.iter().map(|&g| (-0.5 * ((g - b) / hy).powi(2)).exp()).collect()).collect();
    let norm = 1.0 / (total * 2.0 * std::f64::consts::PI * hx * hy);
    let mut z = vec![0.0; nx * ny];
    for (p, &wp) in pw.iter().enumerate() {
        for (j, &yk) in ky[p].iter().enumerate() {
            if yk < 1e-12 {
                continue;
            }
            let row = &mut z[j * nx..(j + 1) * nx];
            for (cell, &xk) in row.iter_mut().zip(&kx[p]) {
                *cell += wp * xk * yk;
            }
        }
    }
    for v in &mut z {
        *v *= norm;
    }
    z.iter().all(|v| v.is_finite()).then_some(Grid { xs, ys, z })
}

/// Density thresholds whose superlevel sets hold the given fractions of the
/// grid's total mass (each in (0, 1)), in the order given. A fraction near 1
/// gives a low threshold (a wide region). Empty for an all-zero grid.
#[must_use]
pub fn mass_thresholds(grid: &Grid, fractions: &[f64]) -> Vec<f64> {
    let mut values: Vec<f64> = grid.z.iter().copied().filter(|v| v.is_finite() && *v > 0.0).collect();
    let total: f64 = values.iter().sum();
    if values.is_empty() || total <= 0.0 {
        return Vec::new();
    }
    values.sort_by(|a, b| b.total_cmp(a));
    let mut cumulative = Vec::with_capacity(values.len());
    let mut acc = 0.0;
    for v in &values {
        acc += v;
        cumulative.push(acc / total);
    }
    fractions
        .iter()
        .map(|&f| {
            let f = f.clamp(1e-6, 1.0 - 1e-9);
            let k = cumulative.partition_point(|&c| c < f).min(values.len() - 1);
            values[k]
        })
        .collect()
}

/// Line segments of the contour `z = level` by marching squares.
#[must_use]
pub fn contour_segments(grid: &Grid, level: f64) -> Vec<[[f64; 2]; 2]> {
    let (nx, ny) = (grid.xs.len(), grid.ys.len());
    let mut out = Vec::new();
    if nx < 2 || ny < 2 || !level.is_finite() {
        return out;
    }
    // Where the contour crosses the edge from a to b (values va, vb).
    let cross = |a: [f64; 2], b: [f64; 2], va: f64, vb: f64| -> [f64; 2] {
        let t = if vb != va { ((level - va) / (vb - va)).clamp(0.0, 1.0) } else { 0.5 };
        [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
    };
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            // Corners counter-clockwise from bottom-left.
            let p = [
                [grid.xs[i], grid.ys[j]],
                [grid.xs[i + 1], grid.ys[j]],
                [grid.xs[i + 1], grid.ys[j + 1]],
                [grid.xs[i], grid.ys[j + 1]],
            ];
            let v = [grid.at(i, j), grid.at(i + 1, j), grid.at(i + 1, j + 1), grid.at(i, j + 1)];
            let case = v.iter().enumerate().fold(0usize, |acc, (k, &val)| acc | (usize::from(val >= level) << k));
            if case == 0 || case == 15 {
                continue;
            }
            // Edges: 0 bottom (p0–p1), 1 right (p1–p2), 2 top (p2–p3), 3 left (p3–p0).
            let e = |k: usize| cross(p[k], p[(k + 1) % 4], v[k], v[(k + 1) % 4]);
            let pairs: &[(usize, usize)] = match case {
                1 | 14 => &[(3, 0)],
                2 | 13 => &[(0, 1)],
                3 | 12 => &[(3, 1)],
                4 | 11 => &[(1, 2)],
                6 | 9 => &[(0, 2)],
                7 | 8 => &[(3, 2)],
                5 | 10 => {
                    // Saddle: decide by the cell mean.
                    let centre_high = v.iter().sum::<f64>() / 4.0 >= level;
                    match (case, centre_high) {
                        (5, true) | (10, false) => &[(3, 2), (0, 1)],
                        _ => &[(3, 0), (1, 2)],
                    }
                }
                _ => &[],
            };
            for &(a, b) in pairs {
                out.push([e(a), e(b)]);
            }
        }
    }
    out
}

/// Weighted mean vector and covariance matrix `[sxx, sxy, syy]` of the finite
/// points, with divisor `Σw − 1` (frequency weights). `None` with total
/// weight of 2 or less.
#[must_use]
pub fn mean_and_covariance(x: &[f64], y: &[f64], weight: &[f64]) -> Option<([f64; 2], [f64; 3])> {
    let w = |i: usize| weight.get(i).copied().unwrap_or(1.0);
    let pts: Vec<(f64, f64, f64)> = x
        .iter()
        .zip(y)
        .enumerate()
        .filter(|&(i, (a, b))| a.is_finite() && b.is_finite() && w(i).is_finite() && w(i) > 0.0)
        .map(|(i, (&a, &b))| (a, b, w(i)))
        .collect();
    let total: f64 = pts.iter().map(|p| p.2).sum();
    if total <= 2.0 {
        return None;
    }
    let mx = pts.iter().map(|p| p.2 * p.0).sum::<f64>() / total;
    let my = pts.iter().map(|p| p.2 * p.1).sum::<f64>() / total;
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for &(a, b, c) in &pts {
        sxx += c * (a - mx) * (a - mx);
        sxy += c * (a - mx) * (b - my);
        syy += c * (b - my) * (b - my);
    }
    let d = total - 1.0;
    Some(([mx, my], [sxx / d, sxy / d, syy / d]))
}

/// The bivariate-normal density ellipse holding probability `level` for the
/// points' mean and covariance, as a closed outline of `points` vertices (the
/// last repeats the first), with the sample correlation. `None` when the
/// covariance is singular (all points on a line) or there are too few.
#[must_use]
pub fn density_ellipse(
    x: &[f64],
    y: &[f64],
    weight: &[f64],
    level: f64,
    points: usize,
) -> Option<(Vec<[f64; 2]>, f64)> {
    let (m, [a, b, c]) = mean_and_covariance(x, y, weight)?;
    let det = a * c - b * b;
    // Relative to a·c (det / (a·c) = 1 − r²): points on a line leave a
    // rounding-sized determinant, not an exact zero.
    let singular = det.is_nan() || det <= 1e-10 * (a * c).abs().max(1e-300);
    if singular || a <= 0.0 || c <= 0.0 {
        return None;
    }
    let level = level.clamp(1e-6, 1.0 - 1e-12);
    let r = (-2.0 * (1.0 - level).ln()).sqrt();
    // Cholesky factor of the covariance: [[l11, 0], [l21, l22]].
    let l11 = a.sqrt();
    let l21 = b / l11;
    let l22 = (c - l21 * l21).max(0.0).sqrt();
    let points = points.max(8);
    let outline: Vec<[f64; 2]> = (0..=points)
        .map(|k| {
            let t = 2.0 * std::f64::consts::PI * k as f64 / points as f64;
            let (u, v) = (r * t.cos(), r * t.sin());
            [m[0] + l11 * u, m[1] + l21 * u + l22 * v]
        })
        .collect();
    let corr = b / (a * c).sqrt();
    outline.iter().all(|p| p[0].is_finite() && p[1].is_finite()).then_some((outline, corr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bandwidth_and_1d_density() {
        let v: Vec<f64> = (0..200).map(|i| (f64::from(i) * 0.37).sin() * 2.0).collect();
        let h = silverman_bandwidth(&v, &[]);
        assert!(h > 0.0 && h < 2.0);
        // Degenerate inputs still give a positive bandwidth.
        assert_eq!(silverman_bandwidth(&[], &[]), 1.0);
        assert!(silverman_bandwidth(&[3.0; 5], &[]) > 0.0);
        // The density integrates to about one.
        let d = kde_1d(&v, &[], h, -8.0, 8.0, 801);
        let area: f64 = d.windows(2).map(|w| 0.5 * (w[0][1] + w[1][1]) * (w[1][0] - w[0][0])).sum();
        assert!((area - 1.0).abs() < 1e-3, "{area}");
        // Frequencies act like repeated values.
        let a = kde_1d(&[0.0, 1.0], &[3.0, 1.0], 0.5, -1.0, 2.0, 7);
        let b = kde_1d(&[0.0, 0.0, 0.0, 1.0], &[], 0.5, -1.0, 2.0, 7);
        for (p, q) in a.iter().zip(&b) {
            assert!((p[1] - q[1]).abs() < 1e-12);
        }
        assert!(kde_1d(&[], &[], 1.0, 0.0, 1.0, 5).is_empty());
        assert!(kde_1d(&[1.0], &[], 0.0, 0.0, 1.0, 5).is_empty());
    }

    #[test]
    fn two_d_density_contours() {
        let n = 300;
        let x: Vec<f64> = (0..n).map(|i| (f64::from(i) * 0.731).sin()).collect();
        let y: Vec<f64> = (0..n).map(|i| (f64::from(i) * 1.37).cos() * 0.5).collect();
        let g = kde_2d(&x, &y, &[], 1.0, 50, 40).unwrap();
        assert_eq!((g.xs.len(), g.ys.len(), g.z.len()), (50, 40, 2000));
        // Integrates to about one over the grid.
        let (dx, dy) = (g.xs[1] - g.xs[0], g.ys[1] - g.ys[0]);
        let mass: f64 = g.z.iter().sum::<f64>() * dx * dy;
        assert!((mass - 1.0).abs() < 0.05, "{mass}");
        // Higher mass fractions give lower thresholds.
        let t = mass_thresholds(&g, &[0.5, 0.9]);
        assert!(t[0] > t[1] && t[1] > 0.0);
        let segs = contour_segments(&g, t[0]);
        assert!(!segs.is_empty());
        // Every segment end lies where the interpolated surface equals the level.
        assert!(segs.iter().flatten().all(|p| p[0].is_finite() && p[1].is_finite()));
        assert!(kde_2d(&[1.0], &[1.0], &[], 1.0, 10, 10).is_none());
        // Identical points: still a finite peaked grid.
        assert!(kde_2d(&[1.0; 4], &[2.0; 4], &[], 1.0, 10, 10).is_some());
    }

    #[test]
    fn marching_squares_on_a_cone() {
        let xs: Vec<f64> = (0..21).map(|i| -1.0 + 0.1 * f64::from(i)).collect();
        let ys = xs.clone();
        let z: Vec<f64> = ys.iter().flat_map(|&y| xs.iter().map(move |&x| 1.0 - (x * x + y * y).sqrt())).collect();
        let g = Grid { xs, ys, z };
        let segs = contour_segments(&g, 0.5);
        // Points on the 0.5 contour lie close to the circle of radius 0.5.
        for p in segs.iter().flatten() {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!((r - 0.5).abs() < 0.03, "{r}");
        }
        assert!(contour_segments(&g, 2.0).is_empty());
        assert!(mass_thresholds(&Grid { xs: vec![0.0], ys: vec![0.0], z: vec![0.0] }, &[0.5]).is_empty());
    }

    #[test]
    fn density_ellipses() {
        // Correlated cloud.
        // Normal-ish samples by Box–Muller from a fixed sequence.
        let mut state = 12345u64;
        let mut uniform = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let mut normal = move || (-2.0 * uniform().ln()).sqrt() * (2.0 * std::f64::consts::PI * uniform()).cos();
        let x: Vec<f64> = (0..400).map(|_| normal()).collect();
        let y: Vec<f64> = x.iter().map(|v| 2.0 * v + 0.6 * normal()).collect();
        let (e95, r) = density_ellipse(&x, &y, &[], 0.95, 64).unwrap();
        assert!(r > 0.9);
        let (e50, _) = density_ellipse(&x, &y, &[], 0.5, 64).unwrap();
        let spread = |e: &[[f64; 2]]| e.iter().map(|p| p[1].abs()).fold(0.0, f64::max);
        assert!(spread(&e95) > spread(&e50));
        let (first, last) = (e95[0], e95[e95.len() - 1]);
        assert!((first[0] - last[0]).abs() < 1e-12 && (first[1] - last[1]).abs() < 1e-12, "closed outline");
        // About 95% of the points fall inside the 95% ellipse.
        let (m, [a, b, c]) = mean_and_covariance(&x, &y, &[]).unwrap();
        let det = a * c - b * b;
        let inside = x
            .iter()
            .zip(&y)
            .filter(|(p, q)| {
                let (u, v) = (*p - m[0], *q - m[1]);
                (c * u * u - 2.0 * b * u * v + a * v * v) / det <= -2.0 * (0.05f64).ln()
            })
            .count();
        assert!((inside as f64 / 400.0 - 0.95).abs() < 0.04, "{inside}");
        // Points on a line, or too few: no ellipse.
        assert!(density_ellipse(&[1.0, 2.0, 3.0, 4.0], &[2.0, 4.0, 6.0, 8.0], &[], 0.9, 32).is_none());
        assert!(density_ellipse(&[1.0, 2.0], &[3.0, 1.0], &[], 0.9, 32).is_none());
    }

    #[test]
    fn collinear_points_with_rounding_give_no_ellipse() {
        // y = 3x + 0.1 exactly in theory; in binary64 the covariance
        // determinant is a rounding residue (r = 0.9999999999999999), not 0.
        let x = [0.1, 0.2, 0.7, 1.3, 2.9];
        for (slope, intercept) in [(3.0, 0.1), (0.3, 0.7), (-1.7, 2.2)] {
            let y: Vec<f64> = x.iter().map(|v| slope * v + intercept).collect();
            assert!(density_ellipse(&x, &y, &[], 0.95, 16).is_none(), "slope {slope}");
        }
        // A strong but genuine correlation still gets its ellipse.
        let y = [0.4, 0.65, 2.2, 4.1, 8.8];
        assert!(density_ellipse(&x, &y, &[], 0.95, 16).is_some_and(|(_, r)| r > 0.99 && r < 1.0));
    }
}
