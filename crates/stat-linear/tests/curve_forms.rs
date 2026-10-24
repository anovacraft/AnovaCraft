//! The curve-library forms added after the first eight (straight line,
//! dose–response on both scales, asymmetric logistic, Weibull and beta
//! growth, segmented line, two exponentials, Gaussian and Lorentzian peaks),
//! checked against independent SciPy fits.
//!
//! The reference values below were printed by
//! `tests/data/curve_forms_reference.py`: `scipy.optimize.curve_fit`
//! (MINPACK Levenberg–Marquardt, tolerances 1e-15) from the generating
//! parameters with an independently written analytic Jacobian, standard
//! errors `s·√diag((JᵀJ)⁻¹)` (curve_fit's `absolute_sigma=False`). The Rust
//! fits start from the library's automatic starting values.

use stat_linear::nonlinear::forms::{CurveForm, FormModel};
use stat_linear::nonlinear::{NonlinearFit, NonlinearOptions, fit_nonlinear, numeric_gradient};

struct Case {
    name: &'static str,
    key: &'static str,
    x: &'static [f64],
    y: &'static [f64],
    estimates: &'static [f64],
    se: &'static [f64],
}

/// Printed by `tests/data/curve_forms_reference.py`.
#[rustfmt::skip]
const CASES: &[Case] = &[
    // line: sse = 0.9293934029599202
    Case { name: "line", key: "line",
        x: &[0.0, 0.909091, 1.818182, 2.727273, 3.636364, 4.545455, 5.454545, 6.363636, 7.272727, 8.181818, 9.090909, 10.0],
        y: &[2.525, 3.2499, 5.0301, 6.3069, 7.0421, 8.0632, 9.3719, 11.0296, 12.0472, 13.2704, 15.0261, 15.3974],
        estimates: &[2.352348465712241, 1.3355269735242186],
        se: &[0.16554513462982273, 0.02804300024957397] },
    // dose_response_log: sse = 110.94242166842373
    Case { name: "dose_response_log", key: "dose_response_log",
        x: &[-9.0, -8.666667, -8.333333, -8.0, -7.666667, -7.333333, -7.0, -6.666667, -6.333333, -6.0, -5.666667, -5.333333, -5.0, -4.666667, -4.333333, -4.0, -3.666667, -3.333333, -3.0],
        y: &[0.232, 4.7335, -0.1159, 9.1737, 9.3679, 6.136, 11.6985, 16.4196, 34.8993, 46.8287, 64.1974, 82.336, 88.7703, 93.2348, 95.0927, 97.0247, 95.2627, 94.0185, 96.1707],
        estimates: &[3.8502695088744305, 96.45192490242917, -5.973372839369485, 1.0385959254202919],
        se: &[1.2313002974122635, 1.2509583074174484, 0.03454137813023216, 0.07647710508677472] },
    // inhibition_log: sse = 99.90294802030603
    Case { name: "inhibition_log", key: "dose_response_log",
        x: &[-9.0, -8.666667, -8.333333, -8.0, -7.666667, -7.333333, -7.0, -6.666667, -6.333333, -6.0, -5.666667, -5.333333, -5.0, -4.666667, -4.333333, -4.0, -3.666667, -3.333333, -3.0],
        y: &[97.6425, 101.208, 99.889, 101.4451, 101.4648, 98.1335, 96.1239, 93.6124, 85.8129, 80.6895, 64.2421, 50.2165, 30.1246, 22.1494, 17.253, 14.0123, 17.8248, 6.9431, 8.4886],
        estimates: &[9.99880777985021, 100.11465719174987, -5.476561655166274, -0.9599613913641208],
        se: &[1.4918634682706584, 1.0633515424452828, 0.03687440159832739, 0.07002556017481805] },
    // dose_response: sse = 7.997574459520002
    Case { name: "dose_response", key: "dose_response",
        x: &[0.0, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0],
        y: &[2.7321, 3.1926, 2.8365, 4.094, 6.109, 8.9527, 16.739, 21.8435, 32.0478, 41.8311, 43.3558, 45.063, 46.7558, 47.0177],
        estimates: &[2.8607419025145258, 47.384546759448696, 11.268614034267474, 1.1077553971362708],
        se: &[0.49326977708777114, 0.6211188029063106, 0.6535064266405183, 0.06716746861593798] },
    // logistic5: sse = 0.5549271729534382
    Case { name: "logistic5", key: "logistic5",
        x: &[-3.0, -2.75, -2.5, -2.25, -2.0, -1.75, -1.5, -1.25, -1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 2.75, 3.0],
        y: &[2.3847, 2.6773, 2.8688, 2.8244, 3.5799, 4.0799, 4.8891, 5.6662, 6.4481, 7.8664, 9.2111, 10.8408, 12.6853, 14.4108, 15.9468, 17.0341, 18.2905, 19.1226, 19.1498, 19.7647, 20.0641, 19.7944, 19.8183, 19.9179, 19.6411],
        estimates: &[1.45644800665844, 19.95119965741739, 0.41729303602272516, 0.9705241437618408, 0.39991088900628724],
        se: &[0.2601598230509115, 0.08470543337619879, 0.0808545852185707, 0.06982056566853322, 0.05905419097907414] },
    // weibull_growth: sse = 4.262072498374671
    Case { name: "weibull_growth", key: "weibull_growth",
        x: &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0],
        y: &[2.0688, 2.3089, 5.2453, 8.3017, 12.4742, 17.8624, 22.7149, 27.337, 31.0884, 33.769, 35.4365, 37.7802, 38.3598, 39.5695, 39.8792, 38.5991, 40.2827, 40.6925, 40.1592, 40.3849, 39.1518],
        estimates: &[39.991895587906235, 1.8983911479916966, 0.14843900430125764, 2.0702154310081515],
        se: &[0.19323760442818408, 0.34929233077215593, 0.0015771666220198018, 0.06185675674330046] },
    // beta_growth: sse = 7.641653408076452
    Case { name: "beta_growth", key: "beta_growth",
        x: &[0.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 44.0, 48.0, 52.0, 56.0, 60.0, 64.0, 68.0, 72.0, 76.0, 80.0, 84.0, 88.0, 92.0, 96.0, 100.0],
        y: &[-0.2387, 0.1729, 0.7438, 1.4901, 3.0225, 3.8593, 5.8377, 7.9398, 10.2644, 12.6674, 14.633, 18.1713, 20.5769, 24.9203, 26.0703, 28.4481, 28.383, 29.7732, 29.6301, 30.3896, 29.1337, 29.794, 30.7173, 29.9731, 30.4816, 28.9343],
        estimates: &[29.871825840209226, 69.38565972784359, 41.427682006016084],
        se: &[0.19256610278260292, 1.070571353011177, 0.486312924651837] },
    // segmented: sse = 2.6304839661010404
    Case { name: "segmented", key: "segmented",
        x: &[0.0, 0.909091, 1.818182, 2.727273, 3.636364, 4.545455, 5.454545, 6.363636, 7.272727, 8.181818, 9.090909, 10.0, 10.909091, 11.818182, 12.727273, 13.636364, 14.545455, 15.454545, 16.363636, 17.272727, 18.181818, 19.090909, 20.0],
        y: &[3.8232, 5.3168, 6.8771, 8.5567, 9.572, 11.0108, 12.3635, 13.1133, 14.8276, 17.2723, 17.7387, 19.6781, 21.0952, 20.4099, 20.5227, 19.4145, 18.2995, 18.7917, 18.3102, 17.3942, 16.686, 16.3219, 15.2826],
        estimates: &[3.9096799633847636, 1.557197517536899, -0.6159413797650634, 11.058983604870633],
        se: &[0.19502502667993252, 0.03033877435132554, 0.045061616372805126, 0.1463660403958607] },
    // two_exponentials: sse = 1.501681565868205
    Case { name: "two_exponentials", key: "two_exponentials",
        x: &[0.0, 0.666667, 1.333333, 2.0, 2.666667, 3.333333, 4.0, 4.666667, 5.333333, 6.0, 6.666667, 7.333333, 8.0, 8.666667, 9.333333, 10.0, 10.666667, 11.333333, 12.0, 12.666667, 13.333333, 14.0, 14.666667, 15.333333, 16.0, 16.666667, 17.333333, 18.0, 18.666667, 19.333333, 20.0],
        y: &[43.7009, 23.731, 15.9654, 12.1592, 10.6209, 9.3975, 8.4627, 7.4611, 7.4878, 7.1117, 6.1019, 5.4659, 5.5013, 5.4184, 5.2558, 4.5551, 4.4386, 4.3514, 4.3041, 3.5806, 3.7188, 3.0874, 3.2621, 2.9872, 2.5951, 2.7726, 2.7559, 2.7705, 2.6565, 2.6853, 2.8073],
        estimates: &[1.8735827263031226, 30.03443705278305, 1.4689161028925593, 11.77288847860449, 0.1456002125289676],
        se: &[0.22251044447196303, 0.5362396655371945, 0.047348274678975726, 0.3660411339580248, 0.01155811223696914] },
    // gaussian_peak: sse = 10.169125636617183
    Case { name: "gaussian_peak", key: "gaussian_peak",
        x: &[400.0, 408.0, 416.0, 424.0, 432.0, 440.0, 448.0, 456.0, 464.0, 472.0, 480.0, 488.0, 496.0, 504.0, 512.0, 520.0, 528.0, 536.0, 544.0, 552.0, 560.0, 568.0, 576.0, 584.0, 592.0, 600.0],
        y: &[2.8, 2.3869, 2.8106, 3.3983, 2.6151, 5.8542, 7.8471, 11.9243, 19.8818, 28.3985, 39.5869, 49.5143, 52.1074, 51.2367, 43.4563, 32.5779, 21.8931, 15.5308, 8.2001, 5.1272, 3.9048, 3.3263, 3.5879, 2.3846, 1.5406, 2.3369],
        estimates: &[2.6926265278816857, 50.10155745384168, 497.38309923163075, 22.564415831577687],
        se: &[0.20495279063454652, 0.3996244889823232, 0.19367065027698122, 0.23354327462401472] },
    // lorentzian_peak: sse = 2.431759624950574
    Case { name: "lorentzian_peak", key: "lorentzian_peak",
        x: &[-10.0, -9.230769, -8.461538, -7.692308, -6.923077, -6.153846, -5.384615, -4.615385, -3.846154, -3.076923, -2.307692, -1.538462, -0.769231, 0.0, 0.769231, 1.538462, 2.307692, 3.076923, 3.846154, 4.615385, 5.384615, 6.153846, 6.923077, 7.692308, 8.461538, 9.230769, 10.0],
        y: &[1.4084, 2.344, 2.2789, 2.2137, 1.8768, 1.492, 2.088, 2.6859, 3.4002, 3.2766, 4.1091, 5.638, 7.829, 11.247, 13.0929, 11.9418, 8.3879, 6.2694, 4.2593, 3.6735, 3.34, 2.7075, 1.9525, 1.6023, 1.998, 2.1957, 1.8904],
        estimates: &[1.4253638716272525, 11.887902882437272, 0.8558507634323163, 1.7718489711334116],
        se: &[0.11116577068068863, 0.24623002796809257, 0.0360519052182582, 0.06909798755180399] },
];

