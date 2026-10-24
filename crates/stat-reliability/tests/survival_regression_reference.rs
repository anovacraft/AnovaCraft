//! Parametric survival regression compared with an independent SciPy
//! maximization of the censored likelihood and with lifelines'
//! `WeibullAFTFitter`, `LogNormalAFTFitter` and `LogLogisticAFTFitter`
//! (`tests/data/survival_regression_reference.py`, which writes
//! `survival_regression_reference.json`): right-censored Rossi data, an
//! interval-censored capacitor life test with unit counts, and invented data
//! mixing exact, left-, interval- and right-censored times.

use stat_core::Matrix;
use stat_data::{JsonValue, parse_json};
use stat_reliability::{
    AftDistribution, LifeObservation, SurvivalRegressionFit, SurvivalRegressionOptions, fit_survival_regression,
};

fn cases() -> JsonValue {
    parse_json(include_str!("data/survival_regression_reference.json")).unwrap()
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn end(v: &JsonValue, missing: f64) -> f64 {
    v.as_f64().unwrap_or(missing)
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-3);
    assert!(
        (got - want).abs() <= rel * scale || got == want,
        "{what}: got {got}, reference {want} (relative error {:e})",
        (got - want).abs() / scale
    );
}

fn fit(case: &JsonValue, distribution: AftDistribution) -> SurvivalRegressionFit {
    let lower: Vec<f64> =
        case.get("lower").unwrap().as_array().unwrap().iter().map(|v| end(v, f64::NEG_INFINITY)).collect();
    let upper: Vec<f64> =
        case.get("upper").unwrap().as_array().unwrap().iter().map(|v| end(v, f64::INFINITY)).collect();
    let weight = nums(case.get("weight").unwrap());
    let obs: Vec<LifeObservation> =
        lower.iter().zip(&upper).zip(&weight).map(|((&l, &u), &w)| LifeObservation::interval(l, u, w)).collect();
    let rows: Vec<Vec<f64>> = case.get("x").unwrap().as_array().unwrap().iter().map(nums).collect();
    let names = case.get("names").unwrap().as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect();
    fit_survival_regression(
        &obs,
        &Matrix::from_rows(&rows).unwrap(),
        &SurvivalRegressionOptions { distribution, names, ..Default::default() },
    )
    .unwrap()
}

const DISTRIBUTIONS: [(AftDistribution, &str); 4] = [
    (AftDistribution::Weibull, "weibull"),
    (AftDistribution::Exponential, "exponential"),
    (AftDistribution::Lognormal, "lognormal"),
    (AftDistribution::LogLogistic, "loglogistic"),
];

