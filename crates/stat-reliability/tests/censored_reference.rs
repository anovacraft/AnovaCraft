//! Censored life-distribution fits, Turnbull estimates and the censored
//! Anderson–Darling distance compared with independent reference values
//! computed with SciPy and NumPy (`tests/data/censored_reference.py`, which
//! writes `censored_reference.json`). The datasets are invented for the tests.

use stat_core::distribution_fit::FitFamily;
use stat_data::{JsonValue, parse_json};
use stat_reliability::{
    LifeData, LifeObservation, censored_anderson_darling, fit_censored, turnbull, weibull_known_shape,
};

fn cases() -> JsonValue {
    parse_json(include_str!("data/censored_reference.json")).unwrap()
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

fn data(case: &JsonValue) -> LifeData {
    let obs = case
        .get("obs")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            let o = o.as_array().unwrap();
            LifeObservation::interval(end(&o[0], f64::NEG_INFINITY), end(&o[1], f64::INFINITY), num(&o[2]))
        })
        .collect();
    LifeData::new(obs).unwrap()
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-12);
    assert!(
        (got - want).abs() <= rel * scale || (got.is_infinite() && got == want),
        "{what}: got {got}, reference {want} (relative error {:e})",
        (got - want).abs() / scale
    );
}

#[test]
fn censored_fits_match_reference() {
    let all = cases();
    let mut checked = 0;
    for name in ["right", "left", "interval", "mixed", "threshold"] {
        let case = all.get(name).unwrap();
        let d = data(case);
        let JsonValue::Object(fits) = case.get("fits").unwrap() else { panic!() };
        for (key, want) in fits {
            let family = FitFamily::from_key(key).unwrap();
            let fit = fit_censored(&d, family).unwrap_or_else(|e| panic!("{name} {key}: {e}"));
            let label = format!("{name} {key}");
            close(fit.log_likelihood(), num(want.get("loglik").unwrap()), 1e-9, &format!("{label} log-likelihood"));
            let lr = want.get("param_lr").is_some();
            let params = fit.parameters(0.95, lr);
            let est = nums(want.get("params").unwrap());
            let se = nums(want.get("se").unwrap());
            for (i, p) in params.iter().enumerate() {
                close(p.estimate, est[i], 1e-6, &format!("{label} {}", p.name));
                close(p.std_error, se[i], 2e-4, &format!("{label} SE {}", p.name));
            }
            if let Some(limits) = want.get("param_lr") {
                for (i, lim) in limits.as_array().unwrap().iter().enumerate() {
                    let lim = nums(lim);
                    let got = params[i].likelihood_ratio.unwrap();
                    close(got.0, lim[0], 1e-6, &format!("{label} LR lower {}", params[i].name));
                    close(got.1, lim[1], 1e-6, &format!("{label} LR upper {}", params[i].name));
                }
                let JsonValue::Object(qs) = want.get("quantiles").unwrap() else { panic!() };
                for (p, v) in qs {
                    let p: f64 = p.parse().unwrap();
                    let v = nums(v);
                    let q = fit.quantile(p, 0.95, true);
                    close(q.estimate, v[0], 1e-6, &format!("{label} quantile {p}"));
                    let (lo, hi) = q.likelihood_ratio.unwrap();
                    close(lo, v[1], 1e-6, &format!("{label} quantile {p} LR lower"));
                    close(hi, v[2], 1e-6, &format!("{label} quantile {p} LR upper"));
                }
                let JsonValue::Object(fs) = want.get("probabilities").unwrap() else { panic!() };
                for (t, v) in fs {
                    let t: f64 = t.parse().unwrap();
                    let v = nums(v);
                    let f = fit.failure_probability(t, 0.95, true);
                    close(f.estimate, v[0], 1e-6, &format!("{label} F({t})"));
                    let (lo, hi) = f.likelihood_ratio.unwrap();
                    close(lo, v[1], 1e-6, &format!("{label} F({t}) LR lower"));
                    close(hi, v[2], 1e-6, &format!("{label} F({t}) LR upper"));
                }
            }
            checked += 1;
        }
    }
    assert!(checked >= 39, "{checked} fits checked");
}

#[test]
fn turnbull_and_anderson_darling_match_reference() {
    let all = cases();
    for name in ["right", "left", "interval", "mixed", "threshold"] {
        let case = all.get(name).unwrap();
        let d = data(case);
        let est = turnbull(&d).unwrap();
        let want = case.get("turnbull").unwrap().as_array().unwrap();
        assert_eq!(est.intervals.len(), want.len(), "{name}: innermost intervals");
        for (iv, w) in est.intervals.iter().zip(want) {
            let w = w.as_array().unwrap();
            assert_eq!(iv.lower, end(&w[0], f64::NEG_INFINITY), "{name}");
            assert_eq!(iv.upper, end(&w[1], f64::INFINITY), "{name}");
            assert!((iv.probability - num(&w[3])).abs() < 1e-7, "{name}: mass {} vs {}", iv.probability, num(&w[3]));
        }
        if let Some(ad) = case.get("ad_weibull") {
            let fit = fit_censored(&d, FitFamily::Weibull).unwrap();
            close(censored_anderson_darling(&est, &fit.distribution), num(ad), 1e-6, &format!("{name} AD"));
        }
    }
}

#[test]
fn exponential_threshold_at_its_boundary() {
    let case = cases();
    let case = case.get("threshold").unwrap();
    let want = case.get("exponential_boundary").unwrap();
    let fit = fit_censored(&data(case), FitFamily::Exponential2).unwrap();
    assert!(fit.threshold_at_boundary);
    close(fit.distribution.threshold, num(want.get("threshold").unwrap()), 1e-12, "threshold");
    close(fit.distribution.scale, num(want.get("scale").unwrap()), 1e-7, "scale");
    close(fit.log_likelihood(), num(want.get("loglik").unwrap()), 1e-9, "log-likelihood");
    let p = fit.parameters(0.95, true);
    assert!(p[1].std_error.is_nan() && p[1].likelihood_ratio.is_none());
    assert!(p[0].std_error > 0.0);
}

#[test]
fn known_shape_weibull_matches_closed_form() {
    let all = cases();
    for case in all.get("known_shape").unwrap().as_array().unwrap() {
        let times = nums(case.get("times").unwrap());
        let events = nums(case.get("events").unwrap());
        let obs = times
            .iter()
            .zip(&events)
            .map(|(&t, &e)| if e > 0.0 { LifeObservation::exact(t, 1.0) } else { LifeObservation::right(t, 1.0) })
            .collect();
        let d = LifeData::new(obs).unwrap();
        let shape = num(case.get("shape").unwrap());
        let k = weibull_known_shape(&d, shape, 0.95).unwrap();
        match case.get("scale").unwrap().as_f64() {
            Some(s) => close(k.scale, s, 1e-12, "scale"),
            None => assert!(k.scale.is_nan()),
        }
        close(k.scale_interval.0, num(case.get("lower").unwrap()), 1e-9, "lower");
        match case.get("upper").unwrap().as_f64() {
            Some(u) => close(k.scale_interval.1, u, 1e-9, "upper"),
            None => assert_eq!(k.scale_interval.1, f64::INFINITY),
        }
        close(k.scale_lower_bound, num(case.get("bound").unwrap()), 1e-9, "bound");
        close(k.quantile_lower_bound(0.1), num(case.get("b10_bound").unwrap()), 1e-9, "B10 bound");
        close(k.reliability_lower_bound(400.0), num(case.get("reliability_bound_400").unwrap()), 1e-9, "R(400)");
    }
}