const NEW_FORMS: [CurveForm; 10] = [
    CurveForm::StraightLine,
    CurveForm::DoseResponseLog,
    CurveForm::DoseResponse,
    CurveForm::AsymmetricLogistic5,
    CurveForm::WeibullGrowth,
    CurveForm::BetaGrowth,
    CurveForm::Segmented,
    CurveForm::TwoPhaseExponential,
    CurveForm::GaussianPeak,
    CurveForm::LorentzianPeak,
];

fn fit(case: &Case) -> NonlinearFit {
    let form = CurveForm::from_key(case.key).unwrap();
    let start = form.starting_values(case.x, case.y).unwrap();
    let mut options = NonlinearOptions::new(form.parameter_names().iter().map(|s| (*s).to_owned()).collect());
    options.profile_intervals = false;
    fit_nonlinear(&mut FormModel { form, x: case.x }, case.y, &start, &options).unwrap()
}

/// Largest relative differences (estimates, standard errors) from the
/// reference.
fn worst(case: &Case, fit: &NonlinearFit) -> (f64, f64) {
    let rel = |a: f64, b: f64| ((a - b) / b).abs();
    let e = fit.parameters.iter().zip(case.estimates).map(|(p, &b)| rel(p.estimate, b)).fold(0.0, f64::max);
    let s = fit.parameters.iter().zip(case.se).map(|(p, &b)| rel(p.std_error, b)).fold(0.0, f64::max);
    (e, s)
}

