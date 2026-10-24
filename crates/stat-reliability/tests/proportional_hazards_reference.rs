//! Proportional hazards regression compared with statsmodels `PHReg`,
//! lifelines `CoxPHFitter` and a direct NumPy implementation of the
//! textbook formulas (`tests/data/proportional_hazards_reference.py`, which
//! writes `proportional_hazards_reference.json`), on the leukemia remission
//! data with covariates and the stratified Rossi recidivism data.

use stat_core::Matrix;
use stat_data::{JsonValue, parse_json};
use stat_reliability::{
    ProportionalHazardsData, ProportionalHazardsFit, ProportionalHazardsOptions, TieMethod, TimeTransform,
    fit_proportional_hazards,
};

fn cases() -> JsonValue {
    parse_json(include_str!("data/proportional_hazards_reference.json")).unwrap()
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-3);
    assert!(
        (got - want).abs() <= rel * scale,
        "{what}: got {got}, reference {want} (relative error {:e})",
        (got - want).abs() / scale
    );
}

fn fit(case: &JsonValue, ties: TieMethod) -> ProportionalHazardsFit {
    let time = nums(case.get("time").unwrap());
    let event: Vec<bool> = nums(case.get("event").unwrap()).iter().map(|&e| e == 1.0).collect();
    let rows: Vec<Vec<f64>> = case.get("x").unwrap().as_array().unwrap().iter().map(nums).collect();
    let strata = case
        .get("strata")
        .and_then(JsonValue::as_array)
        .map(|s| s.iter().map(|v| v.as_usize().unwrap()).collect::<Vec<usize>>());
    let data = ProportionalHazardsData::new(time, event, &Matrix::from_rows(&rows).unwrap(), strata).unwrap();
    fit_proportional_hazards(&data, &ProportionalHazardsOptions { ties, ..Default::default() }).unwrap()
}

/// Statsmodels `PHReg`: estimates, likelihoods, global tests, profile limits.
#[test]
fn estimates_match_statsmodels() {
    let all = cases();
    for name in ["leukemia", "rossi_strata"] {
        let case = all.get(name).unwrap();
        for (ties, key) in [(TieMethod::Efron, "efron"), (TieMethod::Breslow, "breslow")] {
            let f = fit(case, ties);
            let want = case.get(key).unwrap();
            let what = |s: &str| format!("{name} {key} {s}");
            for (j, c) in f.coefficients.iter().enumerate() {
                close(c.estimate, nums(want.get("coef").unwrap())[j], 1e-7, &what("coefficient"));
                close(c.std_error, nums(want.get("se").unwrap())[j], 1e-6, &what("standard error"));
                let prof = nums(&want.get("profile").unwrap().as_array().unwrap()[j]);
                let (lo, hi) = c.profile.unwrap();
                close(lo, prof[0], 1e-6, &what("profile lower"));
                close(hi, prof[1], 1e-6, &what("profile upper"));
            }
            close(f.log_likelihood, num(want.get("loglik").unwrap()), 1e-10, &what("log-likelihood"));
            close(f.null_log_likelihood, num(want.get("null_loglik").unwrap()), 1e-10, &what("null log-likelihood"));
            close(f.likelihood_ratio.statistic, num(want.get("lr").unwrap()), 1e-7, &what("LR"));
            close(f.wald.statistic, num(want.get("wald").unwrap()), 1e-6, &what("Wald"));
            close(f.score.statistic, num(want.get("score").unwrap()), 1e-9, &what("score"));
        }
    }
}

