//! Warranty-return fits and forecasts compared with an independent SciPy
//! implementation (`tests/data/warranty_reference.py`, which writes
//! `warranty_reference.json`) that builds the interval-censored likelihood
//! directly from the shipment and return counts. The counts are invented for
//! the tests.

use stat_core::distribution_fit::FitFamily;
use stat_data::{JsonValue, parse_json};
use stat_reliability::fit_censored;
use stat_reliability::warranty::{WarrantyCohort, WarrantyData, forecast_returns};

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

#[test]
fn fits_and_forecasts_match_reference() {
    let all = parse_json(include_str!("data/warranty_reference.json")).unwrap();
    let shipped = nums(all.get("shipped").unwrap());
    let returns: Vec<Vec<f64>> = all.get("returns").unwrap().as_array().unwrap().iter().map(nums).collect();
    let periods = num(all.get("periods").unwrap()) as usize;
    let planned = nums(all.get("planned").unwrap());
    let horizon = num(all.get("horizon").unwrap()) as usize;
    let cohorts: Vec<WarrantyCohort> = shipped
        .iter()
        .zip(&returns)
        .enumerate()
        .map(|(s, (&n, r))| WarrantyCohort { period: s, shipped: n, returns: r.clone() })
        .collect();
    let JsonValue::Object(cases) = all.get("cases").unwrap() else { panic!() };
    assert_eq!(cases.len(), 3);
    for (name, case) in cases {
        let warranty = case.get("warranty").and_then(JsonValue::as_f64);
        let seen: Vec<Vec<f64>> = case.get("returns").unwrap().as_array().unwrap().iter().map(nums).collect();
        let cohorts: Vec<WarrantyCohort> =
            cohorts.iter().zip(seen).map(|(c, r)| WarrantyCohort { returns: r, ..c.clone() }).collect();
        let data = WarrantyData::new(cohorts, periods, warranty).unwrap();
        let family = match case.get("family").unwrap().as_str().unwrap() {
            "weibull" => FitFamily::Weibull,
            _ => FitFamily::Lognormal,
        };
        let fit = fit_censored(&data.life_data().unwrap(), family).unwrap();
        close(fit.log_likelihood(), num(case.get("log_likelihood").unwrap()), 1e-9, &format!("{name} log-likelihood"));
        let params = case.get("params").unwrap();
        let d = &fit.distribution;
        if family == FitFamily::Weibull {
            close(d.scale.recip(), num(params.get("shape").unwrap()), 1e-5, &format!("{name} shape"));
            close(d.location.exp(), num(params.get("scale").unwrap()), 1e-5, &format!("{name} scale"));
        } else {
            close(d.location, num(params.get("mu").unwrap()), 1e-5, &format!("{name} mu"));
            close(d.scale, num(params.get("sigma").unwrap()), 1e-5, &format!("{name} sigma"));
        }
        let f = forecast_returns(&data, &fit, horizon, &planned, 0.95).unwrap();
        let reference = case.get("forecast").unwrap();
        for (key, got) in [("periods", &f.periods), ("cumulative", &f.cumulative)] {
            let want = reference.get(key).unwrap().as_array().unwrap();
            assert_eq!(got.len(), want.len());
            for (g, w) in got.iter().zip(want) {
                let what = format!("{name} {key} {}", g.period);
                close(g.expected, num(w.get("expected").unwrap()), 1e-5, &format!("{what} expected"));
                close(
                    g.parameter_std_error,
                    num(w.get("parameter_se").unwrap()),
                    2e-3,
                    &format!("{what} parameter SE"),
                );
                close(g.std_error, num(w.get("std_error").unwrap()), 1e-3, &format!("{what} SE"));
                close(g.upper, num(w.get("upper").unwrap()), 1e-4, &format!("{what} upper"));
            }
        }
    }
}