#[test]
fn new_forms_match_scipy_from_automatic_starts() {
    for case in CASES {
        let fit = fit(case);
        assert!(fit.converged(), "{}: {:?}", case.name, fit.termination);
        assert_eq!(fit.parameters.len(), case.estimates.len());
        let (e, s) = worst(case, &fit);
        println!("{}: n = {}, estimates rel {e:.1e}, SEs rel {s:.1e}", case.name, case.x.len());
        assert!(e < 1e-6, "{}: estimates {:?} vs {:?}", case.name, fit.estimates(), case.estimates);
        assert!(s < 1e-5, "{}: SEs off by {s:e}", case.name);
    }
    // Every new form has a reference case.
    for form in NEW_FORMS {
        assert!(CASES.iter().any(|c| c.key == form.key()), "{form:?}");
    }
}

#[test]
fn straight_line_is_ordinary_least_squares() {
    let case = CASES.iter().find(|c| c.key == "line").unwrap();
    let n = case.x.len() as f64;
    let (mx, my) = (case.x.iter().sum::<f64>() / n, case.y.iter().sum::<f64>() / n);
    let sxy: f64 = case.x.iter().zip(case.y).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = case.x.iter().map(|x| (x - mx).powi(2)).sum();
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let start = CurveForm::StraightLine.starting_values(case.x, case.y).unwrap();
    let fit = fit(case);
    for (got, want) in
        [(start[0], intercept), (start[1], slope), (fit.estimates()[0], intercept), (fit.estimates()[1], slope)]
    {
        assert!((got - want).abs() <= 1e-10 * want.abs(), "{got} vs {want}");
    }
}

