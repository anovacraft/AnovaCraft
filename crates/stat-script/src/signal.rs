//! Signal and spectrum functions on arrays of numbers (`stat_signal`):
//! smoothing filters, derivatives, running integrals, interpolation, baseline
//! estimation, peak finding and multi-peak fitting. Signals are evenly spaced
//! arrays in order; the x–y functions take the x values as their own array.
//! Missing values (`()`) are skipped by the filters and stay missing in their
//! result; the x–y functions drop pairs with a missing value.

use crate::value::{ScriptResult, as_number, fail, number, script_err, to_json};
use rhai::{Array, Dynamic, Engine, FLOAT, Map};
use stat_signal::peak_fit::{BaselineTerm, PeakFit, PeakFitSpec, PeakStart, Spacing, compare_peak_counts};
use stat_signal::peaks::position_at;
use stat_signal::{BaselineMethod, Integration, Interpolation, PeakSearch, PeakShape};

/// Register the functions.
pub(crate) fn register(engine: &mut Engine) {
    engine.register_fn("smooth", |y: Array| smooth(&y, &Map::new()));
    engine.register_fn("smooth", |y: Array, o: Map| smooth(&y, &o));
    engine.register_fn("derivative", |x: Array, y: Array| derivative(&x, &y));
    engine.register_fn("cumulative_integral", |x: Array, y: Array| integral(&x, &y, &Map::new()));
    engine.register_fn("cumulative_integral", |x: Array, y: Array, o: Map| integral(&x, &y, &o));
    engine.register_fn("interpolate", |x: Array, y: Array, at: Dynamic| interpolate(&x, &y, &at, &Map::new()));
    engine.register_fn("interpolate", |x: Array, y: Array, at: Dynamic, o: Map| interpolate(&x, &y, &at, &o));
    engine.register_fn("baseline", |y: Array| baseline(&y, &Map::new()));
    engine.register_fn("baseline", |y: Array, o: Map| baseline(&y, &o));
    engine.register_fn("noise_level", |y: Array| -> ScriptResult<FLOAT> { Ok(stat_signal::noise_level(&present(&y)?.1)) });
    engine.register_fn("find_peaks", |y: Array| find_peaks(&y, &Map::new()));
    engine.register_fn("find_peaks", |y: Array, o: Map| find_peaks(&y, &o));
    engine.register_fn("fit_peaks", |x: Array, y: Array| fit_peaks(&x, &y, &Map::new()));
    engine.register_fn("fit_peaks", |x: Array, y: Array, o: Map| fit_peaks(&x, &y, &o));
}

/// Positions and values of the non-missing numbers of an array.
fn present(a: &Array) -> ScriptResult<(Vec<usize>, Vec<f64>)> {
    let mut at = Vec::with_capacity(a.len());
    let mut values = Vec::with_capacity(a.len());
    for (i, v) in a.iter().enumerate() {
        match as_number(v) {
            Some(x) if x.is_nan() => {}
            Some(x) => {
                at.push(i);
                values.push(x);
            }
            None => return fail(format!("expected numbers (found {})", v.type_name())),
        }
    }
    Ok((at, values))
}

/// The (x, y) pairs with both present, sorted by x, with their positions.
fn pairs(x: &Array, y: &Array) -> ScriptResult<(Vec<usize>, Vec<f64>, Vec<f64>)> {
    if x.len() != y.len() {
        return fail(format!("x has {} values and y has {}", x.len(), y.len()));
    }
    let mut p = Vec::new();
    for (i, (a, b)) in x.iter().zip(y).enumerate() {
        let (Some(a), Some(b)) = (as_number(a), as_number(b)) else {
            return fail("x and y must be numbers");
        };
        if a.is_finite() && b.is_finite() {
            p.push((i, a, b));
        }
    }
    p.sort_by(|a, b| a.1.total_cmp(&b.1));
    Ok((p.iter().map(|t| t.0).collect(), p.iter().map(|t| t.1).collect(), p.iter().map(|t| t.2).collect()))
}

