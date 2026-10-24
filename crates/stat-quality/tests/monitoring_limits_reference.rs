//! Environmental monitoring fits and limits checked against R 4.3
//! (`tests/data/monitoring_limits_reference.R`): `MASS::fitdistr` for the
//! Poisson, negative binomial and continuous families, `pscl::zeroinfl` for the
//! zero-inflated forms, and `optim` on the beta-binomial likelihood.

use stat_core::distribution_fit::FitFamily;
use stat_data::{JsonValue, parse_json};
use stat_quality::{CountFamily, ReadingKind, best_candidate, fit_candidates, fit_counts, fit_positives};

fn cases() -> JsonValue {
    parse_json(include_str!("data/monitoring_limits_reference.json")).unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-9), "{what}: {a} vs {b}");
}

const QS: [f64; 2] = [0.975, 0.9985];

#[test]
fn negative_binomial_and_poisson_match_fitdistr() {
    let c = cases();
    let c = c.get("nb").unwrap();
    let f = |k: &str| c.get(k).unwrap().as_f64().unwrap();
    let x = nums(c.get("x").unwrap());
    let pois = fit_counts(&x, CountFamily::Poisson).unwrap();
    close(pois.mean, f("poisson_lambda"), 1e-12, "λ");
    close(pois.log_likelihood, f("poisson_loglik"), 1e-10, "Poisson ℓ");
    let nb = fit_counts(&x, CountFamily::NegativeBinomial).unwrap();
    close(nb.size, f("nb_size"), 1e-4, "size");
    close(nb.mean, f("nb_mu"), 1e-5, "μ");
    close(nb.log_likelihood, f("nb_loglik"), 1e-8, "NB ℓ");
    for (q, (want_nb, want_p)) in
        QS.iter().zip(nums(c.get("nb_quantiles").unwrap()).into_iter().zip(nums(c.get("poisson_quantiles").unwrap())))
    {
        assert_eq!(nb.quantile(*q, 0.0), want_nb, "NB quantile {q}");
        assert_eq!(pois.quantile(*q, 0.0), want_p, "Poisson quantile {q}");
    }
    // These overdispersed counts have more zeros than even the negative
    // binomial expects: the zero-inflated form has the smallest AIC, as in R.
    let candidates = fit_candidates(&x, &[], ReadingKind::Counts);
    let best = best_candidate(&candidates).unwrap();
    assert_eq!(candidates[best].label, "Zero-inflated negative binomial");
    close(candidates[best].fit.as_ref().unwrap().aic(), f("zinb_aic"), 1e-7, "ZINB AIC");
}

#[test]
fn zero_inflated_fits_match_zeroinfl() {
    let all = cases();
    let c = all.get("zip").unwrap();
    let f = |k: &str| c.get(k).unwrap().as_f64().unwrap();
    let x = nums(c.get("x").unwrap());
    let zip = fit_counts(&x, CountFamily::ZeroInflatedPoisson).unwrap();
    // The estimates solve the likelihood equations exactly; zeroinfl stops
    // its quasi-Newton search a little short, so its likelihood is no higher.
    close(zip.mean, f("lambda"), 1e-4, "λ");
    close(zip.zero_inflation, f("pi"), 1e-4, "π");
    close(zip.log_likelihood, f("loglik"), 1e-9, "ZIP ℓ");
    assert!(zip.log_likelihood >= f("loglik") - 1e-12);
    let q: Vec<f64> = QS.iter().map(|&q| zip.quantile(q, 0.0)).collect();
    assert_eq!(q, nums(c.get("quantiles").unwrap()));

    let c = all.get("zinb").unwrap();
    let f = |k: &str| c.get(k).unwrap().as_f64().unwrap();
    let x = nums(c.get("x").unwrap());
    let zinb = fit_counts(&x, CountFamily::ZeroInflatedNegativeBinomial).unwrap();
    close(zinb.mean, f("mu"), 1e-4, "μ");
    close(zinb.size, f("size"), 1e-3, "size");
    close(zinb.zero_inflation, f("pi"), 1e-4, "π");
    close(zinb.log_likelihood, f("loglik"), 1e-8, "ZINB ℓ");
    let q: Vec<f64> = QS.iter().map(|&q| zinb.quantile(q, 0.0)).collect();
    assert_eq!(q, nums(c.get("quantiles").unwrap()));
}

#[test]
fn binomial_and_beta_binomial_match_r() {
    let c = cases();
    let c = c.get("bb").unwrap();
    let f = |k: &str| c.get(k).unwrap().as_f64().unwrap();
    let (x, n) = (nums(c.get("x").unwrap()), nums(c.get("trials").unwrap()));
    let bin = fit_positives(&x, &n, CountFamily::Binomial).unwrap();
    close(bin.mean, f("binomial_p"), 1e-12, "p");
    close(bin.log_likelihood, f("binomial_loglik"), 1e-10, "binomial ℓ");
    let bb = fit_positives(&x, &n, CountFamily::BetaBinomial).unwrap();
    close(bb.mean, f("mu"), 1e-4, "μ");
    close(bb.size, f("size"), 2e-3, "a + b");
    close(bb.log_likelihood, f("loglik"), 1e-8, "beta-binomial ℓ");
    let q: Vec<f64> = QS.iter().map(|&q| bb.quantile(q, 10.0)).collect();
    assert_eq!(q, nums(c.get("quantiles_n10").unwrap()));
    let q: Vec<f64> = QS.iter().map(|&q| bin.quantile(q, 10.0)).collect();
    assert_eq!(q, nums(c.get("binomial_quantiles_n10").unwrap()));
}

#[test]
fn continuous_families_match_fitdistr() {
    let c = cases();
    let c = c.get("cont").unwrap();
    let x = nums(c.get("x").unwrap());
    let candidates = fit_candidates(&x, &[], ReadingKind::Continuous);
    for (family, key) in
        [(FitFamily::Gamma, "gamma"), (FitFamily::Lognormal, "lognormal"), (FitFamily::Weibull, "weibull")]
    {
        let fit = candidates.iter().find(|k| k.label == family.label()).unwrap().fit.as_ref().unwrap();
        close(fit.log_likelihood(), c.get(&format!("{key}_loglik")).unwrap().as_f64().unwrap(), 1e-7, key);
        for (q, want) in QS.iter().zip(nums(c.get(&format!("{key}_quantiles")).unwrap())) {
            close(fit.quantile(*q, 0.0), want, 1e-4, &format!("{key} quantile {q}"));
        }
    }
}
