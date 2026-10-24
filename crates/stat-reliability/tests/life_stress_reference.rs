//! Life–stress models compared with independent SciPy maximum likelihood and
//! lifelines AFT fits (`tests/data/life_stress_reference.py`, which writes
//! `life_stress_reference.json`), and with estimates printed in the
//! literature for two published accelerated life tests.

use stat_data::{JsonValue, parse_json};
use stat_reliability::life_regression::RegressionFamily;
use stat_reliability::life_stress::{
    LifeStressData, LifeStressFit, LifeStressModel, StressTransform, StressVariable, fit_life_stress,
};
use stat_reliability::{IntervalEstimate, LifeObservation};

const LEVEL: f64 = 0.95;

fn cases() -> JsonValue {
    parse_json(include_str!("data/life_stress_reference.json")).unwrap()
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn pair(v: &JsonValue) -> (f64, f64) {
    let v = nums(v);
    (v[0], v[1])
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-12);
    assert!(
        (got - want).abs() <= rel * scale,
        "{what}: got {got}, reference {want} (relative error {:e})",
        (got - want).abs() / scale
    );
}

fn fit_case(case: &JsonValue) -> LifeStressFit {
    let family = RegressionFamily::from_key(case.get("family").unwrap().as_str().unwrap()).unwrap();
    let stresses: Vec<StressVariable> = case
        .get("kinds")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|k| StressVariable::new(StressTransform::from_key(k.as_str().unwrap()).unwrap()))
        .collect();
    let model = LifeStressModel {
        family,
        stresses,
        interaction: case.get("interaction").unwrap().as_bool().unwrap(),
        stress_dependent_scale: case.get("scale_stress").unwrap().as_bool().unwrap(),
    };
    let observations = case
        .get("obs")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            let o = o.as_array().unwrap();
            LifeObservation::interval(
                o[0].as_f64().unwrap_or(f64::NEG_INFINITY),
                o[1].as_f64().unwrap_or(f64::INFINITY),
                num(&o[2]),
            )
        })
        .collect();
    let stresses = case.get("stresses").unwrap().as_array().unwrap().iter().map(nums).collect();
    fit_life_stress(&LifeStressData { observations, stresses }, &model).unwrap()
}

fn check_interval(got: &IntervalEstimate, want: &JsonValue, rel: f64, what: &str) {
    close(got.estimate, num(want.get("estimate").unwrap()), 2e-5, &format!("{what} estimate"));
    let (wl, wu) = pair(want.get("wald").unwrap());
    close(got.wald.0, wl, rel, &format!("{what} Wald lower"));
    close(got.wald.1, wu, rel, &format!("{what} Wald upper"));
    let (ll, lu) = pair(want.get("lr").unwrap());
    let lr = got.likelihood_ratio.unwrap();
    close(lr.0, ll, rel, &format!("{what} LR lower"));
    close(lr.1, lu, rel, &format!("{what} LR upper"));
}

/// Probabilities are compared on the logit scale on which their limits are
/// formed, relative to the width of the Wald interval there: tiny
/// probabilities at use conditions sit far out in the tail, where the
/// small difference between two numerical Hessians moves the limits by a
/// few percent of their (minute) values.
fn check_probability(got: &IntervalEstimate, want: &JsonValue, rel: f64, what: &str) {
    let logit = |f: f64| f.ln() - (-f).ln_1p();
    let est = num(want.get("estimate").unwrap());
    assert!((logit(got.estimate) - logit(est)).abs() < 1e-5, "{what} estimate: {} vs {est}", got.estimate);
    let (wl, wu) = pair(want.get("wald").unwrap());
    let width = logit(wu) - logit(wl);
    let (ll, lu) = pair(want.get("lr").unwrap());
    let lr = got.likelihood_ratio.unwrap();
    for (g, w, side) in
        [(got.wald.0, wl, "Wald lower"), (got.wald.1, wu, "Wald upper"), (lr.0, ll, "LR lower"), (lr.1, lu, "LR upper")]
    {
        assert!((logit(g) - logit(w)).abs() <= rel * width, "{what} {side}: got {g}, reference {w}");
    }
}