/// `values` placed back at positions `at` of an array of length `n`.
fn scatter(n: usize, at: &[usize], values: &[f64]) -> Array {
    let mut out = vec![Dynamic::UNIT; n];
    for (&i, &v) in at.iter().zip(values) {
        out[i] = f(v);
    }
    out
}

fn f(x: f64) -> Dynamic {
    if x.is_nan() { Dynamic::UNIT } else { (x as FLOAT).into() }
}

fn opt_number(o: &Map, key: &str, default: f64) -> ScriptResult<f64> {
    match o.get(key) {
        None => Ok(default),
        Some(v) => match as_number(v) {
            Some(x) if !x.is_nan() => Ok(x),
            _ => fail(format!("`{key}` must be a number")),
        },
    }
}

fn opt_count(o: &Map, key: &str, default: usize) -> ScriptResult<usize> {
    let v = opt_number(o, key, default as f64)?;
    if v < 0.0 || v.fract() != 0.0 {
        return fail(format!("`{key}` must be a whole number, 0 or more"));
    }
    Ok(v as usize)
}

fn opt_bool(o: &Map, key: &str, default: bool) -> ScriptResult<bool> {
    o.get(key).map_or(Ok(default), |v| v.as_bool().map_err(|_| script_err(format!("`{key}` must be true or false"))))
}

fn opt_text(o: &Map, key: &str, default: &str) -> String {
    o.get(key).map_or_else(|| default.to_owned(), |v| v.to_string())
}

fn smooth(y: &Array, o: &Map) -> ScriptResult<Array> {
    let smoother = app_model::signal_curves::smoother_from_json(&to_json(&Dynamic::from_map(o.clone()))?)
        .map_err(script_err)?;
    let (at, values) = present(y)?;
    let out = stat_signal::smooth(&values, &smoother, opt_number(o, "spacing", 1.0)?).map_err(script_err)?;
    Ok(scatter(y.len(), &at, &out))
}

fn derivative(x: &Array, y: &Array) -> ScriptResult<Array> {
    let (at, xs, ys) = pairs(x, y)?;
    let d = stat_signal::gradient(&xs, &ys).map_err(script_err)?;
    Ok(scatter(y.len(), &at, &d))
}

fn integral(x: &Array, y: &Array, o: &Map) -> ScriptResult<Array> {
    let rule = match opt_text(o, "rule", "trapezoid").as_str() {
        "trapezoid" => Integration::Trapezoid,
        "simpson" => Integration::Simpson,
        other => return fail(format!("unknown `rule` \"{other}\" (use \"trapezoid\" or \"simpson\")")),
    };
    let (at, xs, ys) = pairs(x, y)?;
    let i = stat_signal::cumulative_integral(&xs, &ys, rule).map_err(script_err)?;
    Ok(scatter(y.len(), &at, &i))
}

fn interpolate(x: &Array, y: &Array, at: &Dynamic, o: &Map) -> ScriptResult<Dynamic> {
    let method_key = opt_text(o, "method", "linear");
    let Some(method) = Interpolation::from_key(&method_key) else {
        return fail(format!(
            "unknown `method` \"{method_key}\" (use \"linear\", \"spline\", \"natural-spline\" or \"pchip\")"
        ));
    };
    let (_, xs, ys) = pairs(x, y)?;
    let g = stat_signal::Interpolator::new(&xs, &ys, method).map_err(script_err)?;
    let eval = |v: &Dynamic| match as_number(v) {
        Some(t) if t.is_finite() => Ok(f(g.eval(t))),
        Some(_) => Ok(Dynamic::UNIT),
        None => fail("expected numbers to interpolate at"),
    };
    match at.clone().try_cast::<Array>() {
        Some(a) => Ok(Dynamic::from_array(a.iter().map(eval).collect::<ScriptResult<Array>>()?)),
        None => eval(at),
    }
}

