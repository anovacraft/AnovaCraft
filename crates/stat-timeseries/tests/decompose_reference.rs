//! Autocorrelation, differencing, Box–Cox λ and seasonal decomposition
//! compared with R 4.3 (`stats` and `forecast`) used as black-box reference
//! software: `tests/data/decompose_reference.R` writes
//! `decompose_reference.json`. The series are R's `datasets`.

use stat_data::{JsonValue, parse_json};
use stat_timeseries::autocorrelation::{
    PortmanteauKind, acf, cross_correlation, pacf, portmanteau, prewhitened_cross_correlation, yule_walker,
};
use stat_timeseries::decompose::{
    DecompositionType, StlFit, StlOptions, classical_decomposition, seasonal_strength, stl_fit,
};
use stat_timeseries::transform::{Differencing, guerrero_lambda};

fn reference() -> JsonValue {
    parse_json(include_str!("data/decompose_reference.json")).unwrap()
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn field(v: &JsonValue, key: &str) -> f64 {
    num(v.get(key).unwrap())
}

fn list<'a>(v: &'a JsonValue, key: &str) -> &'a [JsonValue] {
    v.get(key).unwrap().as_array().unwrap()
}

fn series(all: &JsonValue, case: &JsonValue) -> Vec<f64> {
    let name = case.get("series").unwrap().as_str().unwrap();
    let y = nums(all.get("series").unwrap().get(name).unwrap());
    if case.get("log").and_then(JsonValue::as_bool).unwrap_or(false) { y.iter().map(|v| v.ln()).collect() } else { y }
}

/// Absolute error relative to the scale of the reference vector (so values
/// crossing zero are judged against the component's size).
fn close_all(got: &[f64], want: &[f64], tol: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    let scale = want.iter().filter(|v| v.is_finite()).fold(0.0f64, |a, v| a.max(v.abs())).max(1e-300);
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        if w.is_nan() {
            assert!(g.is_nan(), "{what}[{i}]: got {g}, reference NA");
        } else {
            assert!((g - w).abs() <= tol * scale, "{what}[{i}]: got {g}, reference {w} (scale {scale})");
        }
    }
}

fn close(got: f64, want: f64, tol: f64, what: &str) {
    assert!((got - want).abs() <= tol * want.abs().max(1e-12), "{what}: got {got}, reference {want}");
}

#[test]
fn acf_and_pacf_match_r() {
    let all = reference();
    for case in list(&all, "acf") {
        let y = series(&all, case);
        let k = field(case, "max_lag") as usize;
        let name = case.get("series").unwrap().as_str().unwrap();
        // Default lag 10·log10(n) agrees with R's acf default.
        let r = acf(&y, None).unwrap();
        assert_eq!(r.max_lag(), k, "{name} default lag");
        close_all(&r.values, &nums(case.get("acf").unwrap()), 1e-12, &format!("{name} acf"));
        let p = pacf(&y, None).unwrap();
        close_all(&p.values, &nums(case.get("pacf").unwrap()), 1e-11, &format!("{name} pacf"));
    }
}

#[test]
fn portmanteau_tests_match_r_box_test() {
    let all = reference();
    for case in list(&all, "box") {
        let y = series(&all, case);
        let kind = match case.get("type").unwrap().as_str().unwrap() {
            "Ljung-Box" => PortmanteauKind::LjungBox,
            _ => PortmanteauKind::BoxPierce,
        };
        let fitdf = field(case, "fitdf") as usize;
        let tests = list(case, "tests");
        let lags: Vec<usize> = tests.iter().map(|t| field(t, "lag") as usize).collect();
        let got = portmanteau(&y, &lags, fitdf, kind).unwrap();
        for (g, t) in got.iter().zip(tests) {
            close(g.statistic, field(t, "statistic"), 1e-12, "Q");
            assert_eq!(g.df as f64, field(t, "df"));
            close(g.p_value, field(t, "p_value"), 1e-9, "p-value");
        }
    }
}

