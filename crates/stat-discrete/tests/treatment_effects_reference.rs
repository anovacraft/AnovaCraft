//! Treatment-effect estimates compared with independent references:
//! statsmodels GLMs and a complex-step stacked sandwich for weighting and
//! the doubly robust estimator, the Lunceford–Davidian closed-form standard
//! error, a brute-force Abadie–Imbens ATE matching estimator and NumPy
//! balance statistics (`tests/data/treatment_effects_reference.py`), and R
//! MatchIt 4.5 with sandwich 3.1 for ATT matching
//! (`tests/data/treatment_effects_matchit.R`). Data: the invented
//! cardiac-rehabilitation study in `app-model/tests/data`.

use stat_core::Matrix;
use stat_data::{JsonValue, parse_json};
use stat_discrete::treatment_effects::{
    EffectMethod, EffectScale, Estimand, MatchingOptions, OutcomeKind, TreatmentEffectOptions, covariate_balance,
    estimate_treatment_effect, fit_propensity,
};

struct Data {
    names: Vec<String>,
    x: Matrix,
    t: Vec<f64>,
    reference: JsonValue,
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn f(v: &JsonValue, key: &str) -> f64 {
    v.get(key).unwrap_or_else(|| panic!("no {key}")).as_f64().unwrap()
}

fn data() -> Data {
    let r = parse_json(include_str!("data/treatment_effects_reference.json")).unwrap();
    let rows: Vec<Vec<f64>> = r.get("x").unwrap().as_array().unwrap().iter().map(nums).collect();
    Data {
        names: r.get("names").unwrap().as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect(),
        x: Matrix::from_rows(&rows).unwrap(),
        t: nums(r.get("t").unwrap()),
        reference: r,
    }
}

fn close(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol * b.abs().max(1.0), "{what}: {a} vs {b}");
}

#[test]
fn propensity_model_matches_statsmodels() {
    let d = data();
    let p = fit_propensity(&d.x, &d.names, &d.t).unwrap();
    for (c, want) in p.fit.coefficients.iter().zip(nums(d.reference.get("propensity").unwrap())) {
        close(c.estimate, want, 1e-8, &c.name);
    }
}

#[test]
fn weighting_and_doubly_robust_match_the_reference() {
    let d = data();
    let cases = d.reference.get("cases").unwrap().as_array().unwrap();
    assert_eq!(cases.len(), 14);
    for c in cases {
        let kind = OutcomeKind::from_key(c.get("kind").unwrap().as_str().unwrap()).unwrap();
        let method = match c.get("method").unwrap().as_str().unwrap() {
            "ipw" => EffectMethod::Weighting,
            _ => EffectMethod::DoublyRobust,
        };
        let estimand = Estimand::from_key(c.get("estimand").unwrap().as_str().unwrap()).unwrap();
        let y = nums(d.reference.get("y").unwrap().get(kind.key()).unwrap());
        let options =
            TreatmentEffectOptions { method, estimand, trim: f(c, "trim"), ..TreatmentEffectOptions::new(kind) };
        let fit = estimate_treatment_effect(&d.x, &d.names, &d.t, &y, &options).unwrap();
        let what = format!("{} {} {} trim {}", kind.key(), method.key(), estimand.key(), f(c, "trim"));
        close(fit.mean_treated, f(c, "m1"), 1e-8, &what);
        close(fit.mean_control, f(c, "m0"), 1e-8, &what);
        assert_eq!(fit.n_trimmed, f(c, "n_trimmed") as usize, "{what}");
        let cov: Vec<Vec<f64>> = c.get("cov").unwrap().as_array().unwrap().iter().map(nums).collect();
        for r in 0..2 {
            for k in 0..2 {
                // Central differences against complex-step derivatives.
                close(fit.covariance[r][k], cov[r][k], 1e-6, &format!("{what} cov"));
            }
        }
        let effects = c.get("effects").unwrap();
        for e in &fit.effects {
            let key = match e.scale {
                EffectScale::Difference => "difference",
                EffectScale::Ratio => "log_ratio",
                EffectScale::OddsRatio => "log_odds_ratio",
            };
            let want = nums(effects.get(key).unwrap());
            close(e.analysis_estimate(), want[0], 1e-8, &format!("{what} {key}"));
            close(e.std_error, want[1], 1e-6, &format!("{what} {key} se"));
        }
        assert_eq!(fit.effects.len(), EffectScale::for_outcome(kind).len());
    }
}

