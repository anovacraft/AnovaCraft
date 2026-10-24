//! S–N (fatigue) curves compared with an independent SciPy maximum
//! likelihood implementation (`tests/data/fatigue_reference.py`, which
//! writes `fatigue_reference.json`). The data are an invented
//! rotating-bending test (32 specimens at 200–400 MPa, run-outs at 10⁷
//! cycles), also the application's sample data; the Basquin fits are also
//! compared with lifelines' AFT fitters.

use stat_data::{JsonValue, parse_json};
use stat_reliability::LifeObservation;
use stat_reliability::fatigue::{FatigueData, FatigueFit, FatigueModel, SnForm, fit_fatigue};
use stat_reliability::life_regression::RegressionFamily;

fn reference() -> JsonValue {
    parse_json(include_str!("data/fatigue_reference.json")).unwrap()
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-12);
    assert!(
        (got - want).abs() <= rel * scale,
        "{what}: got {got}, reference {want} (relative error {:e})",
        (got - want).abs() / scale
    );
}

fn data(r: &JsonValue) -> FatigueData {
    let rows = r.get("data").unwrap().as_array().unwrap();
    FatigueData {
        observations: rows
            .iter()
            .map(|o| {
                let n = num(o.get("cycles").unwrap());
                if o.get("failed").unwrap().as_bool().unwrap() {
                    LifeObservation::exact(n, 1.0)
                } else {
                    LifeObservation::right(n, 1.0)
                }
            })
            .collect(),
        stresses: rows.iter().map(|o| num(o.get("stress").unwrap())).collect(),
    }
}

fn fit_case(data: &FatigueData, case: &JsonValue) -> FatigueFit {
    let model = FatigueModel {
        form: SnForm::from_key(case.get("form").unwrap().as_str().unwrap()).unwrap(),
        family: RegressionFamily::from_key(case.get("family").unwrap().as_str().unwrap()).unwrap(),
        stress_dependent_scale: case.get("scale_stress").unwrap().as_bool().unwrap(),
    };
    fit_fatigue(data, &model).unwrap()
}

fn check_case(fit: &FatigueFit, case: &JsonValue, est: f64, se_rel: f64) {
    let name = case.get("name").unwrap().as_str().unwrap();
    close(fit.log_likelihood(), num(case.get("log_likelihood").unwrap()), 1e-9, &format!("{name} log-likelihood"));
    let coef: Vec<f64> = fit.fit.location_coefficients.iter().chain(&fit.fit.scale_coefficients).copied().collect();
    let want: Vec<f64> =
        nums(case.get("location").unwrap()).into_iter().chain(nums(case.get("scale").unwrap())).collect();
    let ses = nums(case.get("std_errors").unwrap());
    let cov = fit.fit.covariance.as_ref().unwrap();
    for (i, (g, w)) in coef.iter().zip(&want).enumerate() {
        // Absolute tolerance relative to the standard error for coefficients near 0.
        assert!((g - w).abs() <= est * w.abs().max(ses[i]), "{name} coefficient {i}: got {g}, reference {w}");
        close(cov[i][i].sqrt(), ses[i], se_rel, &format!("{name} standard error {i}"));
    }
    let sigma = case.get("sigma_at").unwrap();
    close(fit.sigma(200.0), num(sigma.get("200").unwrap()), est, &format!("{name} σ(200)"));
    close(fit.sigma(400.0), num(sigma.get("400").unwrap()), est, &format!("{name} σ(400)"));
    for q in case.get("quantiles").unwrap().as_array().unwrap() {
        let (s, p) = (num(q.get("stress").unwrap()), num(q.get("p").unwrap()));
        let got = fit.quantile(s, p, 0.95, false);
        let what = format!("{name} N_{p} at {s}");
        close(got.estimate, num(q.get("estimate").unwrap()), 10.0 * est, &what);
        let wald = nums(q.get("wald").unwrap());
        close(got.wald.0, wald[0], se_rel, &format!("{what} Wald lower"));
        close(got.wald.1, wald[1], se_rel, &format!("{what} Wald upper"));
    }
    for f in case.get("failure_probability").unwrap().as_array().unwrap() {
        let (s, n) = (num(f.get("stress").unwrap()), num(f.get("cycles").unwrap()));
        let got = fit.failure_probability(s, n, 0.95, false);
        close(got.estimate, num(f.get("estimate").unwrap()), 10.0 * est, &format!("{name} F({n}) at {s}"));
    }
    for a in case.get("stress_at_life").unwrap().as_array().unwrap() {
        let (n, p) = (num(a.get("cycles").unwrap()), num(a.get("p").unwrap()));
        let got = fit.stress_at_life(n, p);
        match a.get("stress").unwrap().as_f64() {
            Some(want) => close(got.unwrap(), want, est, &format!("{name} stress at N_{p} = {n}")),
            None => assert!(got.is_none(), "{name}: stress at {n} should not be reached"),
        }
    }
}