#[test]
fn gradients_match_central_differences() {
    let points: [(CurveForm, &[f64], &[f64]); 20] = [
        (CurveForm::StraightLine, &[2.0, -0.7], &[-1.0, 0.5, 3.0]),
        (CurveForm::StraightLine, &[-1.0, 3.0], &[0.0, 2.0]),
        (CurveForm::DoseResponseLog, &[5.0, 95.0, -6.0, 1.2], &[-8.0, -6.3, -5.0]),
        (CurveForm::DoseResponseLog, &[100.0, 3.0, -5.0, -0.8], &[-7.0, -4.5]),
        (CurveForm::DoseResponse, &[3.0, 48.0, 12.0, 1.1], &[0.0, 0.5, 12.0, 300.0]),
        (CurveForm::DoseResponse, &[90.0, 10.0, 2.0, -1.7], &[0.0, 0.3, 4.0]),
        (CurveForm::AsymmetricLogistic5, &[1.0, 20.0, 0.5, 1.0, 0.35], &[-2.0, 0.4, 2.5]),
        (CurveForm::AsymmetricLogistic5, &[30.0, 2.0, -1.0, -1.5, 2.5], &[-2.0, 0.0, 1.0]),
        (CurveForm::WeibullGrowth, &[40.0, 2.0, 0.15, 2.2], &[0.0, 3.0, 12.0]),
        (CurveForm::WeibullGrowth, &[10.0, 25.0, 0.5, 0.7], &[0.2, 4.0]),
        (CurveForm::BetaGrowth, &[30.0, 70.0, 42.0], &[0.0, 10.0, 42.0, 65.0, 90.0]),
        (CurveForm::BetaGrowth, &[5.0, 12.0, 3.0], &[1.0, 6.0, 11.5, 20.0]),
        (CurveForm::Segmented, &[4.0, 1.5, -0.6, 11.3], &[2.0, 11.0, 15.0]),
        (CurveForm::Segmented, &[-2.0, -0.5, 2.0, 0.4], &[-3.0, 1.0]),
        (CurveForm::TwoPhaseExponential, &[2.0, 30.0, 1.5, 12.0, 0.15], &[0.0, 0.7, 6.0]),
        (CurveForm::TwoPhaseExponential, &[-1.0, -4.0, 3.0, 8.0, 0.4], &[0.3, 2.0]),
        (CurveForm::GaussianPeak, &[3.0, 50.0, 497.0, 22.0], &[450.0, 497.0, 530.0]),
        (CurveForm::GaussianPeak, &[0.5, -2.0, 0.3, 1.3], &[-1.0, 2.5]),
        (CurveForm::LorentzianPeak, &[1.5, 12.0, 0.8, 1.7], &[-3.0, 0.8, 2.0]),
        (CurveForm::LorentzianPeak, &[-0.5, 4.0, -2.0, 0.4], &[-2.3, 1.0]),
    ];
    for (form, theta, xs) in points {
        for &x in xs {
            let mut g = vec![0.0; theta.len()];
            form.gradient(x, theta, &mut g);
            let ng = numeric_gradient(&mut |t: &[f64]| Some(form.value(x, t)), theta).unwrap();
            let scale = 1.0 + form.value(x, theta).abs();
            for (j, (a, b)) in g.iter().zip(&ng).enumerate() {
                assert!((a - b).abs() < 1e-6 * scale * (1.0 + a.abs()), "{form:?} x = {x} parameter {j}: {a} vs {b}");
            }
        }
    }
}

