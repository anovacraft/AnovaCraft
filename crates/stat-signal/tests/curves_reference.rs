//! Smoothing, calculus, baselines, peak finding, the Voigt profile and peak
//! fits compared with SciPy, NumPy, pandas, pybaselines and lmfit, used as
//! black boxes; see `tests/data/curves_reference_cases.py`, which generates the data
//! with a fixed seed and writes `curves_reference_cases.json`.
//!
//! Conventions (converted in the script, explained there): lmfit
//! parameterizes peaks by area and half widths; heights, FWHMs and areas and
//! their delta-method standard errors are computed in the script from
//! lmfit's estimates and covariance matrix.

use stat_data::{JsonValue, parse_json};
use stat_signal::smooth::{MovingAverage, Smoother};
use stat_signal::{
    BaselineTerm, Integration, Interpolation, PeakFitSpec, PeakSearch, PeakShape, PeakStart, Spacing, arpls, asls,
    compare_peak_counts, cumulative_integral, faddeeva_real, find_peaks, fit_peaks, gradient, resample,
    savitzky_golay, savitzky_golay_coefficients, smooth, snip, voigt_profile,
};

fn data() -> JsonValue {
    parse_json(include_str!("data/curves_reference_cases.json")).unwrap()
}

fn key<'a>(v: &'a JsonValue, k: &str) -> &'a JsonValue {
    v.get(k).unwrap_or_else(|| panic!("missing {k}"))
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn field(v: &JsonValue, k: &str) -> f64 {
    num(key(v, k))
}

fn vec_of(v: &JsonValue, k: &str) -> Vec<f64> {
    key(v, k).as_array().unwrap().iter().map(num).collect()
}

fn indices(v: &JsonValue, k: &str) -> Vec<usize> {
    key(v, k).as_array().unwrap().iter().map(|e| e.as_usize().unwrap()).collect()
}

/// Largest absolute difference relative to `scale`.
fn close(ours: &[f64], theirs: &[f64], tol: f64, what: &str) {
    assert_eq!(ours.len(), theirs.len(), "{what}: lengths");
    let scale = theirs.iter().fold(1.0_f64, |m, v| m.max(v.abs()));
    for (i, (a, b)) in ours.iter().zip(theirs).enumerate() {
        assert!((a - b).abs() <= tol * scale, "{what}[{i}]: {a} vs {b}");
    }
}

#[test]
fn savitzky_golay_matches_scipy() {
    let d = data();
    let s = key(&d, "smoothing");
    let y = vec_of(s, "y");
    for case in key(s, "savgol").as_array().unwrap() {
        let (w, k, der) = (field(case, "window") as usize, field(case, "order") as usize, field(case, "derivative") as usize);
        let delta = field(case, "delta");
        let what = format!("savgol w={w} k={k} d={der}");
        close(&savitzky_golay_coefficients(w, k, der, delta).unwrap(), &vec_of(case, "coefficients"), 1e-9, &what);
        close(&savitzky_golay(&y, w, k, der, delta).unwrap(), &vec_of(case, "values"), 1e-9, &what);
    }
}

#[test]
fn filters_match_scipy_and_pandas() {
    let d = data();
    let s = key(&d, "smoothing");
    let y = vec_of(s, "y");
    let f = key(s, "filters");
    let cases = [
        ("centred_mean_7", Smoother::MovingAverage { window: 7, kind: MovingAverage::Centred }),
        ("median_9", Smoother::RunningMedian { window: 9 }),
        ("percentile_9_25", Smoother::RunningPercentile { window: 9, percentile: 25.0 }),
        ("percentile_9_90", Smoother::RunningPercentile { window: 9, percentile: 90.0 }),
        ("gaussian_2_5", Smoother::Gaussian { sigma: 2.5 }),
        ("gaussian_0_8", Smoother::Gaussian { sigma: 0.8 }),
        ("trailing_mean_6", Smoother::MovingAverage { window: 6, kind: MovingAverage::Trailing }),
        ("exponential_0_3", Smoother::Exponential { alpha: 0.3 }),
    ];
    for (name, smoother) in cases {
        close(&smooth(&y, &smoother, 1.0).unwrap(), &vec_of(f, name), 1e-12, name);
    }
}

