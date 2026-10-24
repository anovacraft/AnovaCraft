//! Inverse prediction (calibration): reading an unknown predictor value off a
//! fitted curve from an observed response (Draper & Smith, *Applied
//! Regression Analysis*, 3rd ed., Wiley 1998, §3.2; Eisenhart, "The
//! interpretation of certain regression methods and their use in biological
//! and industrial research", *Ann. Math. Statist.* 10, 1939; Seber & Wild,
//! *Nonlinear Regression*, Wiley 1989, §5.12).
//!
//! # Method
//! For an observed response `y₀`, the estimate `x̂₀` solves `f(x̂₀; θ̂) = y₀`
//! within the calibration range `[a, b]` (normally the range of the
//! standards). The interval is the set of `x` whose prediction interval for
//! one new observation contains `y₀`:
//! `{x : |y₀ − f(x; θ̂)| ≤ t·√(se(x)² + s²/w₀)}`, with `se(x)` the
//! delta-method standard error of the fitted mean and `w₀` the weight of the
//! new observation (for a straight line this is the classical
//! Fieller-type interval of Draper & Smith eq. 3.2.6, exactly). The limits
//! are the crossings nearest `x̂₀` on each side.
//!
//! Roots are bracketed on a grid of 400 intervals over `[a, b]` and refined
//! by bisection to `10⁻¹²·(b − a)`.
//!
//! # Failure modes
//! * `y₀` not reached by the curve within `[a, b]`: no estimate.
//! * More than one solution (a curve that is not monotone): the one nearest
//!   `a` is reported and [`InversePrediction::several_solutions`] is set.
//! * A limit beyond `[a, b]` (a flat curve or a response near an asymptote):
//!   that side of the interval is open (`None`).

/// One inverse prediction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InversePrediction {
    /// The observed response `y₀`.
    pub response: f64,
    /// `x̂₀`, or `None` when the curve does not reach `y₀` in the range.
    pub estimate: Option<f64>,
    /// Lower limit (`None`: open, beyond the range).
    pub lower: Option<f64>,
    /// Upper limit (`None`: open, beyond the range).
    pub upper: Option<f64>,
    /// Whether the curve reaches `y₀` more than once in the range.
    pub several_solutions: bool,
}

const GRID: usize = 400;

fn bisect(g: &mut dyn FnMut(f64) -> Option<f64>, mut a: f64, mut ga: f64, mut b: f64, tol: f64) -> f64 {
    for _ in 0..200 {
        if (b - a).abs() <= tol {
            break;
        }
        let m = 0.5 * (a + b);
        let Some(gm) = g(m) else { break };
        if gm == 0.0 {
            return m;
        }
        if (gm < 0.0) == (ga < 0.0) {
            a = m;
            ga = gm;
        } else {
            b = m;
        }
    }
    0.5 * (a + b)
}