#[test]
fn yule_walker_matches_r_ar_yw() {
    let all = reference();
    for case in list(&all, "ar") {
        let y = series(&all, case);
        let fixed = case.get("fixed").unwrap().as_bool().unwrap();
        let order = field(case, "order") as usize;
        let fit = yule_walker(&y, fixed.then_some(order), None).unwrap();
        assert_eq!(fit.order, order);
        close_all(&fit.coefficients, &nums(case.get("ar").unwrap()), 1e-11, "ar coefficients");
        close(fit.mean, field(case, "mean"), 1e-14, "mean");
        close(fit.innovation_variance, field(case, "var_pred"), 1e-11, "var.pred");
        if !fixed {
            close_all(&fit.aic, &nums(case.get("aic").unwrap()), 1e-10, "aic");
        }
    }
}

#[test]
fn cross_correlation_matches_r_ccf() {
    let all = reference();
    let s = all.get("series").unwrap();
    let c = all.get("ccf").unwrap();
    let x = nums(s.get("mdeaths").unwrap());
    let y = nums(s.get("fdeaths").unwrap());
    let got = cross_correlation(&x, &y, None).unwrap();
    let lags: Vec<f64> = got.lags.iter().map(|&l| l as f64).collect();
    assert_eq!(lags, nums(c.get("lags").unwrap()));
    close_all(&got.values, &nums(c.get("values").unwrap()), 1e-12, "ccf");

    let p = all.get("ccf_prewhitened").unwrap();
    let x = nums(s.get("LakeHuron").unwrap());
    let y: Vec<f64> = nums(s.get("lynx").unwrap())[..x.len()].iter().map(|v| v.ln()).collect();
    let got = prewhitened_cross_correlation(&x, &y, Some(12), None).unwrap();
    assert_eq!(got.filter.order, field(p, "order") as usize);
    close_all(&got.ccf.values, &nums(p.get("values").unwrap()), 1e-10, "prewhitened ccf");
}

#[test]
fn differencing_matches_r_diff() {
    let all = reference();
    let y = nums(all.get("series").unwrap().get("AirPassengers").unwrap());
    for case in list(&all, "diff") {
        let op =
            Differencing::new(field(case, "d") as usize, field(case, "D") as usize, field(case, "m") as usize).unwrap();
        let w = op.apply(&y).unwrap();
        assert_eq!(w, nums(case.get("values").unwrap()));
        let back = op.integrate(&y[..op.lost()], &w).unwrap();
        assert_eq!(back, y[op.lost()..].to_vec());
    }
}

#[test]
fn guerrero_lambda_matches_forecast_boxcox_lambda() {
    let all = reference();
    for case in list(&all, "guerrero") {
        let y = series(&all, case);
        let period = field(case, "period") as usize;
        let got = guerrero_lambda(&y, period, -1.0, 2.0).unwrap();
        // R's optimize() stops at a tolerance of about 1.2e-4, so agreement is
        // to that tolerance, not to the precision of our golden-section search.
        let want = field(case, "lambda");
        assert!(
            (got - want).abs() < 2e-4,
            "{}: got {got}, reference {want}",
            case.get("series").unwrap().as_str().unwrap()
        );
    }
}

#[test]
fn classical_decomposition_matches_r_decompose() {
    let all = reference();
    for case in list(&all, "classical") {
        let y = series(&all, case);
        let period = field(case, "period") as usize;
        let kind = match case.get("type").unwrap().as_str().unwrap() {
            "additive" => DecompositionType::Additive,
            _ => DecompositionType::Multiplicative,
        };
        let d = classical_decomposition(&y, period, kind).unwrap();
        close_all(&d.components.trend, &nums(case.get("trend").unwrap()), 1e-13, "trend");
        close_all(&d.components.seasonal, &nums(case.get("seasonal").unwrap()), 1e-12, "seasonal");
        close_all(&d.components.remainder, &nums(case.get("random").unwrap()), 1e-11, "random");
        close_all(&d.indices, &nums(case.get("figure").unwrap()), 1e-12, "figure");
    }
}

fn stl_options(case: &JsonValue) -> StlOptions {
    let opt = |key: &str| case.get(key).and_then(JsonValue::as_f64).map(|v| v as usize);
    StlOptions {
        seasonal_window: opt("s_window"),
        trend_window: opt("t_window"),
        robust: case.get("robust").unwrap().as_bool().unwrap(),
        inner_iterations: opt("inner"),
        outer_iterations: opt("outer"),
        ..StlOptions::new(field(case, "period") as usize)
    }
}