#[test]
fn limits_and_special_cases() {
    // Dose scale at zero dose: Bottom for a rising curve, Top for a falling one.
    assert_eq!(CurveForm::DoseResponse.value(0.0, &[3.0, 48.0, 12.0, 1.1]), 3.0);
    assert_eq!(CurveForm::DoseResponse.value(0.0, &[3.0, 48.0, 12.0, -1.1]), 48.0);
    // The dose scale and the log-dose scale agree at x = log10(dose).
    let a = CurveForm::DoseResponse.value(7.0, &[3.0, 48.0, 12.0, 1.1]);
    let b = CurveForm::DoseResponseLog.value(7f64.log10(), &[3.0, 48.0, 12f64.log10(), 1.1]);
    assert!((a - b).abs() < 1e-12 * a.abs());
    // Asymmetry 1 is the four-parameter curve.
    for x in [-2.0, -0.3, 0.0, 1.7] {
        let a = CurveForm::AsymmetricLogistic5.value(x, &[1.0, 20.0, 0.5, 1.3, 1.0]);
        let b = CurveForm::DoseResponseLog.value(x, &[1.0, 20.0, 0.5, 1.3]);
        assert!((a - b).abs() < 1e-12 * b.abs(), "{a} vs {b}");
    }
    // Beta growth: 0 at x = 0, Ymax at and beyond End.
    let beta = [30.0, 70.0, 42.0];
    assert_eq!(CurveForm::BetaGrowth.value(0.0, &beta), 0.0);
    assert!((CurveForm::BetaGrowth.value(70.0, &beta) - 30.0).abs() < 1e-12);
    assert_eq!(CurveForm::BetaGrowth.value(95.0, &beta), 30.0);
    // Segmented: continuous at the breakpoint.
    let seg = [4.0, 1.5, -0.6, 11.3];
    let left = CurveForm::Segmented.value(11.3 - 1e-9, &seg);
    let right = CurveForm::Segmented.value(11.3 + 1e-9, &seg);
    assert!((left - right).abs() < 1e-8);
    // Forms on x ≥ 0 reject negative predictor values with a plain message.
    for form in [CurveForm::DoseResponse, CurveForm::WeibullGrowth, CurveForm::BetaGrowth] {
        let err = form.check(&[-1.0, 1.0, 2.0, 3.0, 4.0]).unwrap_err();
        assert!(err.to_string().contains("no negative values"), "{err}");
    }
    // Too few distinct values.
    assert!(CurveForm::Segmented.check(&[1.0, 1.0, 2.0, 3.0]).is_err());
    assert!(CurveForm::TwoPhaseExponential.starting_values(&[0.0, 1.0, 2.0, 3.0, 3.0], &[1.0; 5]).is_err());
    // Keys, labels and names.
    assert_eq!(CurveForm::ALL.len(), 18);
    assert_eq!(&CurveForm::ALL[8..], &NEW_FORMS);
    let keys: Vec<&str> = NEW_FORMS.iter().map(|f| f.key()).collect();
    assert_eq!(
        keys,
        [
            "line",
            "dose_response_log",
            "dose_response",
            "logistic5",
            "weibull_growth",
            "beta_growth",
            "segmented",
            "two_exponentials",
            "gaussian_peak",
            "lorentzian_peak"
        ]
    );
    for form in NEW_FORMS {
        assert_eq!(CurveForm::from_key(form.key()), Some(form));
        let names: Vec<String> = form.parameter_names().iter().map(|s| (*s).to_owned()).collect();
        let text = form.formula("x", &names);
        assert!(names.iter().all(|n| text.contains(n.as_str())), "{text}");
    }
}