#[test]
fn closed_form_curves_match_scipy() {
    let r = reference();
    let d = data(&r);
    for case in r.get("cases").unwrap().as_array().unwrap() {
        if case.get("form").unwrap().as_str().unwrap() == "endurance-limit" {
            continue;
        }
        let fit = fit_case(&d, case);
        assert!(fit.endurance_limit.is_none());
        check_case(&fit, case, 1e-6, 1e-4);
    }
}

#[test]
fn endurance_limit_matches_scipy_profile() {
    let r = reference();
    let d = data(&r);
    for case in r.get("cases").unwrap().as_array().unwrap() {
        if case.get("form").unwrap().as_str().unwrap() != "endurance-limit" {
            continue;
        }
        let name = case.get("name").unwrap().as_str().unwrap();
        let fit = fit_case(&d, case);
        let want = case.get("endurance_limit").unwrap();
        let el = fit.endurance_limit.as_ref().unwrap();
        assert!(!el.at_zero);
        close(el.estimate, num(want.get("estimate").unwrap()), 2e-5, &format!("{name} endurance limit"));
        let lr = nums(want.get("likelihood_ratio").unwrap());
        close(el.likelihood_ratio.0, lr[0], 1e-5, &format!("{name} endurance limit lower"));
        close(el.likelihood_ratio.1, lr[1], 1e-5, &format!("{name} endurance limit upper"));
        let again = fit.endurance_limit_interval(0.95).unwrap();
        close(again.0, lr[0], 1e-5, &format!("{name} recomputed lower"));
        // Coefficients move with Ê, so they are compared a little more loosely.
        check_case(&fit, case, 2e-5, 1e-4);
        // A wider interval at a higher level.
        let wide = fit.endurance_limit_interval(0.99).unwrap();
        assert!(wide.0 < el.likelihood_ratio.0 && wide.1 > el.likelihood_ratio.1);
        // The profile on the grid is never above the maximum.
        assert!(el.profile.iter().all(|&(_, l)| l <= fit.log_likelihood() + 1e-9));
        // Below the endurance limit life is infinite.
        assert!(fit.quantile(el.estimate * 0.9, 0.5, 0.95, false).estimate.is_infinite());
        assert_eq!(fit.failure_probability(el.estimate * 0.9, 1e9, 0.95, false).estimate, 0.0);
    }
}

#[test]
fn levels_residuals_and_basquin_coefficients() {
    let r = reference();
    let d = data(&r);
    let case = &r.get("cases").unwrap().as_array().unwrap()[0];
    let fit = fit_case(&d, case);
    let counts: Vec<(f64, f64, f64)> = fit.levels.iter().map(|l| (l.stress, l.failures, l.run_outs)).collect();
    assert_eq!(counts[0], (200.0, 6.0, 2.0));
    assert_eq!(counts.len(), 5);
    let res = fit.standardized_residuals();
    assert_eq!(res.len(), d.stresses.len());
    assert_eq!(res.iter().filter(|r| r.1).count(), 2);
    let (a, b) = fit.basquin_coefficients().unwrap();
    // S = A N^b reproduces the median curve at every stress.
    for s in [220.0, 300.0, 390.0] {
        close(a * fit.quantile(s, 0.5, 0.95, false).estimate.powf(b), s, 1e-9, "Basquin inversion");
    }
    // Likelihood-ratio limits bracket the estimate.
    let q = fit.quantile(250.0, 0.1, 0.95, true);
    let (lo, hi) = q.likelihood_ratio.unwrap();
    assert!(lo < q.estimate && q.estimate < hi);
}

/// A second, unrelated implementation: lifelines' lognormal and Weibull AFT
/// fitters with covariate ln S (a Basquin curve is an AFT model in ln S).
#[test]
fn basquin_matches_lifelines() {
    let r = reference();
    let d = data(&r);
    let ll = r.get("lifelines").unwrap();
    for family in [RegressionFamily::Lognormal, RegressionFamily::Weibull] {
        let want = ll.get(family.key()).unwrap();
        let model = FatigueModel { form: SnForm::Basquin, family, stress_dependent_scale: false };
        let fit = fit_fatigue(&d, &model).unwrap();
        let what = format!("lifelines {}", family.key());
        close(fit.log_likelihood(), num(want.get("log_likelihood").unwrap()), 1e-9, &format!("{what} log-likelihood"));
        for (g, w) in fit.fit.location_coefficients.iter().zip(nums(want.get("location").unwrap())) {
            close(*g, w, 1e-6, &format!("{what} coefficient"));
        }
        close(fit.fit.scale_coefficients[0], num(want.get("log_sigma").unwrap()), 1e-5, &format!("{what} ln σ"));
    }
}
