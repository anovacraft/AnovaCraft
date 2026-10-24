//! Area under a sampled curve by the trapezoid rule, above a baseline, with
//! its standard error from replicate measurements.
//!
//! The curve joins the mean response `ȳⱼ` at each distinct `xⱼ`
//! (`x₁ < … < x_m`) with straight lines. Its area is `Σ cⱼ ȳⱼ` with
//! trapezoid weights `c₁ = (x₂ − x₁)/2`, `cⱼ = (xⱼ₊₁ − xⱼ₋₁)/2`,
//! `c_m = (x_m − x_{m−1})/2`. Above a baseline `b` the net area is
//! `Σ cⱼ ȳⱼ − b (x_m − x₁)`. When the baseline is the mean of the first `k`
//! points it is itself a linear combination of the `ȳⱼ`, so the net area is
//! `Σ wⱼ ȳⱼ` with `wⱼ = cⱼ − (x_m − x₁)/k` for `j ≤ k`.
//!
//! With independent replicates at each `x` (for example a different animal
//! at each time, as in destructive sampling), Bailer (1988, *J.
//! Pharmacokinet. Biopharm.* 16, 303–309) gives `Var = Σ wⱼ² sⱼ² / nⱼ` from
//! each point's sample variance `sⱼ²` and count `nⱼ`, and a normal
//! confidence interval `area ± z sd`. It needs at least two replicates at
//! every point.
//!
//! Where a segment crosses the baseline it is split at the crossing, so the
//! area above and the area below the baseline are reported separately (their
//! difference is the net area). A peak is a run of the curve above the
//! baseline between crossings (or the ends of the data).

use crate::common::invalid;
use stat_core::Result;
use stat_core::distributions::{ContinuousDistribution, Normal};

/// Where the baseline of an area comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AreaBaseline {
    /// A given value (0 for the area under the curve itself).
    Value(f64),
    /// The mean of the curve's first `k` points (`k ≥ 1`).
    FirstPoints(usize),
}

/// A region of the curve above the baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaPeak {
    /// Where the region starts (a crossing or the first `x`).
    pub start: f64,
    /// Where it ends (a crossing or the last `x`).
    pub end: f64,
    /// `x` of the highest point.
    pub peak_x: f64,
    /// Height of the highest point above the baseline.
    pub height: f64,
    /// Area of the region above the baseline.
    pub area: f64,
}

/// Area under a curve.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaUnderCurve {
    /// Distinct `x` values, ascending.
    pub x: Vec<f64>,
    /// Mean response at each `x`.
    pub mean: Vec<f64>,
    /// Replicates at each `x`.
    pub count: Vec<usize>,
    /// Baseline value.
    pub baseline: f64,
    /// Net area: above minus below the baseline.
    pub net_area: f64,
    /// Area of the parts of the curve above the baseline.
    pub area_above: f64,
    /// Area of the parts below the baseline (positive).
    pub area_below: f64,
    /// Bailer's standard error of the net area (`None` without at least two
    /// replicates at every `x`).
    pub std_error: Option<f64>,
    /// Normal-theory confidence interval of the net area.
    pub interval: Option<(f64, f64)>,
    /// Regions above the baseline, left to right, those lower than the
    /// requested share of the tallest left out.
    pub peaks: Vec<AreaPeak>,
}