#[test]
fn life_stress_fits_match_scipy() {
    let JsonValue::Object(all) = cases() else { panic!() };
    for (name, case) in &all {
        let fit = fit_case(case);
        let theta = nums(case.get("theta").unwrap());
        let se = nums(case.get("se").unwrap());
        let got: Vec<f64> = fit.fit.location_coefficients.iter().chain(&fit.fit.scale_coefficients).copied().collect();
        assert_eq!(got.len(), theta.len(), "{name}: parameter count");
        close(fit.fit.log_likelihood, num(case.get("loglik").unwrap()), 1e-8, &format!("{name} log-likelihood"));
        let cov = fit.fit.covariance.as_ref().unwrap();
        for i in 0..theta.len() {
            assert!(
                (got[i] - theta[i]).abs() <= 2e-3 * se[i],
                "{name} θ[{i}]: got {}, reference {} (se {})",
                got[i],
                theta[i],
                se[i]
            );
            close(cov[i][i].sqrt(), se[i], 2e-3, &format!("{name} se[{i}]"));
        }
        let use_s = nums(case.get("use").unwrap());
        let other = nums(case.get("other").unwrap());
        for q in case.get("quantiles").unwrap().as_array().unwrap() {
            let p = num(q.get("p").unwrap());
            check_interval(&fit.quantile(&use_s, p, LEVEL, true), q, 2e-3, &format!("{name} t_{p}"));
        }
        for f in case.get("probabilities").unwrap().as_array().unwrap() {
            let t = num(f.get("t").unwrap());
            check_probability(&fit.failure_probability(&use_s, t, LEVEL, true), f, 2e-3, &format!("{name} F({t})"));
        }
        check_interval(
            &fit.acceleration_factor(&other, &use_s, LEVEL, true),
            case.get("acceleration").unwrap(),
            2e-3,
            &format!("{name} acceleration factor"),
        );
        for (j, c) in case.get("coefficients").unwrap().as_array().unwrap().iter().enumerate() {
            let mut a = vec![0.0; j + 2];
            a[j + 1] = 1.0;
            let got = fit.fit.linear_function(&a, 0.0, LEVEL, true);
            let s = num(c.get("se").unwrap());
            let (ll, lu) = pair(c.get("lr").unwrap());
            let lr = got.likelihood_ratio.unwrap();
            assert!((lr.0 - ll).abs() <= 2e-3 * s && (lr.1 - lu).abs() <= 2e-3 * s, "{name} β{} LR limits", j + 1);
        }
    }
}

#[test]
fn arrhenius_models_match_lifelines() {
    let all = cases();
    for name in ["device_a_lognormal", "device_a_weibull", "device_a_loglogistic"] {
        let case = all.get(name).unwrap();
        let fit = fit_case(case);
        let ll = case.get("lifelines").unwrap();
        close(fit.fit.log_likelihood, num(ll.get("loglik").unwrap()), 1e-6, &format!("{name} lifelines ℓ"));
        close(fit.fit.location_coefficients[0], num(ll.get("intercept").unwrap()), 1e-4, &format!("{name} β₀"));
        close(fit.fit.location_coefficients[1], num(ll.get("slope").unwrap()), 1e-4, &format!("{name} β₁"));
        close(fit.sigma(&[10.0]), num(ll.get("sigma").unwrap()), 1e-4, &format!("{name} σ"));
    }
}