#[test]
fn fits_match_scipy() {
    let all = cases();
    for name in ["rossi", "capacitor", "mixed"] {
        let case = all.get(name).unwrap();
        for (d, key) in DISTRIBUTIONS {
            let f = fit(case, d);
            let want = case.get("scipy").unwrap().get(key).unwrap();
            let what = |s: &str| format!("{name} {key} {s}");
            close(f.log_likelihood, num(want.get("loglik").unwrap()), 1e-9, &what("log-likelihood"));
            let theta = nums(want.get("theta").unwrap());
            let se = nums(want.get("se").unwrap());
            for (j, (g, w)) in f.theta().iter().zip(&theta).enumerate() {
                close(*g, *w, 2e-6, &what(&format!("θ{j}")));
            }
            let mut got_se: Vec<f64> = f.coefficients.iter().map(|c| c.std_error).collect();
            if let Some(s) = &f.scale {
                got_se.push(s.std_error / s.estimate);
            }
            for (j, (g, w)) in got_se.iter().zip(&se).enumerate() {
                close(*g, *w, 1e-5, &what(&format!("SE θ{j}")));
            }
            // Profile limits are in the reference for some fits only.
            if let Some(profile) = want.get("profile").and_then(|p| p.as_array()) {
                for (j, c) in f.coefficients.iter().enumerate() {
                    let p = nums(&profile[j]);
                    let (lo, hi) = c.profile.unwrap();
                    close(lo, p[0], 1e-6, &what("profile lower"));
                    close(hi, p[1], 1e-6, &what("profile upper"));
                }
                if let Some(s) = &f.scale {
                    let p = nums(profile.last().unwrap());
                    let (lo, hi) = s.profile.unwrap();
                    close(lo.ln(), p[0], 1e-6, &what("profile lower ln σ"));
                    close(hi.ln(), p[1], 1e-6, &what("profile upper ln σ"));
                }
            }
            for pred in want.get("predictions").unwrap().as_array().unwrap() {
                let x = nums(pred.get("x").unwrap());
                for q in pred.get("quantiles").unwrap().as_array().unwrap() {
                    let got = f.quantile(&x, num(q.get("p").unwrap()), 0.95);
                    close(got.estimate, num(q.get("estimate").unwrap()), 1e-6, &what("quantile"));
                    close(got.lower, num(q.get("lower").unwrap()), 1e-5, &what("quantile lower"));
                    close(got.upper, num(q.get("upper").unwrap()), 1e-5, &what("quantile upper"));
                }
                for p in pred.get("probabilities").unwrap().as_array().unwrap() {
                    let got = f.failure_probability(&x, num(p.get("t").unwrap()), 0.95);
                    close(got.estimate, num(p.get("estimate").unwrap()), 1e-6, &what("failure probability"));
                    close(got.lower, num(p.get("lower").unwrap()), 1e-5, &what("probability lower"));
                    close(got.upper, num(p.get("upper").unwrap()), 1e-5, &what("probability upper"));
                }
            }
            let drop = want.get("drop").unwrap();
            let test = f.drop_columns_test(&[drop.get("covariate").unwrap().as_usize().unwrap()]).unwrap();
            close(test.statistic, num(drop.get("statistic").unwrap()), 1e-5, &what("drop test"));
            for (g, w) in f.cox_snell_residuals().iter().zip(want.get("cox_snell").unwrap().as_array().unwrap()) {
                let w = w.as_array().unwrap();
                let (lo, hi) = (end(&w[0], f64::INFINITY), end(&w[1], f64::INFINITY));
                close(g.0, lo, 1e-5, &what("Cox–Snell lower"));
                close(g.1, hi, 1e-5, &what("Cox–Snell upper"));
            }
        }
    }
}

/// Lifelines' own optimizer stops a little earlier than ours (for the
/// log-logistic about 1e-5 in the log-likelihood), so the tolerances are
/// looser than for the SciPy reference, which was polished with Newton steps.
#[test]
fn fits_match_lifelines() {
    let all = cases();
    for name in ["rossi", "capacitor"] {
        let case = all.get(name).unwrap();
        for (d, key, tol) in [
            (AftDistribution::Weibull, "weibull", 1e-4),
            (AftDistribution::Lognormal, "lognormal", 1e-4),
            (AftDistribution::LogLogistic, "loglogistic", 5e-3),
        ] {
            let f = fit(case, d);
            let want = case.get("lifelines").unwrap().get(key).unwrap();
            let what = |s: &str| format!("{name} lifelines {key} {s}");
            close(f.log_likelihood, num(want.get("loglik").unwrap()), tol * 1e-2, &what("log-likelihood"));
            let coef = nums(want.get("coef").unwrap());
            let se = nums(want.get("se").unwrap());
            // The capacitor's temperature was centred at 105 for lifelines:
            // its intercept is the location at 105 °C, lot A.
            let intercept = if name == "capacitor" { f.location(&[105.0, 0.0]) } else { f.coefficients[0].estimate };
            close(intercept, coef[0], tol, &what("intercept"));
            for j in 1..coef.len() {
                close(f.coefficients[j].estimate, coef[j], tol, &what("coefficient"));
                close(f.coefficients[j].std_error, se[j], 10.0 * tol, &what("standard error"));
            }
            close(f.sigma().ln(), num(want.get("log_sigma").unwrap()), tol, &what("ln σ"));
        }
    }
}