#[test]
fn calculus_matches_numpy_and_scipy() {
    let d = data();
    let c = key(&d, "calculus");
    let (x, y, grid) = (vec_of(c, "x"), vec_of(c, "y"), vec_of(c, "grid"));
    close(&gradient(&x, &y).unwrap(), &vec_of(c, "gradient"), 1e-12, "gradient");
    close(&cumulative_integral(&x, &y, Integration::Trapezoid).unwrap(), &vec_of(c, "trapezoid"), 1e-12, "trapezoid");
    close(&cumulative_integral(&x, &y, Integration::Simpson).unwrap(), &vec_of(c, "simpson"), 1e-12, "simpson");
    let s = key(&d, "smoothing");
    close(
        &cumulative_integral(&vec_of(s, "t"), &vec_of(s, "y"), Integration::Simpson).unwrap(),
        &vec_of(c, "simpson_even"),
        1e-12,
        "simpson (even spacing)",
    );
    for (name, method) in [
        ("linear", Interpolation::Linear),
        ("spline", Interpolation::CubicSpline),
        ("natural", Interpolation::NaturalSpline),
        ("pchip", Interpolation::Pchip),
    ] {
        close(&resample(&x, &y, &grid, method).unwrap(), &vec_of(c, name), 1e-12, name);
    }
}

#[test]
fn baselines_match_pybaselines() {
    let d = data();
    let b = key(&d, "baseline");
    let y = vec_of(b, "y");
    let counts = vec_of(b, "counts");
    let a = key(b, "asls");
    close(&asls(&y, field(a, "lambda"), field(a, "p")).unwrap(), &vec_of(a, "values"), 1e-8, "asls");
    let r = key(b, "arpls");
    close(&arpls(&y, field(r, "lambda")).unwrap(), &vec_of(r, "values"), 1e-8, "arpls");
    let sn = key(b, "snip");
    let hw = field(sn, "half_window") as usize;
    close(&snip(&y, hw, false, false).unwrap(), &vec_of(sn, "values"), 1e-12, "snip");
    close(&snip(&y, hw, true, false).unwrap(), &vec_of(key(b, "snip_decreasing"), "values"), 1e-12, "snip decreasing");
    close(&snip(&counts, hw, false, true).unwrap(), &vec_of(key(b, "snip_lls"), "values"), 1e-10, "snip lls");
}

#[test]
fn peaks_match_scipy() {
    let d = data();
    let p = key(&d, "peaks");
    let y = vec_of(p, "y");
    for case in key(p, "cases").as_array().unwrap() {
        let opt = |k: &str| key(case, k).as_f64();
        let search = PeakSearch {
            min_height: opt("min_height").unwrap_or(f64::NEG_INFINITY),
            min_prominence: opt("min_prominence").unwrap_or(0.0),
            min_distance: opt("min_distance").map_or(1, |v| v as usize),
        };
        let found = find_peaks(&y, &search).unwrap();
        let what = format!("{search:?}");
        assert_eq!(found.iter().map(|q| q.index).collect::<Vec<_>>(), indices(case, "index"), "{what}");
        assert_eq!(found.iter().map(|q| q.left_base).collect::<Vec<_>>(), indices(case, "left_base"), "{what}");
        assert_eq!(found.iter().map(|q| q.right_base).collect::<Vec<_>>(), indices(case, "right_base"), "{what}");
        close(&found.iter().map(|q| q.prominence).collect::<Vec<_>>(), &vec_of(case, "prominence"), 1e-12, &what);
        close(&found.iter().map(|q| q.left_crossing).collect::<Vec<_>>(), &vec_of(case, "left_crossing"), 1e-12, &what);
        close(
            &found.iter().map(|q| q.right_crossing).collect::<Vec<_>>(),
            &vec_of(case, "right_crossing"),
            1e-12,
            &what,
        );
    }
}

