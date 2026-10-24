//! Nonlinear regression checked against independent SciPy fits
//! (`tests/data/nonlinear_reference.py`: `scipy.optimize.least_squares` at
//! machine-precision tolerances, standard errors from `(JᵀJ)⁻¹`, profile t
//! intervals by `brentq` on constrained refits, delta-method bands), and
//! against the published Puromycin estimates of Bates & Watts (1988).
//!
//! The Rust fits start from the library's automatic starting values; the
//! reference fits start near the generating parameters.

use stat_core::distributions::{ContinuousDistribution, StudentsT};
use stat_core::optimize::BoundState;
use stat_core::{Matrix, StatError};
use stat_data::{JsonValue, parse_json};
use stat_linear::nonlinear::forms::{CurveForm, FormModel};
use stat_linear::nonlinear::{ModelFunction, NonlinearFit, NonlinearOptions, fit_nonlinear};

fn cases() -> Vec<JsonValue> {
    let v = parse_json(include_str!("data/nonlinear_reference.json")).unwrap();
    v.get("cases").unwrap().as_array().unwrap().to_vec()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-300), "{what}: {a} vs {b} (rel {:e})", ((a - b) / b).abs());
}

/// The same form without its analytic derivatives (central differences).
struct Numeric<'a>(FormModel<'a>);

impl ModelFunction for Numeric<'_> {
    fn n_observations(&self) -> usize {
        self.0.n_observations()
    }
    fn values(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        self.0.values(theta, out)
    }
}

fn run(case: &JsonValue, analytic: bool) -> NonlinearFit {
    let form = CurveForm::from_key(case.get("form").unwrap().as_str().unwrap()).unwrap();
    let x = nums(case.get("x").unwrap());
    let y = nums(case.get("y").unwrap());
    let mut options = NonlinearOptions::new(form.parameter_names().iter().map(|s| (*s).to_owned()).collect());
    let p = options.names.len();
    if let Some(lower) = case.get("lower") {
        options.lower = nums(lower).into_iter().map(|v| if v.is_nan() { f64::NEG_INFINITY } else { v }).collect();
        options.upper =
            nums(case.get("upper").unwrap()).into_iter().map(|v| if v.is_nan() { f64::INFINITY } else { v }).collect();
    }
    let mut start = form.starting_values(&x, &y).unwrap();
    for j in 0..start.len() {
        if !options.lower.is_empty() {
            start[j] = start[j].clamp(options.lower[j], options.upper[j]);
        }
    }
    assert_eq!(start.len(), p);
    let model = FormModel { form, x: &x };
    if analytic {
        fit_nonlinear(&mut model.clone(), &y, &start, &options).unwrap()
    } else {
        fit_nonlinear(&mut Numeric(model), &y, &start, &options).unwrap()
    }
}