fn baseline(y: &Array, o: &Map) -> ScriptResult<Array> {
    let method = match opt_text(o, "method", "arpls").as_str() {
        "asls" => BaselineMethod::Asls { lambda: opt_number(o, "lambda", 1e5)?, p: opt_number(o, "p", 0.01)? },
        "arpls" => BaselineMethod::Arpls { lambda: opt_number(o, "lambda", 1e5)? },
        "snip" => BaselineMethod::Snip {
            half_window: opt_count(o, "half_window", 20)?,
            decreasing: opt_bool(o, "decreasing", true)?,
            lls: opt_bool(o, "lls", false)?,
        },
        other => return fail(format!("unknown `method` \"{other}\" (use \"asls\", \"arpls\" or \"snip\")")),
    };
    let (at, values) = present(y)?;
    let b = stat_signal::estimate_baseline(&values, &method).map_err(script_err)?;
    Ok(scatter(y.len(), &at, &b))
}

fn find_peaks(y: &Array, o: &Map) -> ScriptResult<Array> {
    let (at, values) = present(y)?;
    let search = PeakSearch {
        min_height: opt_number(o, "min_height", f64::NEG_INFINITY)?,
        min_prominence: opt_number(o, "min_prominence", 0.0)?,
        min_distance: opt_count(o, "min_distance", 1)?.max(1),
    };
    let peaks = stat_signal::find_peaks(&values, &search).map_err(script_err)?;
    // Fractional positions (crossings) mapped back to the array's positions.
    let pos = |t: f64| {
        let xs: Vec<f64> = at.iter().map(|&i| i as f64).collect();
        position_at(&xs, t)
    };
    Ok(peaks
        .iter()
        .map(|p| {
            let mut m = Map::new();
            m.insert("index".into(), Dynamic::from_int(at[p.index] as rhai::INT));
            m.insert("height".into(), number(p.height));
            m.insert("prominence".into(), number(p.prominence));
            m.insert("left_base".into(), Dynamic::from_int(at[p.left_base] as rhai::INT));
            m.insert("right_base".into(), Dynamic::from_int(at[p.right_base] as rhai::INT));
            m.insert("width".into(), number(pos(p.right_crossing) - pos(p.left_crossing)));
            Dynamic::from_map(m)
        })
        .collect())
}

/// Starting values: the given `centres`, else the `peaks` most prominent
/// local maxima of y (at least 5 noise levels prominent).
fn starts(xs: &[f64], ys: &[f64], o: &Map) -> ScriptResult<Vec<PeakStart>> {
    let range = xs.last().unwrap_or(&0.0) - xs.first().unwrap_or(&0.0);
    let height_at = |c: f64| {
        let i = xs.partition_point(|&v| v < c).min(xs.len().saturating_sub(1));
        ys[i].abs().max(1e-12)
    };
    if let Some(c) = o.get("centres") {
        let Some(c) = c.clone().try_cast::<Array>() else { return fail("`centres` must be an array of numbers") };
        let width = opt_number(o, "fwhm", range / (4.0 * c.len().max(1) as f64))?;
        return c
            .iter()
            .map(|v| match as_number(v) {
                Some(centre) if centre.is_finite() => Ok(PeakStart { centre, height: height_at(centre), fwhm: width }),
                _ => fail("`centres` must be numbers"),
            })
            .collect();
    }
    let noise = stat_signal::noise_level(ys);
    let search = PeakSearch { min_height: f64::NEG_INFINITY, min_prominence: 5.0 * noise, min_distance: 1 };
    let mut found = stat_signal::find_peaks(ys, &search).map_err(script_err)?;
    found.sort_by(|a, b| b.prominence.total_cmp(&a.prominence));
    let keep = opt_count(o, "peaks", found.len().min(app_model::signal_curves::MAX_FIT_PEAKS))?;
    if keep > found.len() {
        return fail(format!("asked for {keep} peaks but found {}; give `centres` instead", found.len()));
    }
    let min_width = 2.0 * range.abs() / (xs.len().max(2) - 1) as f64;
    Ok(found[..keep]
        .iter()
        .map(|p| PeakStart {
            centre: xs[p.index],
            height: p.prominence,
            fwhm: (position_at(xs, p.right_crossing) - position_at(xs, p.left_crossing)).abs().max(min_width),
        })
        .collect())
}

