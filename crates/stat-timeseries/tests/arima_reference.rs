//! Seasonal ARIMA fits, exact likelihoods, forecasts and the automatic order
//! search compared with R's `stats::arima(method = "ML")` and the forecast
//! package (`auto.arima`), used as black-box reference software
//! (`tests/data/arima_reference.R` writes `arima_reference.json`). The series
//! are classic public data sets from R's `datasets` package.
//!
//! R starts the differenced part of the state with a large finite variance
//! `kappa` (default 10⁶, in units of σ²) instead of the exact diffuse limit
//! used here. The reference script therefore runs R with `kappa = 1e10`,
//! within about 10⁻⁶ of the limit (larger values lose precision in R); at the
//! default the log-likelihood of a differenced model differs by up to a few
//! 10⁻³ (also checked). Standard errors come from R's default fit, whose
//! finite-difference Hessian is less noisy.

use stat_data::{JsonValue, parse_json};
use stat_timeseries::arima::{
    ArimaFit, ArimaOptions, ArimaSpec, AutoArimaOptions, Regressor, auto_arima, evaluate_arima, fit_arima,
};

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

/// A numeric vector that jsonlite may have written as a bare scalar.
fn nums(v: &JsonValue) -> Vec<f64> {
    match v.as_array() {
        Some(a) => a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect(),
        None => vec![num(v)],
    }
}

fn close(got: f64, want: f64, rel: f64, abs: f64, what: &str) {
    let tol = abs.max(rel * want.abs());
    assert!((got - want).abs() <= tol, "{what}: got {got}, reference {want} (difference {:e})", (got - want).abs());
}

fn reference() -> JsonValue {
    parse_json(include_str!("data/arima_reference.json")).unwrap()
}

struct Case {
    name: String,
    y: Vec<f64>,
    spec: ArimaSpec,
    xreg: Vec<Regressor>,
    newxreg: Vec<Vec<f64>>,
    case: JsonValue,
}

fn cases() -> Vec<Case> {
    let all = reference();
    let JsonValue::Object(cases) = all.get("cases").unwrap().clone() else { panic!() };
    cases
        .into_iter()
        .map(|(name, case)| {
            let o = nums(case.get("order").unwrap());
            let s = nums(case.get("seasonal").unwrap());
            let period = num(case.get("period").unwrap()) as usize;
            let constant =
                case.get("mean").unwrap().as_bool().unwrap() || case.get("drift").unwrap().as_bool().unwrap();
            let mut spec = ArimaSpec::new(o[0] as usize, o[1] as usize, o[2] as usize).with_constant(constant);
            if s.iter().any(|&v| v > 0.0) {
                spec = spec.seasonal(s[0] as usize, s[1] as usize, s[2] as usize, period);
            }
            let xreg: Vec<Regressor> = case
                .get("xreg")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|c| Regressor { name: "trend".into(), values: nums(c) })
                .collect();
            let newxreg: Vec<Vec<f64>> = case.get("newxreg").unwrap().as_array().unwrap().iter().map(nums).collect();
            Case { name, y: nums(case.get("y").unwrap()), spec, xreg, newxreg, case }
        })
        .collect()
}

fn check_forecasts(c: &Case, fit: &ArimaFit, rel: f64) {
    let h = num(c.case.get("horizon").unwrap()) as usize;
    let fc = fit.forecast(h, &c.newxreg, &[0.95]).unwrap();
    let pred = nums(c.case.get("pred").unwrap());
    let se = nums(c.case.get("pred_se").unwrap());
    for k in 0..h {
        close(fc.mean[k], pred[k], rel, 1e-12, &format!("{} forecast {k}", c.name));
        close(fc.std_error[k], se[k], rel, 1e-12, &format!("{} forecast se {k}", c.name));
        let half = fc.intervals[0].upper[k] - fc.mean[k];
        close(half, 1.959_963_984_540_054 * fc.std_error[k], 1e-9, 1e-12, "interval half-width");
    }
}

/// The exact likelihood, σ², residuals and forecasts at R's estimates held
/// fixed. Without differencing the likelihood is the same function as R's,
/// so it agrees to rounding (1e-9); with differencing R's kappa leaves a
/// difference below 1e-5 in ℓ and 1e-6 relative in σ² and the forecasts.
#[test]
fn likelihood_and_forecasts_at_reference_coefficients() {
    for c in cases() {
        let coef = nums(c.case.get("coef").unwrap());
        let fit = evaluate_arima(&c.y, &c.spec, &c.xreg, &coef).unwrap();
        assert_eq!(fit.coefficient_names.len(), coef.len());
        let differenced = c.spec.differencing_lag() > 0;
        let (ll_tol, rel) = if differenced { (1e-5, 1e-6) } else { (1e-9, 1e-9) };
        let ll = num(c.case.get("fixed_loglik").unwrap());
        close(fit.log_likelihood, ll, 0.0, ll_tol, &format!("{} loglik", c.name));
        let ll_default = num(c.case.get("fixed_loglik_default_kappa").unwrap());
        let default_tol = if differenced { 5e-3 } else { 1e-9 };
        close(fit.log_likelihood, ll_default, 0.0, default_tol, &format!("{} loglik, default kappa", c.name));
        close(fit.sigma2, num(c.case.get("fixed_sigma2").unwrap()), rel, 0.0, &format!("{} sigma2", c.name));
        check_forecasts(&c, &fit, if differenced { 1e-7 } else { 1e-9 });
        // Residuals after the differencing start-up; R's first d + mD
        // residuals come from its finite prior and are not compared.
        let res = nums(c.case.get("residuals").unwrap());
        let nd = c.spec.differencing_lag();
        let scale = fit.sigma2.sqrt();
        for t in nd..res.len() {
            close(fit.residuals[t], res[t], 0.0, 1e-6 * scale, &format!("{} residual {t}", c.name));
        }
        assert!(fit.residuals[..nd].iter().all(|v| v.is_nan()));
    }
}