#[test]
fn faddeeva_and_voigt_match_scipy() {
    let d = data();
    for case in key(&d, "faddeeva").as_array().unwrap() {
        let (x, y, re) = (field(case, "x"), field(case, "y"), field(case, "re"));
        let ours = faddeeva_real(x, y);
        // Weideman's expansion is accurate in absolute terms (|w| ≤ 1).
        assert!((ours - re).abs() <= 1e-13, "w({x} + {y}i): {ours} vs {re}");
    }
    for case in key(&d, "voigt").as_array().unwrap() {
        let v = voigt_profile(field(case, "x"), field(case, "sigma"), field(case, "gamma"));
        let r = field(case, "value");
        assert!((v - r).abs() <= 1e-13 * voigt_profile(0.0, field(case, "sigma"), field(case, "gamma")), "{v} vs {r}");
    }
}

fn starts(v: &JsonValue) -> Vec<PeakStart> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| PeakStart { centre: field(s, "centre"), height: field(s, "height"), fwhm: field(s, "fwhm") })
        .collect()
}

fn pair(v: &JsonValue, k: &str) -> (f64, f64) {
    let p = vec_of(v, k);
    (p[0], p[1])
}

#[test]
fn peak_fits_match_lmfit() {
    let d = data();
    for case in key(&d, "fits").as_array().unwrap() {
        let name = key(case, "name").as_str().unwrap();
        let shape = PeakShape::from_key(key(case, "shape").as_str().unwrap()).unwrap();
        let spacing = match key(case, "spacing") {
            JsonValue::Null => Spacing::Free,
            JsonValue::String(s) if s == "equal" => Spacing::Equal,
            other => Spacing::Fixed(other.as_f64().unwrap()),
        };
        let spec = PeakFitSpec {
            baseline: BaselineTerm::Linear,
            common_width: key(case, "common_width").as_bool().unwrap(),
            spacing,
            ..PeakFitSpec::new(shape, starts(key(case, "starts")))
        };
        let fit = fit_peaks(&vec_of(case, "x"), &vec_of(case, "y"), &spec).unwrap();
        assert!(fit.converged(), "{name}: {:?}", fit.termination);
        assert_eq!(fit.n_free, field(case, "n_free") as usize, "{name}");
        let sse = field(case, "sse");
        assert!((fit.sse - sse).abs() <= 1e-8 * sse, "{name}: SSE {} vs {sse}", fit.sse);
        for (k, (ours, theirs)) in fit.peaks.iter().zip(key(case, "peaks").as_array().unwrap()).enumerate() {
            for (what, mine) in [("height", ours.height), ("centre", ours.centre), ("fwhm", ours.fwhm), ("area", ours.area)] {
                let (est, se) = pair(theirs, what);
                assert!((mine.0 - est).abs() <= 1e-6 * est.abs().max(1.0), "{name} peak {k} {what}: {} vs {est}", mine.0);
                assert!((mine.1 - se).abs() <= 2e-3 * se, "{name} peak {k} {what} SE: {} vs {se}", mine.1);
            }
        }
        for (i, what) in ["intercept", "slope"].iter().enumerate() {
            let (est, se) = pair(case, what);
            assert!((fit.baseline[i].0 - est).abs() <= 1e-6 * est.abs().max(1e-3), "{name} {what}");
            assert!((fit.baseline[i].1 - se).abs() <= 2e-3 * se, "{name} {what} SE");
        }
    }
}

#[test]
fn peak_count_comparison_matches_lmfit() {
    let d = data();
    let c = key(&d, "peak_counts");
    let spec = PeakFitSpec::new(PeakShape::Gaussian, starts(key(c, "starts")));
    let rows = compare_peak_counts(&vec_of(c, "x"), &vec_of(c, "y"), &spec, 3).unwrap();
    for (ours, theirs) in rows.iter().zip(key(c, "rows").as_array().unwrap()) {
        let fit = ours.fit.as_ref().unwrap();
        let sse = field(theirs, "sse");
        assert!((fit.sse - sse).abs() <= 1e-8 * sse, "{} peaks: {} vs {sse}", ours.peaks, fit.sse);
        assert!((fit.aicc - field(theirs, "aicc")).abs() <= 1e-6, "{} peaks AICc", ours.peaks);
        if let Some(f) = theirs.get("f") {
            assert!((ours.f - num(f)).abs() <= 1e-6 * num(f), "{} peaks F", ours.peaks);
        }
    }
    let best = rows.iter().min_by(|a, b| a.delta_aicc.total_cmp(&b.delta_aicc)).unwrap();
    assert_eq!(best.peaks, 3);
}