fn fit_map(fit: &PeakFit) -> Map {
    let pair = |m: &mut Map, key: &str, (est, se): (f64, f64)| {
        m.insert(key.into(), f(est));
        m.insert(format!("{key}_se").into(), f(se));
    };
    let peaks: Array = fit
        .peaks
        .iter()
        .map(|p| {
            let mut m = Map::new();
            pair(&mut m, "centre", p.centre);
            pair(&mut m, "height", p.height);
            pair(&mut m, "fwhm", p.fwhm);
            pair(&mut m, "area", p.area);
            m.insert("parameters".into(), Dynamic::from_array(p.params.iter().map(|&v| f(v)).collect()));
            m.insert("parameter_se".into(), Dynamic::from_array(p.param_se.iter().map(|&v| f(v)).collect()));
            Dynamic::from_map(m)
        })
        .collect();
    let mut m = Map::new();
    m.insert("shape".into(), fit.spec.shape.key().into());
    m.insert(
        "parameter_names".into(),
        Dynamic::from_array(fit.spec.shape.param_names().iter().map(|s| Dynamic::from(s.to_string())).collect()),
    );
    m.insert("peaks".into(), Dynamic::from_array(peaks));
    m.insert("baseline".into(), Dynamic::from_array(fit.baseline.iter().map(|b| f(b.0)).collect()));
    m.insert("baseline_se".into(), Dynamic::from_array(fit.baseline.iter().map(|b| f(b.1)).collect()));
    m.insert("n".into(), number(fit.n as f64));
    m.insert("parameters".into(), number(fit.n_free as f64));
    m.insert("sse".into(), f(fit.sse));
    m.insert("sigma".into(), f(fit.sigma));
    m.insert("r_squared".into(), f(fit.r_squared));
    m.insert("aicc".into(), f(fit.aicc));
    m.insert("bic".into(), f(fit.bic));
    m.insert("converged".into(), fit.converged().into());
    m.insert("termination".into(), fit.termination.label().into());
    m.insert("fitted".into(), Dynamic::from_array(fit.fitted.iter().map(|&v| f(v)).collect()));
    m
}

