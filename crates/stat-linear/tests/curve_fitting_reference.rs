//! Weighted, global, robust and implicit (orthogonal-distance) fits, the
//! outlier rule, model comparison and Hougaard's skewness, checked against
//! independent SciPy and SymPy computations
//! (`tests/data/curve_fitting_reference.py`; values in
//! `curve_fitting_reference.json`).

use stat_data::{JsonValue, parse_json};
use stat_linear::nonlinear::compare::{FitScore, aicc_probability, extra_sum_of_squares};
use stat_linear::nonlinear::forms::{CurveForm, FormModel};
use stat_linear::nonlinear::global::{GlobalModel, layout};
use stat_linear::nonlinear::implicit::ImplicitModel;
use stat_linear::nonlinear::robust::{fit_robust, outliers};
use stat_linear::nonlinear::{ModelFunction, NonlinearOptions, fit_nonlinear};

fn case(name: &str) -> JsonValue {
    let v = parse_json(include_str!("data/curve_fitting_reference.json")).unwrap();
    v.get(name).unwrap().clone()
}

fn nums(v: &JsonValue, key: &str) -> Vec<f64> {
    v.get(key).unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn num(v: &JsonValue, key: &str) -> f64 {
    v.get(key).unwrap().as_f64().unwrap()
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs(), "{what}: {a} vs {b} (rel {:e})", ((a - b) / b).abs());
}

fn options(names: &[&str]) -> NonlinearOptions {
    let mut o = NonlinearOptions::new(names.iter().map(|s| (*s).to_owned()).collect());
    o.profile_intervals = false;
    o.relative_offset = 1e-12;
    o
}

#[test]
fn weighted_fit_matches_scipy() {
    let c = case("weighted");
    let (x, y, w) = (nums(&c, "x"), nums(&c, "y"), nums(&c, "w"));
    let mut o = options(&["Vmax", "Km"]);
    o.weights = w;
    let fit =
        fit_nonlinear(&mut FormModel { form: CurveForm::MichaelisMenten, x: &x }, &y, &[200.0, 0.06], &o).unwrap();
    assert!(fit.weighted);
    for (j, p) in fit.parameters.iter().enumerate() {
        close(p.estimate, nums(&c, "estimates")[j], 1e-8, "estimate");
        close(p.std_error, nums(&c, "std_errors")[j], 1e-6, "std error");
    }
    close(fit.sse, num(&c, "sse"), 1e-9, "weighted SSE");
    // Residuals and fitted values are on the response scale.
    for i in 0..x.len() {
        assert!((fit.fitted[i] + fit.residuals[i] - y[i]).abs() < 1e-9);
    }
}

#[test]
fn global_fit_and_comparison_match_scipy() {
    let c = case("global");
    let (x, ya, yb) = (nums(&c, "x"), nums(&c, "ya"), nums(&c, "yb"));
    // Lower and Upper shared; Rate and Midpoint per data set.
    let (map, owner) = layout(&[true, true, false, false], 2);
    assert_eq!(owner, vec![(0, None), (1, None), (2, Some(0)), (2, Some(1)), (3, Some(0)), (3, Some(1))]);
    let parts: Vec<Box<dyn ModelFunction>> = vec![
        Box::new(FormModel { form: CurveForm::Logistic4, x: &x }),
        Box::new(FormModel { form: CurveForm::Logistic4, x: &x }),
    ];
    let mut model = GlobalModel::new(parts, map, owner.len()).unwrap();
    let y: Vec<f64> = ya.iter().chain(&yb).copied().collect();
    let start = [0.0, 100.0, 1.0, 1.0, 0.0, 1.0];
    let o = options(&["Lower", "Upper", "Rate A", "Rate B", "Mid A", "Mid B"]);
    let fit = fit_nonlinear(&mut model, &y, &start, &o).unwrap();
    assert!(fit.analytic_derivatives);
    // SciPy's order: lo, hi, rate A, mid A, rate B, mid B.
    let reference = nums(&c, "estimates");
    let ref_se = nums(&c, "std_errors");
    for (mine, theirs) in [(0, 0), (1, 1), (2, 2), (4, 3), (3, 4), (5, 5)] {
        close(fit.parameters[mine].estimate, reference[theirs], 1e-7, "global estimate");
        close(fit.parameters[mine].std_error, ref_se[theirs], 1e-5, "global std error");
    }
    close(fit.sse, num(&c, "sse"), 1e-9, "global SSE");
    // One curve for both sets: every parameter shared.
    let (map1, owner1) = layout(&[true; 4], 2);
    let parts: Vec<Box<dyn ModelFunction>> = vec![
        Box::new(FormModel { form: CurveForm::Logistic4, x: &x }),
        Box::new(FormModel { form: CurveForm::Logistic4, x: &x }),
    ];
    let mut shared = GlobalModel::new(parts, map1, owner1.len()).unwrap();
    let one =
        fit_nonlinear(&mut shared, &y, &[0.0, 100.0, 1.0, 0.0], &options(&["Lower", "Upper", "Rate", "Mid"])).unwrap();
    close(one.sse, num(&c, "shared_sse"), 1e-9, "shared SSE");
    let a = FitScore { sse: one.sse, n: one.n, parameters: one.n_estimated };
    let b = FitScore { sse: fit.sse, n: fit.n, parameters: fit.n_estimated };
    let f = extra_sum_of_squares(a, b).unwrap();
    close(f.f, num(&c, "f"), 1e-8, "F");
    close(f.p_value, num(&c, "p"), 1e-6, "p");
    close(a.aicc(), num(&c, "aicc_shared"), 1e-10, "AICc shared");
    close(b.aicc(), num(&c, "aicc_global"), 1e-10, "AICc global");
    assert!(aicc_probability(a, b) > 0.999);
}

