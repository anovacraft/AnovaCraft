//! Degradation analysis compared with independent NumPy/SciPy computations
//! and a statsmodels mixed model (`tests/data/degradation_reference.py`,
//! which writes `degradation_reference.json`). The laser data are Meeker &
//! Escobar's (1998, Table C.17); the other datasets are invented.

use stat_core::distribution_fit::FitFamily;
use stat_data::{JsonValue, parse_json};
use stat_reliability::degradation::{
    Crossing, DestructiveData, DestructiveModel, FailureDirection, PathModel, ScaleTransform, UnitSeries,
    fit_destructive, fit_random_paths, fit_unit_path,
};
use stat_reliability::life_regression::RegressionFamily;
use stat_reliability::life_stress::{StressTransform, StressVariable};
use stat_reliability::{LifeData, fit_censored};

fn cases() -> JsonValue {
    parse_json(include_str!("data/degradation_reference.json")).unwrap()
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

fn direction(case: &JsonValue) -> FailureDirection {
    if case.get("rising").unwrap().as_bool().unwrap() { FailureDirection::Rising } else { FailureDirection::Falling }
}

#[test]
fn unit_paths_and_pseudo_failure_times_match_reference() {
    let all = cases();
    for name in ["laser_linear", "asymptotic", "power", "light_output"] {
        let case = all.get(name).unwrap();
        let tr = |k: &str| ScaleTransform::from_key(case.get(k).unwrap().as_str().unwrap()).unwrap();
        let model = match case.get("model").unwrap().as_str().unwrap() {
            "linear" => PathModel::Linear { response: tr("response"), time: tr("time") },
            "asymptotic" => PathModel::Asymptotic,
            _ => PathModel::Power,
        };
        let threshold = num(case.get("threshold").unwrap());
        let nonlinear = !matches!(model, PathModel::Linear { .. });
        let tol = if nonlinear { 2e-5 } else { 1e-9 };
        let mut observations = Vec::new();
        for (u, want) in
            case.get("units").unwrap().as_array().unwrap().iter().zip(case.get("paths").unwrap().as_array().unwrap())
        {
            let path = fit_unit_path(
                &nums(u.get("times").unwrap()),
                &nums(u.get("values").unwrap()),
                model,
                threshold,
                direction(case),
            )
            .unwrap();
            let p = nums(want.get("parameters").unwrap());
            // numpy.polyfit returns (slope, intercept); the script stores [a, b].
            close(path.parameters[0], p[0], tol, &format!("{name} parameter 0"));
            close(path.parameters[1], p[1], tol, &format!("{name} parameter 1"));
            let kind = want.get("kind").unwrap().as_str().unwrap();
            let t = num(want.get("time").unwrap());
            match (kind, path.crossing) {
                ("at", Crossing::At(got)) => close(got, t, tol * 10.0, &format!("{name} pseudo failure time")),
                ("never", Crossing::Never(got)) | ("before", Crossing::Before(got)) => assert_eq!(got, t),
                (k, c) => panic!("{name}: reference {k}, got {c:?}"),
            }
            observations.push(path.crossing.observation());
        }
        let data = LifeData::new(observations).unwrap();
        let JsonValue::Object(life) = case.get("life").unwrap() else { panic!() };
        for (family, want) in life {
            let f = if family == "weibull" { FitFamily::Weibull } else { FitFamily::Lognormal };
            let fit = fit_censored(&data, f).unwrap();
            let tol = if nonlinear { 1e-4 } else { 1e-6 };
            close(fit.distribution.location, num(want.get("mu").unwrap()), tol, &format!("{name} {family} μ"));
            close(fit.distribution.scale, num(want.get("sigma").unwrap()), tol * 10.0, &format!("{name} {family} σ"));
            close(fit.log_likelihood(), num(want.get("loglik").unwrap()), tol, &format!("{name} {family} ℓ"));
        }
    }
}

#[test]
fn random_path_model_matches_statsmodels() {
    let case = cases().get("random_effects").unwrap().clone();
    let units: Vec<UnitSeries> = case
        .get("units")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|u| UnitSeries { times: nums(u.get("times").unwrap()), values: nums(u.get("values").unwrap()) })
        .collect();
    let fit = fit_random_paths(
        &units,
        ScaleTransform::Identity,
        ScaleTransform::Identity,
        num(case.get("threshold").unwrap()),
        FailureDirection::Rising,
        true,
    )
    .unwrap();
    let beta = nums(case.get("beta").unwrap());
    close(fit.beta[0], beta[0], 1e-5, "β₀");
    close(fit.beta[1], beta[1], 1e-5, "β₁");
    let cov = nums(case.get("cov_re").unwrap());
    close(fit.sigma[0], cov[0], 2e-3, "Σ₀₀");
    close(fit.sigma[2], cov[2], 2e-3, "Σ₁₁");
    assert!((fit.sigma[1] - cov[1]).abs() < 2e-3 * (cov[0] * cov[2]).sqrt(), "Σ₀₁ {} vs {}", fit.sigma[1], cov[1]);
    close(fit.residual_sd * fit.residual_sd, num(case.get("scale").unwrap()), 2e-3, "residual variance");
    for f in case.get("probabilities").unwrap().as_array().unwrap() {
        let t = num(f.get("t").unwrap());
        let want = num(f.get("F").unwrap());
        assert!((fit.failure_probability(t) - want).abs() < 5e-4, "F({t}): {} vs {want}", fit.failure_probability(t));
    }
    for q in case.get("quantiles").unwrap().as_array().unwrap() {
        let p = num(q.get("p").unwrap());
        close(fit.quantile(p, 1000.0), num(q.get("t").unwrap()), 2e-3, &format!("t_{p}"));
    }
    // The bootstrap is reproducible and its limits bracket the estimates.
    let bands = fit.bootstrap(&[3.0, 5.0], &[0.5], 1000.0, 40, 11, 0.9);
    let again = fit.bootstrap(&[3.0, 5.0], &[0.5], 1000.0, 40, 11, 0.9);
    assert_eq!(bands, again);
    assert!(bands.replicates >= 35);
    for (k, &t) in bands.times.iter().enumerate() {
        let f = fit.failure_probability(t);
        assert!(bands.lower[k] <= f && f <= bands.upper[k], "band at {t}");
    }
    let (_, lo, hi) = bands.quantiles[0];
    let median = fit.quantile(0.5, 1000.0);
    assert!(lo <= median && median <= hi);
}