#[test]
fn library_forms_match_scipy() {
    for case in cases() {
        let name = case.get("name").unwrap().as_str().unwrap().to_owned();
        for analytic in [true, false] {
            let fit = run(&case, analytic);
            let what = |s: &str| format!("{name} ({}) {s}", if analytic { "analytic" } else { "numeric" });
            assert!(fit.converged(), "{}: {:?}", what("convergence"), fit.termination);
            assert_eq!(fit.analytic_derivatives, analytic);
            let est = nums(case.get("estimates").unwrap());
            let se = nums(case.get("se").unwrap());
            for (j, par) in fit.parameters.iter().enumerate() {
                close(par.estimate, est[j], 1e-7, &what(&format!("estimate {}", par.name)));
                if se[j].is_nan() {
                    assert!(par.std_error.is_nan(), "{}", what("bound se"));
                    assert_ne!(par.bound, BoundState::Free);
                } else {
                    // Central differences carry O(h²) ≈ 10⁻¹¹ relative error
                    // into V; the standard errors keep about 7 digits.
                    close(par.std_error, se[j], if analytic { 1e-7 } else { 1e-5 }, &what("std error"));
                }
            }
            close(fit.sse, case.get("sse").unwrap().as_f64().unwrap(), 1e-10, &what("sse"));
            assert_eq!(fit.df, case.get("df").unwrap().as_usize().unwrap());
            close(fit.residual_se, case.get("residual_se").unwrap().as_f64().unwrap(), 1e-10, &what("s"));
            let corr = case.get("correlation").unwrap().as_array().unwrap();
            for (i, row) in corr.iter().enumerate() {
                for (j, c) in nums(row).into_iter().enumerate() {
                    if c.is_nan() {
                        assert!(fit.correlation[(i, j)].is_nan());
                    } else {
                        assert!((fit.correlation[(i, j)] - c).abs() < 1e-6, "{}", what("correlation"));
                    }
                }
            }
            for (j, prof) in case.get("profile").unwrap().as_array().unwrap().iter().enumerate() {
                let Some(limits) = prof.as_array() else { continue };
                let interval = fit.parameters[j].profile_interval.unwrap();
                let width = limits[1].as_f64().unwrap() - limits[0].as_f64().unwrap();
                for (got, want) in [interval.lower, interval.upper].into_iter().zip(limits) {
                    let want = want.as_f64().unwrap();
                    let got = got.unwrap_or_else(|| panic!("{}", what("open profile limit")));
                    assert!((got - want).abs() < 1e-6 * width, "{}: {got} vs {want}", what("profile limit"));
                }
            }
            // Delta-method bands at the reference points.
            let form = CurveForm::from_key(case.get("form").unwrap().as_str().unwrap()).unwrap();
            let theta = fit.estimates();
            for band in case.get("bands").unwrap().as_array().unwrap() {
                let x0 = band.get("x").unwrap().as_f64().unwrap();
                let mut g = vec![0.0; theta.len()];
                form.gradient(x0, &theta, &mut g);
                let value = form.value(x0, &theta);
                close(value, band.get("fit").unwrap().as_f64().unwrap(), 1e-8, &what("band fit"));
                let (lo, hi) = fit.mean_interval(value, &g).unwrap();
                let mean = nums(band.get("mean").unwrap());
                let (plo, phi) = fit.prediction_interval(value, &g).unwrap();
                let pred = nums(band.get("prediction").unwrap());
                let tol = 1e-6 * (mean[1] - mean[0]);
                assert!((lo - mean[0]).abs() < tol && (hi - mean[1]).abs() < tol, "{}", what("mean band"));
                assert!((plo - pred[0]).abs() < tol && (phi - pred[1]).abs() < tol, "{}", what("prediction band"));
            }
        }
    }
}

/// Bates & Watts (1988, §2.2.1) report for the treated Puromycin data
/// θ̂ = (212.7, 0.06412), standard errors (6.947, 0.008281) and s = 10.93 on
/// 10 degrees of freedom.
#[test]
fn puromycin_matches_the_published_fit() {
    let case = cases().into_iter().find(|c| c.get("name").unwrap().as_str() == Some("puromycin")).unwrap();
    let fit = run(&case, true);
    assert!((fit.parameters[0].estimate - 212.7).abs() < 0.05);
    assert!((fit.parameters[1].estimate - 0.06412).abs() < 0.000_005);
    assert!((fit.parameters[0].std_error - 6.947).abs() < 0.0005);
    assert!((fit.parameters[1].std_error - 0.008281).abs() < 0.000_000_5);
    assert!((fit.residual_se - 10.93).abs() < 0.005);
    assert_eq!(fit.df, 10);
}