#[test]
fn level_comparison_matches_scipy() {
    let case = cases().get("device_a_lognormal").unwrap().clone();
    let fit = fit_case(&case);
    let want = case.get("levels").unwrap();
    let cmp = fit.level_comparison.as_ref().unwrap();
    // 10 °C has no failures, so it is left out.
    assert_eq!(cmp.used, vec![1, 2, 3]);
    close(cmp.individual, num(want.get("individual").unwrap()), 1e-7, "individual fits ℓ");
    close(cmp.common_scale.unwrap(), num(want.get("common_scale").unwrap()), 1e-7, "common scale ℓ");
    close(cmp.life_stress, num(want.get("life_stress").unwrap()), 1e-7, "life–stress ℓ on 40–80 °C");
    let dfs: Vec<usize> = cmp.tests.iter().map(|t| t.df).collect();
    assert_eq!(dfs, vec![2, 1, 3]);
    // Stress-dependent scale test (1 df) exists.
    assert_eq!(fit.scale_test.as_ref().unwrap().df, 1);
}

/// Device-A (Meeker & Escobar 1998, Table C.10, from Hooper & Amster 1990):
/// Meeker & Escobar's lognormal–Arrhenius maximum likelihood estimates
/// (ch. 19, Device-A example) are β₀ = −13.469, β₁ = Eₐ = 0.6279 eV,
/// σ = 0.9778 with log-likelihood −321.7.
#[test]
fn device_a_matches_published_estimates() {
    let fit = fit_case(cases().get("device_a_lognormal").unwrap());
    let b = &fit.fit.location_coefficients;
    assert!((b[0] - -13.469).abs() < 6e-4, "β₀ = {}", b[0]);
    assert!((b[1] - 0.6279).abs() < 6e-5, "Eₐ = {}", b[1]);
    assert!((fit.sigma(&[10.0]) - 0.9778).abs() < 6e-5, "σ = {}", fit.sigma(&[10.0]));
    assert!((fit.fit.log_likelihood - -321.7).abs() < 0.06, "ℓ = {}", fit.fit.log_likelihood);
    let ea = fit.stress_parameters(&["Temperature".to_owned()], LEVEL, false);
    assert_eq!(ea.len(), 1);
    assert!((ea[0].estimate.estimate - 0.6279).abs() < 6e-5);
}

/// Class-B insulation (Nelson 1990, *Accelerated Testing*, Table 4.1 of
/// ch. 3): Nelson's lognormal–Arrhenius fit of log₁₀ life on 1000/T (T in
/// kelvin, °C + 273.16) gives γ₀ ≈ −6.02, γ₁ ≈ 4.31 and σ ≈ 0.259 (log₁₀
/// units). The model on ln life and 1/(kT) is the same model
/// reparameterized, so the comparison converts the coefficients.
#[test]
fn class_b_matches_published_estimates() {
    // Refit with Kelvin = °C + 273.16 as Nelson did.
    let case = cases().get("class_b_lognormal").unwrap().clone();
    let mut fit = fit_case(&case);
    let model = LifeStressModel {
        stresses: vec![StressVariable {
            transform: StressTransform::Arrhenius,
            unit: stat_reliability::life_stress::TemperatureUnit::Kelvin,
        }],
        ..fit.model.clone()
    };
    let data = LifeStressData {
        observations: fit.data.observations.clone(),
        stresses: fit.data.stresses.iter().map(|s| vec![s[0] + 273.16]).collect(),
    };
    fit = fit_life_stress(&data, &model).unwrap();
    let k = 8.617_333_262e-5;
    let ln10 = std::f64::consts::LN_10;
    let b = &fit.fit.location_coefficients;
    // μ = β₀ + β₁/(kT) = ln10·(γ₀ + γ₁·1000/T)  ⇒  γ₀ = β₀/ln10, γ₁ = β₁/(1000 k ln10).
    let g0 = b[0] / ln10;
    let g1 = b[1] / (1000.0 * k * ln10);
    let sigma10 = fit.sigma(&[453.16]) / ln10;
    assert!((g0 - -6.02).abs() < 0.006, "γ₀ = {g0}");
    assert!((g1 - 4.31).abs() < 0.006, "γ₁ = {g1}");
    assert!((sigma10 - 0.259).abs() < 6e-4, "σ = {sigma10}");
    // The 150 °C level has no failures, so it is left out of the level comparison.
    assert_eq!(fit.level_comparison.as_ref().unwrap().used.len(), 3);
}