#[test]
fn ipw_standard_error_agrees_with_the_closed_form() {
    let d = data();
    let y = nums(d.reference.get("y").unwrap().get("continuous").unwrap());
    let fit =
        estimate_treatment_effect(&d.x, &d.names, &d.t, &y, &TreatmentEffectOptions::new(OutcomeKind::Continuous))
            .unwrap();
    // The closed form uses population identities (E[t/e] = 1), so agreement
    // is asymptotic: within 1% here.
    close(fit.effects[0].std_error, f(&d.reference, "ld_ipw_ate_se"), 0.01, "Lunceford–Davidian SE");
}

#[test]
fn ate_matching_with_replacement_matches_brute_force() {
    let d = data();
    let y = nums(d.reference.get("y").unwrap().get("continuous").unwrap());
    for c in d.reference.get("ate_matching").unwrap().as_array().unwrap() {
        let ratio = f(c, "ratio") as usize;
        let options = TreatmentEffectOptions {
            method: EffectMethod::Matching(MatchingOptions { ratio, replace: true, caliper: None }),
            estimand: Estimand::Ate,
            ..TreatmentEffectOptions::new(OutcomeKind::Continuous)
        };
        let fit = estimate_treatment_effect(&d.x, &d.names, &d.t, &y, &options).unwrap();
        close(fit.effects[0].estimate, f(c, "ate"), 1e-10, &format!("ATE matching 1:{ratio}"));
    }
}

#[test]
fn att_matching_matches_matchit() {
    let d = data();
    let y = nums(d.reference.get("y").unwrap().get("continuous").unwrap());
    let r = parse_json(include_str!("data/treatment_effects_matchit.json")).unwrap();
    for c in r.get("cases").unwrap().as_array().unwrap() {
        let m = MatchingOptions {
            ratio: f(c, "ratio") as usize,
            replace: c.get("replace").unwrap().as_bool().unwrap(),
            caliper: c.get("caliper").and_then(JsonValue::as_f64),
        };
        let options = TreatmentEffectOptions {
            method: EffectMethod::Matching(m),
            estimand: Estimand::Att,
            ..TreatmentEffectOptions::new(OutcomeKind::Continuous)
        };
        let fit = estimate_treatment_effect(&d.x, &d.names, &d.t, &y, &options).unwrap();
        let what = format!("{m:?}");
        for (i, (got, want)) in fit.weights.iter().zip(nums(c.get("weights").unwrap())).enumerate() {
            close(*got, want, 1e-9, &format!("{what} weight of row {i}"));
        }
        close(fit.effects[0].estimate, f(c, "estimate"), 1e-9, &what);
        close(fit.effects[0].std_error, f(c, "se"), 1e-7, &format!("{what} se"));
    }
}

#[test]
fn balance_matches_numpy() {
    let d = data();
    let vars: Vec<(String, Vec<f64>)> =
        d.names.iter().enumerate().map(|(j, n)| (n.clone(), d.x.col(j).to_vec())).collect();
    let e = fit_propensity(&d.x, &d.names, &d.t).unwrap().scores;
    let n = d.t.len();
    for c in d.reference.get("balance").unwrap().as_array().unwrap() {
        let estimand = Estimand::from_key(c.get("estimand").unwrap().as_str().unwrap()).unwrap();
        let w: Vec<f64> = (0..n)
            .map(|i| match estimand {
                Estimand::Ate => d.t[i] / e[i] + (1.0 - d.t[i]) / (1.0 - e[i]),
                Estimand::Att => d.t[i] + (1.0 - d.t[i]) * e[i] / (1.0 - e[i]),
            })
            .collect();
        let name = c.get("name").unwrap().as_str().unwrap();
        let rows = covariate_balance(&vars, &d.t, &w, estimand);
        let b = rows.iter().find(|b| b.name == name).unwrap();
        close(b.smd, f(c, "smd"), 1e-10, name);
        close(b.adjusted_smd, f(c, "adjusted_smd"), 1e-8, name);
        match c.get("variance_ratio").and_then(JsonValue::as_f64) {
            Some(v) => {
                close(b.variance_ratio.unwrap(), v, 1e-10, name);
                close(b.adjusted_variance_ratio.unwrap(), f(c, "adjusted_variance_ratio"), 1e-8, name);
            }
            None => assert!(b.binary && b.variance_ratio.is_none(), "{name}"),
        }
    }
}
