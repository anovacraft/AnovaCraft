//! Transfer-function–noise models compared with a profile-likelihood fit in
//! R (`tests/data/transfer_reference.R` writes `transfer_reference.json`):
//! for given transfer weights R filters the centred input by an explicit
//! recursion, `stats::arima(method = "ML", kappa = 1e10)` fits the noise
//! model to `y − v`, and `optim` maximises that profile log-likelihood over
//! the weights. The cases are Box–Jenkins Series M (R's `BJsales` with its
//! leading indicator; delay 3, first-order denominator, ARIMA(0,1,1) noise,
//! with and without drift) and a simulated stationary series with AR(1)
//! noise (a rational lag, and a pure distributed lag).

use stat_data::{JsonValue, parse_json};
use stat_timeseries::arima::{ArimaOptions, ArimaSpec, TransferInput, evaluate_arima_transfer, fit_arima_transfer};

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

/// A numeric vector that jsonlite may have written as a bare scalar.
fn nums(v: Option<&JsonValue>) -> Vec<f64> {
    match v {
        None => Vec::new(),
        Some(v) => match v.as_array() {
            Some(a) => a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect(),
            None => vec![num(v)],
        },
    }
}

fn close(got: f64, want: f64, rel: f64, abs: f64, what: &str) {
    let tol = abs.max(rel * want.abs());
    assert!((got - want).abs() <= tol, "{what}: got {got}, reference {want} (difference {:e})", (got - want).abs());
}

struct Case {
    name: String,
    y: Vec<f64>,
    spec: ArimaSpec,
    input: TransferInput,
    c: JsonValue,
}

impl Case {
    /// R's coefficients in this crate's order: ARMA, mean or drift, ω, δ.
    fn reference_coefficients(&self) -> Vec<f64> {
        let mut out = nums(self.c.get("noise_coef"));
        out.extend(nums(self.c.get("omega")));
        out.extend(nums(self.c.get("delta")));
        out
    }
}

fn cases() -> Vec<Case> {
    let all = parse_json(include_str!("data/transfer_reference.json")).unwrap();
    let JsonValue::Object(cases) = all.get("cases").unwrap().clone() else { panic!() };
    cases
        .into_iter()
        .map(|(name, c)| {
            let o = nums(c.get("order"));
            let constant = c.get("mean").unwrap().as_bool().unwrap() || c.get("drift").unwrap().as_bool().unwrap();
            let spec = ArimaSpec::new(o[0] as usize, o[1] as usize, o[2] as usize).with_constant(constant);
            let order = |k: &str| num(c.get(k).unwrap()) as usize;
            let input = TransferInput::new("x", nums(c.get("x"))).with_orders(
                order("delay"),
                order("numerator"),
                order("denominator"),
            );
            Case { name, y: nums(c.get("y")), spec, input, c }
        })
        .collect()
}

#[test]
fn likelihood_at_reference_estimates_matches_r() {
    for case in cases() {
        let coefs = case.reference_coefficients();
        let fit = evaluate_arima_transfer(&case.y, &case.spec, &[], std::slice::from_ref(&case.input), &coefs).unwrap();
        let want = num(case.c.get("loglik").unwrap());
        let tol = if case.spec.d > 0 { 1e-5 } else { 1e-8 };
        close(fit.log_likelihood, want, 0.0, tol, &format!("{} ℓ at R's estimates", case.name));
        close(fit.sigma2, num(case.c.get("sigma2").unwrap()), 1e-6, 0.0, &format!("{} σ²", case.name));
        let effect = nums(case.c.get("effect"));
        for (a, b) in fit.transfer[0].effect.iter().zip(&effect) {
            close(*a, *b, 1e-12, 1e-12, &format!("{} filtered input", case.name));
        }
    }
}

#[test]
fn maximum_likelihood_matches_r_profile_fit() {
    for case in cases() {
        let fit =
            fit_arima_transfer(&case.y, &case.spec, &[], std::slice::from_ref(&case.input), &ArimaOptions::default())
                .unwrap();
        let want_ll = num(case.c.get("loglik").unwrap());
        // Never below R's optimum (beyond the diffuse-prior difference), and equal to it.
        assert!(fit.log_likelihood >= want_ll - 1e-5, "{}: ℓ {} < R {want_ll}", case.name, fit.log_likelihood);
        close(fit.log_likelihood, want_ll, 0.0, 1e-4, &format!("{} fitted ℓ", case.name));
        let want = case.reference_coefficients();
        for ((name, got), (want, se)) in
            fit.coefficient_names.iter().zip(&fit.coefficients).zip(want.iter().zip(&fit.std_errors))
        {
            assert!(se.is_finite() && *se > 0.0, "{}: {name} std error {se}", case.name);
            // Within 1% of a standard error of R's estimate.
            close(*got, *want, 0.0, 0.01 * se, &format!("{} {name}", case.name));
        }
        close(fit.sigma2, num(case.c.get("sigma2").unwrap()), 1e-4, 0.0, &format!("{} σ²", case.name));
        assert_eq!(fit.transfer.len(), 1);
        assert_eq!(fit.transfer[0].delay, case.input.delay);
    }
}

#[test]
fn forecasts_with_future_inputs_match_r() {
    for case in cases() {
        let Some(h) = case.c.get("horizon").map(|v| num(v) as usize) else { continue };
        let coefs = case.reference_coefficients();
        let fit = evaluate_arima_transfer(&case.y, &case.spec, &[], std::slice::from_ref(&case.input), &coefs).unwrap();
        let future = nums(case.c.get("x_future"));
        assert_eq!(fit.future_inputs_needed(h), vec![h.saturating_sub(case.input.delay)]);
        let f = fit.forecast_with_inputs(h, &[], std::slice::from_ref(&future), &[0.95]).unwrap();
        let pred = nums(case.c.get("pred"));
        let se = nums(case.c.get("pred_se"));
        let rel = if case.spec.d > 0 { 1e-6 } else { 1e-9 };
        for k in 0..h {
            close(f.mean[k], pred[k], rel, 1e-9, &format!("{} forecast {k}", case.name));
            close(f.std_error[k], se[k], 1e-6, 1e-9, &format!("{} forecast se {k}", case.name));
        }
        // Too few future input values is an error; the plain forecast refuses a model with inputs.
        let need = h.saturating_sub(case.input.delay);
        if need > 0 {
            assert!(fit.forecast_with_inputs(h, &[], &[future[..need - 1].to_vec()], &[0.95]).is_err());
        }
        assert!(fit.forecast(h, &[], &[0.95]).is_err());
    }
}

#[test]
fn automatic_noise_search_keeps_the_input_structure() {
    use stat_timeseries::arima::{AutoArimaOptions, auto_arima_transfer};
    for case in cases() {
        let options = AutoArimaOptions { allow_constant: case.spec.constant, ..AutoArimaOptions::new(1) };
        let r = auto_arima_transfer(&case.y, &[], std::slice::from_ref(&case.input), &options).unwrap();
        // The input's lag structure is fixed; the noise model is searched.
        assert_eq!(r.fit.transfer[0].omega.len(), case.input.numerator + 1, "{}", case.name);
        assert_eq!(r.fit.transfer[0].delta.len(), case.input.denominator, "{}", case.name);
        // The chosen model is the best candidate by AICc.
        assert!(r.fit.aicc.is_finite(), "{}", case.name);
        assert!(r.candidates.iter().all(|c| c.aicc >= r.fit.aicc - 1e-9), "{}", case.name);
    }
}