#[test]
fn failure_modes() {
    let x: Vec<f64> = (0..8).map(f64::from).collect();
    let y: Vec<f64> = x.iter().map(|v| 3.0 * (-0.4 * v).exp() + 0.01 * (v * 1.7).sin()).collect();
    let names = |n: &[&str]| NonlinearOptions::new(n.iter().map(|s| (*s).to_owned()).collect());

    // A redundant parameter: a·b·exp(c·x) — the Jacobian is singular.
    struct Redundant<'a>(&'a [f64]);
    impl ModelFunction for Redundant<'_> {
        fn n_observations(&self) -> usize {
            self.0.len()
        }
        fn values(&mut self, t: &[f64], out: &mut [f64]) -> bool {
            for (o, &x) in out.iter_mut().zip(self.0) {
                *o = t[0] * t[1] * (t[2] * x).exp();
            }
            true
        }
    }
    let err = fit_nonlinear(&mut Redundant(&x), &y, &[1.0, 1.0, -0.1], &names(&["a", "b", "c"])).unwrap_err();
    assert!(matches!(err, StatError::RankDeficient { .. }), "{err:?}");

    // No convergence within one iteration: the fit is returned with a warning.
    let model = FormModel { form: CurveForm::Exponential, x: &x };
    let mut short = names(&["Scale", "Rate"]);
    short.max_iterations = 1;
    let fit = fit_nonlinear(&mut model.clone(), &y, &[1.0, 0.0], &short).unwrap();
    assert!(!fit.converged());
    assert!(fit.warnings[0].contains("did not converge"), "{:?}", fit.warnings);
    assert!(fit.parameters.iter().all(|p| p.profile_interval.is_none()));

    // Too few observations, mismatched names.
    let few = [1.0, 2.0];
    let model2 = FormModel { form: CurveForm::Exponential, x: &few };
    assert!(matches!(
        fit_nonlinear(&mut model2.clone(), &[1.0, 2.0], &[1.0, 0.1], &names(&["a", "b"])),
        Err(StatError::InsufficientData { .. })
    ));
    assert!(fit_nonlinear(&mut model.clone(), &y, &[1.0, 0.0], &names(&["a"])).is_err());

    // A parameter on its bound is reported and held there.
    let mut bounded = names(&["Scale", "Rate"]);
    bounded.lower = vec![f64::NEG_INFINITY, -0.3];
    bounded.upper = vec![f64::INFINITY, 0.0];
    let fit = fit_nonlinear(&mut model.clone(), &y, &[1.0, -0.1], &bounded).unwrap();
    assert_eq!(fit.parameters[1].bound, BoundState::Lower);
    assert_eq!(fit.parameters[1].estimate, -0.3);
    assert!(fit.parameters[1].std_error.is_nan());
    assert!(fit.warnings.iter().any(|w| w.contains("lower bound")), "{:?}", fit.warnings);
    let mut g = [0.0; 2];
    CurveForm::Exponential.gradient(1.0, &fit.estimates(), &mut g);
    assert!(fit.mean_std_error(&g).unwrap() > 0.0);

    // A parameter fixed by equal bounds is a known constant: no standard
    // error of its own, not counted in the residual df, and the fit is the
    // closed-form one-parameter least-squares fit with v = exp(−0.35 x):
    // Scale = vᵀy / vᵀv, s² = SSE/(n − 1), se = s·(vᵀv)^{-1/2}, t on n − 1 df.
    let mut fixed = names(&["Scale", "Rate"]);
    fixed.lower = vec![f64::NEG_INFINITY, -0.35];
    fixed.upper = vec![f64::INFINITY, -0.35];
    let fit = fit_nonlinear(&mut model.clone(), &y, &[1.0, -0.35], &fixed).unwrap();
    assert!(fit.converged(), "{:?}", fit.termination);
    assert_eq!(fit.parameters[1].estimate, -0.35);
    assert_ne!(fit.parameters[1].bound, BoundState::Free);
    assert!(fit.parameters[1].std_error.is_nan());
    let v: Vec<f64> = x.iter().map(|&x| (-0.35 * x).exp()).collect();
    let vtv: f64 = v.iter().map(|e| e * e).sum();
    let scale = v.iter().zip(&y).map(|(e, y)| e * y).sum::<f64>() / vtv;
    close(fit.parameters[0].estimate, scale, 1e-10, "fixed-rate scale");
    let sse: f64 = v.iter().zip(&y).map(|(e, y)| (y - scale * e).powi(2)).sum();
    assert_eq!((fit.n_estimated, fit.df), (1, x.len() - 1));
    assert!(fit.parameters[1].fixed && !fit.parameters[0].fixed);
    close(fit.sse, sse, 1e-9, "fixed-rate SSE");
    let s = (sse / (x.len() - 1) as f64).sqrt();
    close(fit.residual_se, s, 1e-9, "fixed-rate residual SE");
    let want_se = s / vtv.sqrt();
    close(fit.parameters[0].std_error, want_se, 1e-8, "fixed-rate scale se");
    let t7 = StudentsT::new((x.len() - 1) as f64).unwrap();
    close(fit.t_critical, t7.quantile(0.975).unwrap(), 1e-12, "t critical on n − 1 df");
    close(fit.parameters[0].wald_interval.1, scale + fit.t_critical * want_se, 1e-8, "Wald upper");
    close(fit.parameters[0].p_value, t7.two_sided_p_value(scale / want_se).unwrap(), 1e-6, "p-value");
    assert!(fit.warnings.iter().any(|w| w.contains("fixed at")), "{:?}", fit.warnings);
    // Only estimated parameters need observations: two points fit one free
    // parameter when the other is fixed.
    let x2 = [0.0, 1.0];
    let y2 = [3.0, 2.2];
    let model2 = FormModel { form: CurveForm::Exponential, x: &x2 };
    let fit = fit_nonlinear(&mut model2.clone(), &y2, &[1.0, -0.35], &fixed).unwrap();
    assert_eq!(fit.df, 1);
}

