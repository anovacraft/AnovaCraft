//! One-dimensional numerical integration for posterior quantities.
//!
//! * [`integrate`]: adaptive Gauss–Kronrod (7-point Gauss, 15-point
//!   Kronrod) quadrature on a finite interval, bisecting the panel with the
//!   largest error estimate until the total estimate meets the tolerance
//!   (Piessens et al., *QUADPACK*, Springer 1983, routine QAG).
//! * [`integrate_log`]: the logarithm of `∫ exp(ln f(x)) dx` over the real
//!   line for a log-integrand with one dominant peak (Bayes factor and
//!   marginal-likelihood integrands in log g). The peak is located on a grid,
//!   the range is cut where the integrand falls 50 nats below its maximum,
//!   and the scaled integrand is integrated adaptively, so results neither
//!   overflow nor underflow.

/// Kronrod nodes of the 15-point rule on [−1, 1] (QUADPACK `qk15`).
const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_5,
    0.0,
];
/// Kronrod weights.
const WGK: [f64; 8] = [
    0.022_935_322_010_529_22,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_18,
    0.140_653_259_715_525_92,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_83,
];
/// Gauss weights of the embedded 7-point rule (nodes `XGK[1]`, `[3]`, `[5]`, `[7]`).
const WG: [f64; 4] =
    [0.129_484_966_168_869_7, 0.279_705_391_489_276_7, 0.381_830_050_505_118_9, 0.417_959_183_673_469_4];

/// One 15-point panel: (Kronrod estimate, |Kronrod − Gauss|).
fn panel(f: &mut impl FnMut(f64) -> f64, a: f64, b: f64) -> (f64, f64) {
    let c = 0.5 * (a + b);
    let h = 0.5 * (b - a);
    let fc = f(c);
    let mut k = WGK[7] * fc;
    let mut g = WG[3] * fc;
    for j in 0..7 {
        let x = h * XGK[j];
        let s = f(c - x) + f(c + x);
        k += WGK[j] * s;
        if j % 2 == 1 {
            g += WG[j / 2] * s;
        }
    }
    (k * h, ((k - g) * h).abs())
}

/// `∫ₐᵇ f(x) dx` by adaptive Gauss–Kronrod quadrature to a relative
/// tolerance `rel_tol` (with an absolute floor of `1e-300`), starting from
/// `panels` equal panels and using at most 2000 panels. Returns the
/// estimate; non-finite values of `f` make it non-finite.
pub fn integrate(mut f: impl FnMut(f64) -> f64, a: f64, b: f64, rel_tol: f64, panels: usize) -> f64 {
    if a == b {
        return 0.0;
    }
    let panels = panels.max(1);
    let w = (b - a) / panels as f64;
    let mut list: Vec<(f64, f64, f64, f64)> = (0..panels)
        .map(|i| {
            let (lo, hi) = (a + i as f64 * w, if i + 1 == panels { b } else { a + (i + 1) as f64 * w });
            let (v, e) = panel(&mut f, lo, hi);
            (lo, hi, v, e)
        })
        .collect();
    for _ in 0..2000 {
        let total: f64 = list.iter().map(|p| p.2).sum();
        let err: f64 = list.iter().map(|p| p.3).sum();
        if !total.is_finite() || err <= (rel_tol * total.abs()).max(1e-300) {
            break;
        }
        let worst = list.iter().enumerate().max_by(|x, y| x.1.3.total_cmp(&y.1.3)).map_or(0, |(i, _)| i);
        let (lo, hi, _, _) = list.swap_remove(worst);
        let mid = 0.5 * (lo + hi);
        let (v1, e1) = panel(&mut f, lo, mid);
        let (v2, e2) = panel(&mut f, mid, hi);
        list.push((lo, mid, v1, e1));
        list.push((mid, hi, v2, e2));
    }
    list.iter().map(|p| p.2).sum()
}

/// The range over which `exp(ln_f)` is within 50 nats of its maximum:
/// `(lower, upper, maximum of ln_f)`. The peak is searched for on a grid
/// over `[−search, search]` (step `search/200`) and the range extended in
/// steps of a tenth of the grid's half-width. `None` when `ln_f` is
/// nowhere finite on the grid.
pub fn peak_range(ln_f: &impl Fn(f64) -> f64, search: f64) -> Option<(f64, f64, f64)> {
    let step = search / 200.0;
    let mut best = (f64::NEG_INFINITY, 0.0);
    for i in 0..=400 {
        let x = -search + i as f64 * step;
        let v = ln_f(x);
        if v.is_finite() && v > best.0 {
            best = (v, x);
        }
    }
    if !best.0.is_finite() {
        return None;
    }
    // Refine the maximum by golden-section search on the bracketing cells.
    let (mut lo, mut hi) = (best.1 - step, best.1 + step);
    let phi = 0.5 * (5f64.sqrt() - 1.0);
    for _ in 0..60 {
        let x1 = hi - phi * (hi - lo);
        let x2 = lo + phi * (hi - lo);
        if ln_f(x1) >= ln_f(x2) {
            hi = x2;
        } else {
            lo = x1;
        }
    }
    let mode = 0.5 * (lo + hi);
    let peak = ln_f(mode).max(best.0);
    let reach = |dir: f64| {
        let mut x = mode;
        let mut h = step.max(1e-3);
        for _ in 0..4000 {
            let next = x + dir * h;
            let v = ln_f(next);
            x = next;
            if v.is_nan() || v <= peak - 50.0 {
                break;
            }
            h *= 1.25;
        }
        x
    };
    Some((reach(-1.0), reach(1.0), peak))
}

/// `ln ∫ exp(ln_f(x)) dx` over the real line, for a log-integrand with one
/// dominant peak inside `[−search, search]` (see [`peak_range`]). Returns
/// `−∞` when the integrand is zero everywhere on the search grid.
pub fn integrate_log(ln_f: impl Fn(f64) -> f64, search: f64) -> f64 {
    let Some((a, b, peak)) = peak_range(&ln_f, search) else {
        return f64::NEG_INFINITY;
    };
    let v = integrate(
        |x| {
            let l = ln_f(x) - peak;
            if l.is_finite() { l.exp() } else { 0.0 }
        },
        a,
        b,
        1e-11,
        16,
    );
    peak + v.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrates_known_functions() {
        let v = integrate(f64::sin, 0.0, std::f64::consts::PI, 1e-12, 1);
        assert!((v - 2.0).abs() < 1e-12);
        let v = integrate(|x| (-x * x).exp(), -10.0, 10.0, 1e-12, 1);
        assert!((v - std::f64::consts::PI.sqrt()).abs() < 1e-12);
        // A narrow peak far from zero, on the log scale.
        let l = integrate_log(|x| -0.5 * ((x - 30.0) / 0.01).powi(2) + 700.0, 40.0);
        let exact = 700.0 + (0.01 * (2.0 * std::f64::consts::PI).sqrt()).ln();
        assert!((l - exact).abs() < 1e-9, "{l} vs {exact}");
    }
}