/// The formula texts, written out by hand. Each mirrors `value` operation for
/// operation, in the formula language (`**` power, float literals).
#[test]
fn formula_texts() {
    let text = |form: CurveForm| {
        let names: Vec<String> = form.parameter_names().iter().map(|s| (*s).to_owned()).collect();
        form.formula("x", &names)
    };
    let expected = [
        (CurveForm::StraightLine, "Intercept + Slope * x"),
        (CurveForm::DoseResponseLog, "Bottom + (Top - Bottom) / (1.0 + 10.0 ** ((LogEC50 - x) * HillSlope))"),
        (CurveForm::DoseResponse, "Bottom + (Top - Bottom) / (1.0 + (EC50 / x) ** HillSlope)"),
        (
            CurveForm::AsymmetricLogistic5,
            "Bottom + (Top - Bottom) / (1.0 + 10.0 ** ((LogC - x) * HillSlope)) ** Asymmetry",
        ),
        (CurveForm::WeibullGrowth, "Asymptote - (Asymptote - Initial) * exp(-((Rate * x) ** Shape))"),
        (
            CurveForm::BetaGrowth,
            "Ymax * (1.0 + (End - min(x, End)) / (End - Peak)) * (min(x, End) / End) ** (End / (End - Peak))",
        ),
        (CurveForm::Segmented, "Intercept + Slope1 * x + (Slope2 - Slope1) * max(x - Breakpoint, 0.0)"),
        (CurveForm::TwoPhaseExponential, "Plateau + Span1 * exp(-Rate1 * x) + Span2 * exp(-Rate2 * x)"),
        (CurveForm::GaussianPeak, "Baseline + Height * exp(-((x - Center) ** 2.0) / (2.0 * Width ** 2.0))"),
        (CurveForm::LorentzianPeak, "Baseline + Height / (1.0 + ((x - Center) / HalfWidth) ** 2.0)"),
    ];
    for (form, want) in expected {
        assert_eq!(text(form), want, "{form:?}");
    }
    // Negative numbers are parenthesised.
    let p = ["-1.5".to_owned(), "2.0".to_owned(), "-0.25".to_owned(), "3.0".to_owned()];
    assert_eq!(CurveForm::Segmented.formula("x", &p), "(-1.5) + 2.0 * x + ((-0.25) - 2.0) * max(x - 3.0, 0.0)");
}

/// Steep curves far from the midpoint: `exp` overflows, and the gradients
/// used to be 0·∞ = NaN. The exact derivatives there are 0 to within
/// e^(−1000), so the gradient must be finite and zero.
#[test]
fn gradients_stay_finite_when_exp_overflows() {
    for (form, theta, x) in [
        (CurveForm::Logistic3, vec![10.0, 100.0, 10.0], 0.0),
        (CurveForm::Logistic4, vec![1.0, 10.0, 100.0, 10.0], 0.0),
        (CurveForm::Gompertz, vec![10.0, 100.0, 10.0], 0.0),
    ] {
        let mut g = vec![f64::NAN; theta.len()];
        form.gradient(x, &theta, &mut g);
        assert!(g.iter().all(|v| v.is_finite()), "{form:?}: {g:?}");
        // Logistic4's lower-asymptote weight is 1 − q = 1 there; the rest vanish.
        let expect: Vec<f64> =
            if form == CurveForm::Logistic4 { vec![1.0, 0.0, 0.0, 0.0] } else { vec![0.0; theta.len()] };
        for (a, b) in g.iter().zip(&expect) {
            assert!((a - b).abs() < 1e-12, "{form:?}: {g:?}");
        }
    }
}