/// Maximum likelihood estimates. Our maximum must be at least as high as the
/// exact likelihood at R's estimates; the log-likelihoods agree to 1e-5 (R's
/// kappa and optimiser tolerance), the estimates to 1% of a standard error,
/// and the standard errors to 1% (R's Hessian uses a fixed step of 10⁻³,
/// coarse for a small coefficient such as a drift).
#[test]
fn maximum_likelihood_fits_match_reference() {
    for c in cases() {
        let fit = fit_arima(&c.y, &c.spec, &c.xreg, &ArimaOptions::default()).unwrap();
        let coef = nums(c.case.get("coef").unwrap());
        let at_reference = evaluate_arima(&c.y, &c.spec, &c.xreg, &coef).unwrap();
        assert!(
            fit.log_likelihood >= at_reference.log_likelihood - 1e-8,
            "{}: maximum {} below the likelihood at R's estimates {}",
            c.name,
            fit.log_likelihood,
            at_reference.log_likelihood
        );
        let ll = num(c.case.get("loglik").unwrap());
        close(fit.log_likelihood, ll, 0.0, 1e-5, &format!("{} loglik", c.name));
        let se = nums(c.case.get("se").unwrap());
        for (k, (&want, &want_se)) in coef.iter().zip(&se).enumerate() {
            close(fit.coefficients[k], want, 0.0, 1e-2 * want_se, &format!("{} coefficient {k}", c.name));
            close(fit.std_errors[k], want_se, 1e-2, 0.0, &format!("{} se {k}", c.name));
        }
        close(fit.sigma2, num(c.case.get("sigma2").unwrap()), 1e-4, 0.0, &format!("{} sigma2", c.name));
        close(fit.aic, num(c.case.get("aic").unwrap()), 0.0, 2e-5, &format!("{} aic", c.name));
        assert!(fit.converged, "{}", c.name);
    }
}

/// Box & Jenkins' airline model for log(AirPassengers): published estimates
/// θ ≈ 0.40, Θ ≈ 0.61 (their sign convention 1 − θB). The exact likelihood
/// estimate of Θ is 0.557 (R gives the same); the published 0.61 is a
/// least-squares (back-forecast) estimate, so only θ is held tightly.
#[test]
fn airline_model_matches_published_estimates() {
    let c = cases().into_iter().find(|c| c.name == "airline").unwrap();
    let fit = fit_arima(&c.y, &c.spec, &[], &ArimaOptions::default()).unwrap();
    assert!((-fit.ma[0] - 0.40).abs() < 0.01, "{:?}", fit.ma);
    assert!((-fit.seasonal_ma[0] - 0.61).abs() < 0.06, "{:?}", fit.seasonal_ma);
    assert_eq!(fit.coefficient_names, ["ma1", "sma1"]);
}

fn auto_cases() -> Vec<(String, JsonValue)> {
    let JsonValue::Object(a) = reference().get("auto").unwrap().clone() else { panic!() };
    a.into_iter().collect()
}

/// The stepwise search with the differencing orders given selects the same
/// model as `auto.arima(..., d, D, stepwise = TRUE, approximation = FALSE)`;
/// AICc agrees to 1e-2 (R uses its default kappa).
#[test]
fn stepwise_search_with_given_differencing_matches_reference() {
    for (name, a) in auto_cases() {
        let y = nums(a.get("y").unwrap());
        let mut opts = AutoArimaOptions::new(num(a.get("period").unwrap()) as usize);
        opts.d = Some(num(a.get("d").unwrap()) as usize);
        opts.seasonal_d = Some(num(a.get("D").unwrap()) as usize);
        let res = auto_arima(&y, &[], &opts).unwrap();
        let o: Vec<usize> = nums(a.get("order").unwrap()).iter().map(|&v| v as usize).collect();
        let s = res.fit.spec;
        assert_eq!(
            (s.p, s.d, s.q, s.seasonal_p, s.seasonal_d, s.seasonal_q),
            (o[0], o[1], o[2], o[3], o[4], o[5]),
            "{name}: selected {}",
            s.label()
        );
        assert_eq!(s.constant, a.get("constant").unwrap().as_bool().unwrap(), "{name}");
        close(res.fit.aicc, num(a.get("aicc").unwrap()), 0.0, 1e-2, &format!("{name} AICc"));
    }
}

/// The full automatic procedure, choosing D by seasonal strength and d by
/// KPSS tests, against `auto.arima` with free differencing orders.
#[test]
fn automatic_differencing_and_search_match_reference() {
    for (name, a) in auto_cases() {
        let y = nums(a.get("y").unwrap());
        let opts = AutoArimaOptions::new(num(a.get("period").unwrap()) as usize);
        let res = auto_arima(&y, &[], &opts).unwrap();
        let o: Vec<usize> = nums(a.get("free_order").unwrap()).iter().map(|&v| v as usize).collect();
        let s = res.fit.spec;
        assert_eq!(
            (s.p, s.d, s.q, s.seasonal_p, s.seasonal_d, s.seasonal_q),
            (o[0], o[1], o[2], o[3], o[4], o[5]),
            "{name}: selected {}",
            s.label()
        );
        assert_eq!(s.constant, a.get("free_constant").unwrap().as_bool().unwrap(), "{name}");
    }
}