fn fit_peaks(x: &Array, y: &Array, o: &Map) -> ScriptResult<Map> {
    let (_, xs, ys) = pairs(x, y)?;
    let shape_key = opt_text(o, "shape", "gaussian");
    let Some(shape) = PeakShape::from_key(&shape_key) else {
        let keys: Vec<&str> = PeakShape::ALL.iter().map(|s| s.key()).collect();
        return fail(format!("unknown `shape` \"{shape_key}\" (use one of: {})", keys.join(", ")));
    };
    let base_key = opt_text(o, "baseline", "linear");
    let Some(baseline) = BaselineTerm::from_key(&base_key) else {
        return fail(format!("unknown `baseline` \"{base_key}\" (use \"none\", \"constant\" or \"linear\")"));
    };
    let spacing = match o.get("spacing") {
        None => Spacing::Free,
        Some(v) if v.to_string() == "free" => Spacing::Free,
        Some(v) if v.to_string() == "equal" => Spacing::Equal,
        Some(v) => match as_number(v) {
            Some(d) if d.is_finite() => Spacing::Fixed(d),
            _ => return fail("`spacing` must be \"free\", \"equal\" or a number"),
        },
    };
    let spec = PeakFitSpec {
        baseline,
        common_width: opt_bool(o, "common_width", false)?,
        spacing,
        ..PeakFitSpec::new(shape, starts(&xs, &ys, o)?)
    };
    let fit = stat_signal::fit_peaks(&xs, &ys, &spec).map_err(script_err)?;
    let mut m = fit_map(&fit);
    if opt_bool(o, "compare", false)? {
        let rows = compare_peak_counts(&xs, &ys, &spec, spec.starts.len()).map_err(script_err)?;
        let table: Array = rows
            .iter()
            .map(|r| {
                let mut c = Map::new();
                c.insert("peaks".into(), number(r.peaks as f64));
                c.insert("aicc".into(), f(r.fit.as_ref().map_or(f64::NAN, |f| f.aicc)));
                c.insert("delta_aicc".into(), f(r.delta_aicc));
                c.insert("f".into(), f(r.f));
                c.insert("p_value".into(), f(r.p_value));
                Dynamic::from_map(c)
            })
            .collect();
        m.insert("comparison".into(), Dynamic::from_array(table));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use crate::{RunOptions, run_script};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn run(source: &str) -> Vec<String> {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&lines);
        run_script(source, &RunOptions::default(), move |s| sink.borrow_mut().push(s.to_owned())).unwrap();
        lines.take()
    }

    /// The script functions pass their arrays and options through to
    /// `stat_signal`, whose values are checked against SciPy, pybaselines and
    /// lmfit in its own tests; here: exact cases and the plumbing.
    #[test]
    fn signal_functions() {
        let out = run(r#"
            let x = []; let y = [];
            for i in 0..41 { let t = i * 0.25; x.push(t); y.push(t * t); }
            // A quadratic passes a quadratic Savitzky–Golay filter unchanged.
            let s = smooth(y, #{ window: 7, order: 2 });
            print(round(s[20] * 1e9) / 1e9 == y[20]);
            let d = smooth(y, #{ window: 7, order: 2, derivative: 1, spacing: 0.25 });
            print(round(d[20] * 1e9) / 1e9);
            print(smooth([1.0, 5.0, (), 2.0, 8.0], #{ method: "running-median", window: 3 }));
            print(derivative(x, y)[8]);
            print(cumulative_integral([0, 1, 2], [0, 1, 4], #{ rule: "simpson" })[2] * 3.0);
            print(interpolate([0, 1, 2], [0, 10, 20], [0.5, 3]));
            print(interpolate(x, y, 1.1, #{ method: "spline" }) - 1.21 < 1e-12);
            print(noise_level([1.0, 1.0, 1.0, 1.0]));
            let b = baseline([1.0, 1.0, 1.0, 1.0, 1.0, 1.0], #{ method: "snip", half_window: 2 });
            print(b);
            // Two Gaussian bands on a sloping line.
            let gx = []; let gy = [];
            for i in 0..200 {
                let t = 1000.0 + i;
                gx.push(t);
                gy.push(2.0 + 0.01 * (t - 1000.0) + 40.0 * exp(-4.0 * ln(2.0) * ((t - 1080.0) / 12.0) ** 2)
                    + 25.0 * exp(-4.0 * ln(2.0) * ((t - 1110.0) / 10.0) ** 2));
            }
            let p = find_peaks(gy, #{ min_prominence: 5.0 });
            print(p.len()); print(p[0].index); print(p[1].index);
            let f = fit_peaks(gx, gy, #{ peaks: 2, compare: true });
            print(f.converged);
            print(round(f.peaks[0].centre * 1e6) / 1e6);
            print(round(f.peaks[1].fwhm * 1e6) / 1e6);
            print(round(f.baseline[1] * 1e6) / 1e6);
            print(f.comparison.len());
            let g = fit_peaks(gx, gy, #{ centres: [1078.0, 1112.0], shape: "lorentzian", baseline: "constant" });
            print(g.shape); print(g.peaks.len());
        "#);
        assert_eq!(
            out,
            [
                "true",
                "10.0",
                "[1.0, 2.0, (), 5.0, 8.0]",
                "4.0",
                "8.0",
                "[5.0, ()]",
                "true",
                "0.0",
                "[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]",
                "2",
                "80",
                "110",
                "true",
                "1080.0",
                "10.0",
                "0.01",
                "2",
                "lorentzian",
                "2",
            ]
        );
    }
}