/// The direct NumPy implementation: baseline, predicted survival with
/// limits, residuals and proportional-hazards tests (both tie methods).
#[test]
fn diagnostics_match_direct_formulas() {
    let all = cases();
    for name in ["leukemia", "rossi_strata"] {
        let case = all.get(name).unwrap();
        for (ties, key) in [(TieMethod::Efron, "efron"), (TieMethod::Breslow, "breslow")] {
            let f = fit(case, ties);
            let want = case.get(key).unwrap().get("direct").unwrap();
            let what = |s: &str| format!("{name} {key} {s}");
            for (s, b) in want.get("baseline").unwrap().as_array().unwrap().iter().enumerate() {
                let times = nums(b.get("times").unwrap());
                let h = nums(b.get("cumulative_hazard").unwrap());
                assert_eq!(f.baseline[s].len(), times.len(), "{}", what("baseline steps"));
                for (step, (t, h)) in f.baseline[s].iter().zip(times.iter().zip(&h)) {
                    assert_eq!(step.time, *t);
                    close(step.cumulative_hazard, *h, 1e-7, &what("baseline"));
                }
            }
            let pred = want.get("prediction").unwrap();
            let x0 = nums(pred.get("x").unwrap());
            for p in pred.get("points").unwrap().as_array().unwrap() {
                let t = num(p.get("time").unwrap());
                let got = f.survival_at(&x0, 0, t, 0.95);
                close(got.survival, num(p.get("survival").unwrap()), 1e-7, &what("predicted survival"));
                close(got.std_error, num(p.get("std_error").unwrap()), 1e-6, &what("SE of cumulative hazard"));
                close(got.lower, num(p.get("lower").unwrap()), 1e-6, &what("survival lower"));
                close(got.upper, num(p.get("upper").unwrap()), 1e-6, &what("survival upper"));
            }
            let sch = f.schoenfeld_residuals();
            let want_sch = want.get("schoenfeld").unwrap().as_array().unwrap();
            assert_eq!(sch.len(), want_sch.len());
            for w in want_sch {
                let unit = w.get("unit").unwrap().as_usize().unwrap();
                let got = sch.iter().find(|r| r.unit == unit).unwrap();
                for (g, r) in got.residual.iter().zip(nums(w.get("residual").unwrap())) {
                    assert!((g - r).abs() < 1e-6, "{}: {g} vs {r}", what("Schoenfeld"));
                }
            }
            for (g, r) in f.martingale_residuals().iter().zip(nums(want.get("martingale").unwrap())) {
                assert!((g - r).abs() < 1e-7, "{}: {g} vs {r}", what("martingale"));
            }
            for (g, r) in f.deviance_residuals().iter().zip(nums(want.get("deviance").unwrap())) {
                assert!((g - r).abs() < 1e-6, "{}: {g} vs {r}", what("deviance"));
            }
            for transform in TimeTransform::ALL {
                let test = f.proportionality_test(transform);
                let w = want.get("ph_tests").unwrap().get(transform.key()).unwrap();
                for (g, r) in test.covariates.iter().zip(nums(w.get("covariates").unwrap())) {
                    close(g.statistic, r, 1e-5, &what(&format!("{} PH test", transform.key())));
                }
                close(test.global.statistic, num(w.get("global").unwrap()), 1e-5, &what("global PH test"));
                close(test.global.p_value, num(w.get("global_p").unwrap()), 1e-4, &what("global PH p-value"));
            }
        }
    }
}

/// Lifelines `CoxPHFitter` (Efron ties): estimates, baseline, residuals and
/// the per-covariate Grambsch–Therneau tests (lifelines numbers tied ranks
/// consecutively, so its rank transform is not compared).
#[test]
fn efron_matches_lifelines() {
    let all = cases();
    for name in ["leukemia", "rossi_strata"] {
        let case = all.get(name).unwrap();
        let f = fit(case, TieMethod::Efron);
        let want = case.get("lifelines_efron").unwrap();
        let what = |s: &str| format!("{name} lifelines {s}");
        for (j, c) in f.coefficients.iter().enumerate() {
            close(c.estimate, nums(want.get("coef").unwrap())[j], 1e-6, &what("coefficient"));
            close(c.std_error, nums(want.get("se").unwrap())[j], 1e-6, &what("standard error"));
        }
        close(f.log_likelihood, num(want.get("loglik").unwrap()), 1e-9, &what("log-likelihood"));
        for (g, r) in f.martingale_residuals().iter().zip(nums(want.get("martingale").unwrap())) {
            assert!((g - r).abs() < 1e-6, "{}: {g} vs {r}", what("martingale"));
        }
        for (g, r) in f.deviance_residuals().iter().zip(nums(want.get("deviance").unwrap())) {
            assert!((g - r).abs() < 1e-6, "{}: {g} vs {r}", what("deviance"));
        }
        if let Some(b) = want.get("baseline") {
            let times = nums(b.get("times").unwrap());
            let h = nums(b.get("cumulative_hazard").unwrap());
            for step in &f.baseline[0] {
                let k = times.iter().position(|t| *t == step.time).unwrap();
                close(step.cumulative_hazard, h[k], 1e-6, &what("baseline"));
            }
            let sch = f.schoenfeld_residuals();
            for w in want.get("schoenfeld").unwrap().as_array().unwrap() {
                let unit = w.get("unit").unwrap().as_usize().unwrap();
                let got = sch.iter().find(|r| r.unit == unit).unwrap();
                for (g, r) in got.residual.iter().zip(nums(w.get("residual").unwrap())) {
                    assert!((g - r).abs() < 1e-6, "{}: {g} vs {r}", what("Schoenfeld"));
                }
            }
        }
        if let Some(tests) = want.get("ph_tests") {
            for transform in [TimeTransform::KaplanMeier, TimeTransform::Identity, TimeTransform::Log] {
                let got = f.proportionality_test(transform);
                for (g, r) in got.covariates.iter().zip(nums(tests.get(transform.key()).unwrap())) {
                    close(g.statistic, r, 1e-5, &what(transform.key()));
                }
            }
        }
    }
}