#[test]
fn robust_fit_and_outliers_match_scipy() {
    let c = case("robust");
    let (x, y) = (nums(&c, "x"), nums(&c, "y"));
    let o = options(&["Vmax", "Km"]);
    let r = fit_robust(&mut FormModel { form: CurveForm::MichaelisMenten, x: &x }, &y, &[200.0, 0.06], &o).unwrap();
    assert!(r.converged);
    let reference = nums(&c, "estimates");
    for j in 0..2 {
        close(r.fit.parameters[j].estimate, reference[j], 1e-6, "robust estimate");
    }
    close(r.scale, num(&c, "scale"), 1e-6, "RSDR");
    // The robust residuals flag the planted outlier, as the reference does.
    let resid: Vec<f64> =
        x.iter().zip(&y).map(|(&xi, &yi)| yi - CurveForm::MichaelisMenten.value(xi, &reference)).collect();
    let found = outliers(&resid, 2, 0.01).unwrap();
    let expected: Vec<usize> =
        c.get("outliers").unwrap().as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as usize).collect();
    assert_eq!(found, expected);
}

fn implicit_fit(
    name: &str,
    f: fn(f64, f64, &[f64]) -> f64,
    start: &[f64],
    names: &[&str],
) -> stat_linear::NonlinearFit {
    let c = case(name);
    let (x, y) = (nums(&c, "x"), nums(&c, "y"));
    let mut model = ImplicitModel::new(Box::new(move |_, a, b, t: &[f64]| Some(f(a, b, t))), &x, &y);
    let zeros = vec![0.0; x.len()];
    fit_nonlinear(&mut model, &zeros, start, &options(names)).unwrap()
}

#[test]
fn circle_by_orthogonal_distance_matches_exact_geometric_fit() {
    let fit = implicit_fit(
        "circle",
        |x, y, t| (x - t[0]).powi(2) + (y - t[1]).powi(2) - t[2] * t[2],
        &[1.5, -0.5, 2.5],
        &["Cx", "Cy", "R"],
    );
    let c = case("circle");
    for j in 0..3 {
        close(fit.parameters[j].estimate.abs(), nums(&c, "estimates")[j].abs(), 1e-7, "circle");
        close(fit.parameters[j].std_error, nums(&c, "std_errors")[j], 1e-4, "circle se");
    }
    close(fit.sse, num(&c, "sse"), 1e-7, "sum of squared distances");
}

#[test]
fn ellipse_by_orthogonal_distance_matches_exact_geometric_fit() {
    let fit = implicit_fit(
        "ellipse",
        |x, y, t| {
            let (s, c) = t[4].sin_cos();
            let u = (x - t[0]) * c + (y - t[1]) * s;
            let v = -(x - t[0]) * s + (y - t[1]) * c;
            u * u / (t[2] * t[2]) + v * v / (t[3] * t[3]) - 1.0
        },
        &[0.8, -1.8, 3.8, 2.2, 0.4],
        &["h", "k", "a", "b", "angle"],
    );
    let c = case("ellipse");
    for j in 0..5 {
        close(fit.parameters[j].estimate, nums(&c, "estimates")[j], 1e-5, "ellipse");
    }
    close(fit.sse, num(&c, "sum_sq_distance"), 1e-6, "sum of squared distances");
}

#[test]
fn hougaard_skewness_matches_symbolic_derivatives() {
    let c = case("hougaard");
    let (x, y) = (nums(&c, "x"), nums(&c, "y"));
    let mut o = options(&["Vmax", "Km"]);
    o.skewness = true;
    let fit =
        fit_nonlinear(&mut FormModel { form: CurveForm::MichaelisMenten, x: &x }, &y, &[200.0, 0.06], &o).unwrap();
    let reference = nums(&c, "skewness");
    for j in 0..2 {
        close(fit.parameters[j].skewness, reference[j], 1e-5, "Hougaard skewness");
    }
    // A model linear in its parameters has no skewness.
    let fit = fit_nonlinear(&mut FormModel { form: CurveForm::StraightLine, x: &x }, &y, &[100.0, 50.0], &o).unwrap();
    for p in &fit.parameters {
        assert!(p.skewness.abs() < 1e-6, "{}", p.skewness);
    }
}