fn describe(case: &JsonValue) -> String {
    format!(
        "{} s.window={:?} robust={} outer={:?}",
        case.get("series").unwrap().as_str().unwrap(),
        case.get("s_window").and_then(JsonValue::as_f64),
        case.get("robust").unwrap().as_bool().unwrap(),
        case.get("outer").and_then(JsonValue::as_f64),
    )
}

/// Seasonal and remainder within 1e-6 of the component's scale, trend within
/// 1e-6 relative at every point, robustness weights within 1e-5 (the reference
/// programs treat scaled residuals within 0.1% of 0 or 1 as exactly 0 or 1).
fn compare_stl(fit: &StlFit, case: &JsonValue, what: &str) {
    let d = &fit.decomposition;
    close_all(&d.seasonal, &nums(case.get("seasonal").unwrap()), 1e-6, &format!("{what} seasonal"));
    for (t, (g, w)) in d.trend.iter().zip(nums(case.get("trend").unwrap())).enumerate() {
        close(*g, w, 1e-6, &format!("{what} trend[{t}]"));
    }
    close_all(&d.remainder, &nums(case.get("remainder").unwrap()), 1e-6, &format!("{what} remainder"));
    for (t, (g, w)) in fit.weights.iter().zip(nums(case.get("weights").unwrap())).enumerate() {
        assert!((g - w).abs() < 1e-5, "{what} weights[{t}]: got {g}, reference {w}");
    }
}

/// Non-robust STL, and robust STL with one or two robustness iterations,
/// against R's `stl()` (periodic and numeric `s.window`).
#[test]
fn stl_matches_r_stl() {
    let all = reference();
    for case in list(&all, "stl") {
        let y = series(&all, case);
        let what = describe(case);
        let fit = stl_fit(&y, &stl_options(case)).unwrap();
        let windows = nums(case.get("windows").unwrap());
        assert_eq!(fit.trend_window as f64, windows[1], "{what} t.window");
        assert_eq!(fit.lowpass_window as f64, windows[2], "{what} l.window");
        compare_stl(&fit, case, &what);
        close(seasonal_strength(&fit.decomposition), field(case, "strength"), 1e-6, &format!("{what} strength"));
    }
}

/// Robust STL with the default fifteen robustness iterations against
/// statsmodels' `STL` (`stl_robust_reference.py`). R's `stl()` is not used
/// here: in some iterations its robustness scale is not 6 · median|R| (the
/// 4th iteration on co2 uses the 246th of 468 ordered |R|), so its robust
/// fits drift from the published algorithm after the second iteration.
#[test]
fn robust_stl_matches_statsmodels() {
    let all = reference();
    let robust = parse_json(include_str!("data/stl_robust_reference.json")).unwrap();
    let s = all.get("series").unwrap();
    for case in list(&robust, "stl") {
        let name = case.get("series").unwrap().as_str().unwrap();
        let y = match name.strip_suffix("_log") {
            Some(base) => nums(s.get(base).unwrap()).iter().map(|v| v.ln()).collect(),
            None => nums(s.get(name).unwrap()),
        };
        let what = describe(case);
        let fit = stl_fit(&y, &stl_options(case)).unwrap();
        compare_stl(&fit, case, &what);
    }
}

/// Series whose length is not a multiple of the period: like R, the
/// incomplete subseries at the start is dropped
/// (`tests/data/guerrero_partial_reference.R`, forecast 8.21.1).
#[test]
fn guerrero_lambda_drops_the_leading_partial_cycle() {
    let all = reference();
    let ap = nums(all.get("series").unwrap().get("AirPassengers").unwrap());
    let gas = nums(all.get("series").unwrap().get("UKgas").unwrap());
    for (y, period, want) in [
        (&ap[3..], 12, -0.273_287_953_835_529_1),
        (&ap[..137], 12, -0.253_680_681_429_767_85),
        (&gas[2..], 4, -0.445_813_378_917_138_8),
    ] {
        let got = guerrero_lambda(y, period, -1.0, 2.0).unwrap();
        assert!((got - want).abs() < 2e-4, "n = {}: got {got}, reference {want}", y.len());
    }
}