#[test]
fn starting_values_and_formulas() {
    // Exact data: the starting grid lands close and the fit recovers the truth.
    let x: Vec<f64> = (1..=12).map(|v| f64::from(v) * 0.5).collect();
    let truths: [(CurveForm, &[f64]); 8] = [
        (CurveForm::Exponential, &[4.0, -0.5]),
        (CurveForm::ExponentialAsymptote, &[10.0, -6.0, -0.7]),
        (CurveForm::Logistic3, &[30.0, 1.5, 3.0]),
        (CurveForm::Logistic4, &[2.0, 30.0, 1.5, 3.0]),
        (CurveForm::Gompertz, &[30.0, 1.2, 2.5]),
        (CurveForm::MichaelisMenten, &[50.0, 1.5]),
        (CurveForm::Power, &[2.0, 1.4]),
        (CurveForm::Rational, &[1.0, 2.0, 0.3]),
    ];
    for (form, truth) in truths {
        let y: Vec<f64> = x.iter().map(|&v| form.value(v, truth)).collect();
        let start = form.starting_values(&x, &y).unwrap();
        let mut options = NonlinearOptions::new(form.parameter_names().iter().map(|s| (*s).to_owned()).collect());
        options.profile_intervals = false;
        // A tiny perturbation keeps a positive residual variance.
        let y: Vec<f64> = y.iter().enumerate().map(|(i, v)| v + 1e-6 * if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let fit = fit_nonlinear(&mut FormModel { form, x: &x }, &y, &start, &options).unwrap();
        assert!(fit.converged(), "{form:?}");
        for (est, want) in fit.estimates().iter().zip(truth) {
            assert!((est - want).abs() < 1e-4 * (1.0 + want.abs()), "{form:?}: {est} vs {want}");
        }
        // The analytic gradient agrees with central differences.
        let mut g = vec![0.0; truth.len()];
        form.gradient(1.3, truth, &mut g);
        let ng = stat_linear::nonlinear::numeric_gradient(&mut |t: &[f64]| Some(form.value(1.3, t)), truth).unwrap();
        for (a, b) in g.iter().zip(&ng) {
            assert!((a - b).abs() < 1e-7 * (1.0 + a.abs()), "{form:?} gradient");
        }
        // The formula text names every parameter.
        let names: Vec<String> = form.parameter_names().iter().map(|s| (*s).to_owned()).collect();
        let text = form.formula("x", &names);
        assert!(names.iter().all(|n| text.contains(n.as_str())), "{text}");
        assert_eq!(CurveForm::from_key(form.key()), Some(form));
    }
    assert!(CurveForm::Power.starting_values(&[-1.0, 1.0, 2.0], &[1.0, 2.0, 3.0]).is_err());
    assert!(CurveForm::Logistic4.starting_values(&[1.0, 1.0, 2.0, 2.0], &[1.0, 2.0, 3.0, 4.0]).is_err());
    let _ = Matrix::zeros(1, 1);
}