/// Inverse prediction of `y0` on `[a, b]`. `curve(x)` returns the fitted
/// value and the prediction-interval half-width `t·√(se(x)² + s²/w₀)` at
/// `x`, or `None` where the curve cannot be computed.
#[must_use]
pub fn inverse_predict(
    y0: f64,
    range: (f64, f64),
    curve: &mut dyn FnMut(f64) -> Option<(f64, f64)>,
) -> InversePrediction {
    let mut out =
        InversePrediction { response: y0, estimate: None, lower: None, upper: None, several_solutions: false };
    let (a, b) = range;
    if !(y0.is_finite() && a.is_finite() && b.is_finite() && b > a) {
        return out;
    }
    let tol = 1e-12 * (b - a);
    let xs: Vec<f64> = (0..=GRID).map(|i| a + (b - a) * i as f64 / GRID as f64).collect();
    let vals: Vec<Option<(f64, f64)>> = xs.iter().map(|&x| curve(x)).collect();
    // Estimate: sign changes of f − y₀.
    let mut roots = Vec::new();
    for i in 0..GRID {
        let (Some((f0, _)), Some((f1, _))) = (vals[i], vals[i + 1]) else { continue };
        let (g0, g1) = (f0 - y0, f1 - y0);
        if g0 == 0.0 {
            roots.push(xs[i]);
        } else if (g0 < 0.0) != (g1 < 0.0) && g1 != 0.0 {
            let mut g = |x: f64| curve(x).map(|(f, _)| f - y0);
            roots.push(bisect(&mut g, xs[i], g0, xs[i + 1], tol));
        } else if i + 1 == GRID && g1 == 0.0 {
            roots.push(xs[i + 1]);
        }
    }
    let Some(&x0) = roots.first() else { return out };
    out.estimate = Some(x0);
    out.several_solutions = roots.len() > 1;
    // Limits: where |f − y₀| − h crosses zero, nearest x̂₀ on each side.
    let mut excess = |x: f64| curve(x).map(|(f, h)| (f - y0).abs() - h);
    let k0 = ((x0 - a) / (b - a) * GRID as f64).floor().clamp(0.0, (GRID - 1) as f64) as usize;
    // Upward from x̂₀.
    let mut prev = (x0, excess(x0).unwrap_or(-1.0).min(0.0));
    for &x in xs.iter().skip(k0 + 1) {
        let Some(e) = excess(x) else { break };
        if e > 0.0 {
            out.upper = Some(bisect(&mut excess, prev.0, prev.1, x, tol));
            break;
        }
        prev = (x, e);
    }
    let mut prev = (x0, excess(x0).unwrap_or(-1.0).min(0.0));
    for &x in xs.iter().take(k0 + 1).rev() {
        if x >= x0 {
            continue;
        }
        let Some(e) = excess(x) else { break };
        if e > 0.0 {
            out.lower = Some(bisect(&mut excess, prev.0, prev.1, x, tol));
            break;
        }
        prev = (x, e);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_core::distributions::{ContinuousDistribution, StudentsT};

    /// A straight line: the interval equals the closed-form calibration
    /// interval of Draper & Smith (eq. 3.2.6), the roots of
    /// `(y₀ − a − bx)² = t²s²(1 + 1/n + (x − x̄)²/Sxx)`.
    #[test]
    fn straight_line_matches_the_closed_form() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let y = [2.1, 3.9, 6.2, 7.8, 10.1, 12.2, 13.8, 16.1];
        let n = x.len() as f64;
        let xb = x.iter().sum::<f64>() / n;
        let yb = y.iter().sum::<f64>() / n;
        let sxx: f64 = x.iter().map(|v| (v - xb).powi(2)).sum();
        let sxy: f64 = x.iter().zip(&y).map(|(a, b)| (a - xb) * (b - yb)).sum();
        let b1 = sxy / sxx;
        let b0 = yb - b1 * xb;
        let sse: f64 = x.iter().zip(&y).map(|(a, b)| (b - b0 - b1 * a).powi(2)).sum();
        let s2 = sse / (n - 2.0);
        let t = StudentsT::new(n - 2.0).unwrap().quantile(0.975).unwrap();
        let y0 = 9.0;
        let mut curve = |v: f64| {
            let se2 = s2 * (1.0 / n + (v - xb).powi(2) / sxx);
            Some((b0 + b1 * v, t * (se2 + s2).sqrt()))
        };
        let r = inverse_predict(y0, (1.0, 8.0), &mut curve);
        assert!((r.estimate.unwrap() - (y0 - b0) / b1).abs() < 1e-10);
        // Quadratic A u² + B u + C = 0 in u = x − x̄.
        let d = y0 - b0 - b1 * xb;
        let qa = b1 * b1 - t * t * s2 / sxx;
        let qb = -2.0 * b1 * d;
        let qc = d * d - t * t * s2 * (1.0 + 1.0 / n);
        let disc = (qb * qb - 4.0 * qa * qc).sqrt();
        let (l, u) = ((-qb - disc) / (2.0 * qa) + xb, (-qb + disc) / (2.0 * qa) + xb);
        assert!((r.lower.unwrap() - l).abs() < 1e-9, "{:?} vs {l}", r.lower);
        assert!((r.upper.unwrap() - u).abs() < 1e-9, "{:?} vs {u}", r.upper);
        // Outside the range: no estimate.
        assert!(inverse_predict(40.0, (1.0, 8.0), &mut curve).estimate.is_none());
        // Near the end of the range: the interval is open on that side.
        let r = inverse_predict(16.0, (1.0, 8.0), &mut curve);
        assert!(r.estimate.is_some() && r.upper.is_none() && r.lower.is_some());
    }
}