#[test]
fn destructive_degradation_matches_scipy() {
    let case = cases().get("destructive").unwrap().clone();
    let model = DestructiveModel {
        family: RegressionFamily::Normal,
        response: ScaleTransform::Log,
        time: ScaleTransform::Sqrt,
        stress: Some(StressVariable::new(StressTransform::Arrhenius)),
        use_stress: Some(num(case.get("use").unwrap())),
        threshold: num(case.get("threshold").unwrap()),
        direction: FailureDirection::Falling,
    };
    let data = DestructiveData {
        times: nums(case.get("times").unwrap()),
        values: nums(case.get("values").unwrap()),
        stresses: nums(case.get("stresses").unwrap()),
    };
    let fit = fit_destructive(&data, &model).unwrap();
    let theta = nums(case.get("theta").unwrap());
    let se = nums(case.get("se").unwrap());
    let got = &fit.parameters;
    for i in 0..4 {
        assert!((got[i] - theta[i]).abs() < 2e-3 * se[i], "θ[{i}]: {} vs {}", got[i], theta[i]);
        close(fit.covariance.as_ref().unwrap()[i][i].sqrt(), se[i], 3e-3, &format!("se[{i}]"));
    }
    // The activation energy of the degradation rate is −β₂ (data simulated with 0.7 eV).
    assert!((-got[2] - 0.7).abs() < 3.0 * se[2]);
    close(fit.log_likelihood, num(case.get("loglik").unwrap()), 1e-8, "ℓ");
    let use_s = Some(num(case.get("use").unwrap()));
    for q in case.get("quantiles").unwrap().as_array().unwrap() {
        let p = num(q.get("p").unwrap());
        let got = fit.quantile(p, use_s, 0.95);
        close(got.estimate, num(q.get("estimate").unwrap()), 1e-4, &format!("t_{p}"));
        let w = nums(q.get("wald").unwrap());
        close(got.wald.0, w[0], 3e-3, &format!("t_{p} lower"));
        close(got.wald.1, w[1], 3e-3, &format!("t_{p} upper"));
    }
    for f in case.get("probabilities").unwrap().as_array().unwrap() {
        let t = num(f.get("t").unwrap());
        let got = fit.failure_probability(t, use_s, 0.95);
        close(got.estimate, num(f.get("estimate").unwrap()), 1e-4, &format!("F({t})"));
        let w = nums(f.get("wald").unwrap());
        close(got.wald.0, w[0], 3e-3, &format!("F({t}) lower"));
        close(got.wald.1, w[1], 3e-3, &format!("F({t}) upper"));
    }
}
