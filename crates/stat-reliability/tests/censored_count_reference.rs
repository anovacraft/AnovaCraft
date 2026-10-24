//! Poisson and negative binomial fits to censored counts compared with an
//! independent SciPy maximum likelihood implementation
//! (`tests/data/censored_count_reference.py`, which writes
//! `censored_count_reference.json`).

use stat_data::{JsonValue, parse_json};
use stat_reliability::censored_count::{CountFamily, CountObservation, fit_censored_counts};

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
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
fn censored_counts_match_scipy() {
    let r = parse_json(include_str!("data/censored_count_reference.json")).unwrap();
    let level = num(r.get("level").unwrap());
    for case in r.get("cases").unwrap().as_array().unwrap() {
        let name = case.get("name").unwrap().as_str().unwrap();
        let family = CountFamily::from_key(case.get("family").unwrap().as_str().unwrap()).unwrap();
        let obs: Vec<CountObservation> = case
            .get("data")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                let o = o.as_array().unwrap();
                CountObservation {
                    lower: num(&o[0]),
                    upper: o[1].as_f64().unwrap_or(f64::INFINITY),
                    weight: num(&o[2]),
                }
            })
            .collect();
        let fit = fit_censored_counts(&obs, family, level).unwrap();
        close(fit.log_likelihood, num(case.get("log_likelihood").unwrap()), 1e-10, &format!("{name} log-likelihood"));
        close(fit.aic, num(case.get("aic").unwrap()), 1e-10, &format!("{name} AIC"));
        close(fit.bic, num(case.get("bic").unwrap()), 1e-10, &format!("{name} BIC"));
        close(fit.n, num(case.get("n").unwrap()), 1e-15, &format!("{name} n"));
        let params = case.get("parameters").unwrap().as_array().unwrap();
        assert_eq!(params.len(), fit.parameters.len());
        for (got, want) in fit.parameters.iter().zip(params) {
            let what = format!("{name} {}", want.get("name").unwrap().as_str().unwrap());
            close(got.estimate, num(want.get("estimate").unwrap()), 1e-6, &what);
            close(got.std_error, num(want.get("std_error").unwrap()), 1e-4, &format!("{what} standard error"));
            let wald = want.get("wald").unwrap().as_array().unwrap();
            close(got.wald.0, num(&wald[0]), 1e-4, &format!("{what} Wald lower"));
            close(got.wald.1, num(&wald[1]), 1e-4, &format!("{what} Wald upper"));
            let lr = want.get("likelihood_ratio").unwrap().as_array().unwrap();
            let (lo, hi) = got.likelihood_ratio.unwrap();
            close(lo, num(&lr[0]), 1e-6, &format!("{what} likelihood-ratio lower"));
            match lr[1].as_f64() {
                Some(w) => close(hi, w, 1e-6, &format!("{what} likelihood-ratio upper")),
                None => assert!(hi.is_infinite(), "{what}: upper limit should be unbounded, got {hi}"),
            }
        }
        for (k, want) in case.get("pmf").unwrap().as_array().unwrap().iter().enumerate() {
            close(fit.pmf(k as u64), num(want), 1e-5, &format!("{name} pmf({k})"));
        }
        let cdf = case.get("cdf").unwrap();
        close(fit.cdf(3), num(cdf.get("3").unwrap()), 1e-5, &format!("{name} cdf(3)"));
        close(fit.cdf(10), num(cdf.get("10").unwrap()), 1e-5, &format!("{name} cdf(10)"));
    }
}

/// The Poisson is the limit of the negative binomial: on Poisson-like
/// counts the size estimate runs off and the fit says so.
#[test]
fn underdispersed_counts_reject_the_negative_binomial() {
    let obs: Vec<CountObservation> =
        [4u64, 5, 5, 5, 6, 6, 4, 5].iter().map(|&c| CountObservation::exact(c, 1.0)).collect();
    assert!(fit_censored_counts(&obs, CountFamily::NegativeBinomial, 0.95).is_err());
    assert!(fit_censored_counts(&obs, CountFamily::Poisson, 0.95).is_ok());
}