/// Trapezoid area of `y` against `x` above `baseline`, with Bailer's
/// standard error when every distinct `x` has at least two `y` values.
/// `level` is the confidence level of the interval; peaks lower than
/// `min_peak_share` × the tallest peak's height are left out of the list.
/// Pairs with a non-finite value are ignored.
///
/// # Errors
/// Mismatched lengths, fewer than two distinct `x` values, a `FirstPoints`
/// baseline of zero or more points than there are, or a level outside (0, 1).
pub fn area_under_curve(
    x: &[f64],
    y: &[f64],
    baseline: AreaBaseline,
    level: f64,
    min_peak_share: f64,
) -> Result<AreaUnderCurve> {
    if x.len() != y.len() {
        return Err(invalid("x and y must have the same length"));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must lie between 0 and 1"));
    }
    let mut pairs: Vec<(f64, f64)> =
        x.iter().zip(y).filter(|(a, b)| a.is_finite() && b.is_finite()).map(|(&a, &b)| (a, b)).collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Group replicates by x.
    let (mut xs, mut sums, mut sq, mut count) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut values: Vec<Vec<f64>> = Vec::new();
    for (a, b) in pairs {
        if xs.last() != Some(&a) {
            xs.push(a);
            values.push(Vec::new());
        }
        if let Some(v) = values.last_mut() {
            v.push(b);
        }
    }
    for v in &values {
        let n = v.len() as f64;
        let m = v.iter().sum::<f64>() / n;
        sums.push(m);
        sq.push(if v.len() > 1 { v.iter().map(|t| (t - m).powi(2)).sum::<f64>() / (n - 1.0) } else { f64::NAN });
        count.push(v.len());
    }
    let mean = sums;
    let m = xs.len();
    if m < 2 {
        return Err(invalid("the area needs at least two distinct x values"));
    }
    let span = xs[m - 1] - xs[0];
    let mut w: Vec<f64> = (0..m)
        .map(|j| {
            let left = if j > 0 { xs[j] - xs[j - 1] } else { 0.0 };
            let right = if j + 1 < m { xs[j + 1] - xs[j] } else { 0.0 };
            (left + right) / 2.0
        })
        .collect();
    let base = match baseline {
        AreaBaseline::Value(b) if b.is_finite() => b,
        AreaBaseline::Value(_) => return Err(invalid("the baseline must be finite")),
        AreaBaseline::FirstPoints(k) => {
            if k == 0 || k > m {
                return Err(invalid(format!("the baseline needs between 1 and {m} points")));
            }
            for wj in w.iter_mut().take(k) {
                *wj -= span / k as f64;
            }
            mean[..k].iter().sum::<f64>() / k as f64
        }
    };
    // A given baseline is a constant: it moves the area, not its variance.
    let gross: f64 = (0..m)
        .map(|j| {
            let left = if j > 0 { xs[j] - xs[j - 1] } else { 0.0 };
            let right = if j + 1 < m { xs[j + 1] - xs[j] } else { 0.0 };
            (left + right) / 2.0 * mean[j]
        })
        .sum();
    let net_area = gross - base * span;
    let std_error = if count.iter().all(|&c| c >= 2) {
        Some((0..m).map(|j| w[j] * w[j] * sq[j] / count[j] as f64).sum::<f64>().sqrt())
    } else {
        None
    };
    let z = Normal::new(0.0, 1.0)?.quantile(0.5 + level / 2.0)?;
    let interval = std_error.map(|se| (net_area - z * se, net_area + z * se));
    // Areas above and below, splitting segments at crossings, and peaks.
    let d: Vec<f64> = mean.iter().map(|v| v - base).collect();
    let (mut above, mut below) = (0.0, 0.0);
    let mut peaks: Vec<AreaPeak> = Vec::new();
    let mut open: Option<AreaPeak> = None;
    let visit = |open: &mut Option<AreaPeak>, x: f64, h: f64| {
        if let Some(p) = open.as_mut() {
            if h > p.height {
                p.height = h;
                p.peak_x = x;
            }
        }
    };
    if d[0] > 0.0 {
        open = Some(AreaPeak { start: xs[0], end: xs[0], peak_x: xs[0], height: d[0], area: 0.0 });
    }
    for j in 0..m - 1 {
        let (x0, x1, d0, d1) = (xs[j], xs[j + 1], d[j], d[j + 1]);
        if d0 * d1 < 0.0 {
            let xc = x0 + d0 / (d0 - d1) * (x1 - x0);
            let (a0, a1) = (d0 * (xc - x0) / 2.0, d1 * (x1 - xc) / 2.0);
            if d0 > 0.0 {
                above += a0;
                below -= a1;
                if let Some(mut p) = open.take() {
                    p.area += a0;
                    p.end = xc;
                    peaks.push(p);
                }
            } else {
                below -= a0;
                above += a1;
                open = Some(AreaPeak { start: xc, end: x1, peak_x: x1, height: d1, area: a1 });
            }
        } else {
            let a = (d0 + d1) / 2.0 * (x1 - x0);
            if d0 > 0.0 || d1 > 0.0 {
                above += a;
                if open.is_none() {
                    open = Some(AreaPeak { start: x0, end: x1, peak_x: x1, height: d1, area: 0.0 });
                }
                if let Some(p) = open.as_mut() {
                    p.area += a;
                    p.end = x1;
                }
                visit(&mut open, x1, d1);
            } else {
                below -= a;
                if let Some(p) = open.take() {
                    peaks.push(p);
                }
            }
        }
    }
    if let Some(p) = open.take() {
        peaks.push(p);
    }
    let tallest = peaks.iter().map(|p| p.height).fold(0.0, f64::max);
    peaks.retain(|p| p.height > 0.0 && p.height >= min_peak_share * tallest);
    Ok(AreaUnderCurve {
        x: xs,
        mean,
        count,
        baseline: base,
        net_area,
        area_above: above,
        area_below: below,
        std_error,
        interval,
        peaks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    /// Hand calculation: points (0, 1), (1, 3), (3, −1), (4, 1) above 0.
    /// Trapezoids give a net area of 2 + 2 + 0 = 4; the crossings at x = 2.5
    /// and 3.5 split it into 2 + 3·1.5/2 + 1·0.5/2 = 4.5 above and
    /// 1·0.5/2 + 1·0.5/2 = 0.5 below.
    #[test]
    fn crossings_and_peaks() {
        let x = [0.0, 1.0, 3.0, 4.0];
        let y = [1.0, 3.0, -1.0, 1.0];
        let a = area_under_curve(&x, &y, AreaBaseline::Value(0.0), 0.95, 0.0).unwrap();
        close(a.net_area, 4.0, 1e-14);
        close(a.area_above, 4.5, 1e-14);
        close(a.area_below, 0.5, 1e-14);
        assert!(a.std_error.is_none(), "no replicates");
        assert_eq!(a.peaks.len(), 2);
        let p = &a.peaks[0];
        assert_eq!((p.start, p.end, p.peak_x, p.height), (0.0, 2.5, 1.0, 3.0));
        close(p.area, 4.25, 1e-14);
        let q = &a.peaks[1];
        assert_eq!((q.start, q.end, q.peak_x, q.height), (3.5, 4.0, 4.0, 1.0));
        close(q.area, 0.25, 1e-14);
        // Peaks lower than half the tallest are left out.
        let a = area_under_curve(&x, &y, AreaBaseline::Value(0.0), 0.95, 0.5).unwrap();
        assert_eq!(a.peaks.len(), 1);
        // Baseline 1 (the first point): net = 4 − 1·4 = 0.
        let b = area_under_curve(&x, &y, AreaBaseline::FirstPoints(1), 0.95, 0.0).unwrap();
        close(b.baseline, 1.0, 1e-14);
        close(b.net_area, 0.0, 1e-14);
        close(b.area_above - b.area_below, 0.0, 1e-14);
        assert!(area_under_curve(&x, &y, AreaBaseline::FirstPoints(5), 0.95, 0.0).is_err());
        assert!(area_under_curve(&[1.0], &[1.0], AreaBaseline::Value(0.0), 0.95, 0.0).is_err());
    }

    /// Bailer's variance with replicates: three animals at each of four
    /// times, checked against R (`tests/data/area_reference.R`):
    /// AUC 75.25, SE 3.15238…, and with the mean of the first two points
    /// (11.75) as baseline −18.75, SE 5.44097… (the baseline's own
    /// uncertainty included).
    #[test]
    fn bailer_standard_error() {
        let x = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 4.0, 4.0, 4.0, 8.0, 8.0, 8.0];
        let y = [2.0, 3.0, 2.5, 20.0, 24.0, 19.0, 15.0, 13.0, 17.0, 9.0, 7.0, 8.5, 3.0, 2.0, 4.0];
        let a = area_under_curve(&x, &y, AreaBaseline::Value(0.0), 0.95, 0.0).unwrap();
        close(a.net_area, AUC, 1e-13);
        close(a.std_error.unwrap(), SE, 1e-12);
        let (lo, hi) = a.interval.unwrap();
        close(lo, AUC - 1.959_963_984_540_054 * SE, 1e-12);
        close(hi, AUC + 1.959_963_984_540_054 * SE, 1e-12);
        let b = area_under_curve(&x, &y, AreaBaseline::FirstPoints(2), 0.95, 0.0).unwrap();
        close(b.net_area, AUC_B, 1e-13);
        close(b.std_error.unwrap(), SE_B, 1e-12);
    }

    const AUC: f64 = 75.25;
    const SE: f64 = 3.152_380_053_229_623;
    const AUC_B: f64 = -18.75;
    const SE_B: f64 = 5.440_971_114_301_809;
}
