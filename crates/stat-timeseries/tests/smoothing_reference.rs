//! Exponential smoothing compared with R's `forecast::ets` (forecast 8.21,
//! used as black-box reference software): `tests/data/smoothing_reference.R`
//! writes `smoothing_reference.json` from classic series of R's `datasets`.
//!
//! Two kinds of check:
//!
//! * **At the reference estimates** (the strong test): the reference's
//!   smoothing parameters and initial states are supplied as fixed values,
//!   and the fitted values, innovations, likelihood, information criteria,
//!   σ² and forecasts must agree to about 1e-8 (relative). The reference
//!   reports the log-likelihood without Gaussian constants,
//!   `−½(n ln SSE + 2Σ ln|μ_t|)`; ours is the full Gaussian
//!   log-likelihood, which differs by the constant `−(n/2)(ln(2π/n) + 1)`.
//!   Its initial seasonal states are listed most recent first; ours are in
//!   time order.
//! * **Estimation**: fitting from scratch, our maximised likelihood must be
//!   at least the reference's (less a small optimiser tolerance), since both
//!   maximise the same likelihood over the same parameter region with
//!   different Nelder–Mead implementations and starts.

use stat_data::{JsonValue, parse_json};
use stat_timeseries::smoothing::{
    ErrorType, EtsOptions, EtsSpec, ForecastOptions, SeasonalType, SelectionOptions, StateVector, TrendType, fit_ets,
    select_ets,
};

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn opt(case: &JsonValue, key: &str) -> Option<f64> {
    case.get(key).and_then(JsonValue::as_f64)
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-300);
    assert!(
        (got - want).abs() <= rel * scale,
        "{what}: got {got}, reference {want} (relative error {:e})",
        (got - want).abs() / scale
    );
}

/// Element-wise comparison relative to the larger of the element and the
/// root mean square of the reference vector (residuals cross zero).
fn close_all(got: &[f64], want: &[f64], rel: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    let rms = (want.iter().map(|v| v * v).sum::<f64>() / want.len() as f64).sqrt();
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        let scale = w.abs().max(rms);
        assert!((g - w).abs() <= rel * scale, "{what}[{i}]: got {g}, reference {w}");
    }
}

fn spec_of(case: &JsonValue) -> EtsSpec {
    let error = match case.get("error").unwrap().as_str().unwrap() {
        "A" => ErrorType::Additive,
        _ => ErrorType::Multiplicative,
    };
    let trend = match case.get("trend").unwrap().as_str().unwrap() {
        "N" => TrendType::None,
        "A" => TrendType::Additive,
        _ => TrendType::DampedAdditive,
    };
    let seasonal = match case.get("season").unwrap().as_str().unwrap() {
        "N" => SeasonalType::None,
        "A" => SeasonalType::Additive,
        _ => SeasonalType::Multiplicative,
    };
    EtsSpec::new(error, trend, seasonal)
}

/// R's log-likelihood convention converted to the full Gaussian one.
fn gaussian_loglik(reference: f64, n: usize) -> f64 {
    let n = n as f64;
    reference - 0.5 * n * ((std::f64::consts::TAU / n).ln() + 1.0)
}

fn reference() -> JsonValue {
    parse_json(include_str!("data/smoothing_reference.json")).unwrap()
}

#[test]
fn fits_and_forecasts_at_the_reference_estimates_match() {
    let all = reference();
    let cases = all.get("cases").unwrap().as_array().unwrap();
    assert_eq!(cases.len(), 13);
    for case in cases {
        let name = case.get("name").unwrap().as_str().unwrap();
        let y = nums(case.get("series").unwrap());
        let n = y.len();
        let period = case.get("period").unwrap().as_usize().unwrap();
        let spec = spec_of(case);
        let mut o = EtsOptions::new(period);
        o.alpha = opt(case, "alpha");
        o.beta = opt(case, "beta");
        o.gamma = opt(case, "gamma");
        o.phi = opt(case, "phi");
        let mut seasonal = nums(case.get("seasonal").unwrap());
        seasonal.reverse();
        o.initial = Some(StateVector {
            level: num(case.get("level").unwrap()),
            slope: opt(case, "slope").unwrap_or(0.0),
            seasonal,
        });
        o.count_supplied_as_estimated = true;
        let fit = fit_ets(&y, spec, &o).unwrap();
        assert_eq!(fit.estimated_parameters, case.get("n_par").unwrap().as_usize().unwrap(), "{name}: parameter count");
        close_all(&fit.fitted, &nums(case.get("fitted").unwrap()), 1e-9, &format!("{name} fitted"));
        close_all(&fit.residuals, &nums(case.get("residuals").unwrap()), 1e-8, &format!("{name} residuals"));
        let loglik = gaussian_loglik(num(case.get("loglik").unwrap()), n);
        close(fit.log_likelihood, loglik, 1e-10, &format!("{name} log-likelihood"));
        // Information criteria differ from the reference's by the same constant.
        let shift = 2.0 * (num(case.get("loglik").unwrap()) - loglik);
        close(fit.aic, num(case.get("aic").unwrap()) + shift, 1e-10, &format!("{name} AIC"));
        close(fit.aicc.unwrap(), num(case.get("aicc").unwrap()) + shift, 1e-10, &format!("{name} AICc"));
        close(fit.bic, num(case.get("bic").unwrap()) + shift, 1e-10, &format!("{name} BIC"));
        close(fit.sigma2, num(case.get("sigma2").unwrap()), 1e-8, &format!("{name} sigma2"));

        let h = case.get("horizon").unwrap().as_usize().unwrap();
        let fc = fit.forecast(h, &ForecastOptions::default()).unwrap();
        let ref_mean = nums(case.get("mean").unwrap());
        if spec.has_linear_states() {
            close_all(&fc.mean, &ref_mean, 1e-9, &format!("{name} forecast mean"));
        } else {
            // Multiplicative seasonality: the point forecasts agree for one
            // cycle ahead; beyond it the reference reports an approximate
            // forecast mean (larger than the point forecast), which our
            // simulated path means estimate.
            close_all(&fc.mean[..period], &ref_mean[..period], 1e-9, &format!("{name} point forecast"));
            let sim = fc.path_means.as_ref().unwrap();
            for i in 0..h {
                assert!(
                    (sim[i] - ref_mean[i]).abs() <= 0.01 * ref_mean[i],
                    "{name} mean h={}: simulated {} reference {}",
                    i + 1,
                    sim[i],
                    ref_mean[i]
                );
            }
        }
        if spec.has_linear_states() {
            // Analytic class 1 and 2 variances. Up to one cycle ahead they
            // agree exactly. From `h = m` on, the reference adds the seasonal
            // term γ of `c_j = w′F^{j−1}g` at `j ≡ −1 (mod m)` where the
            // definition puts it at `j ≡ 0 (mod m)` (an innovation at time
            // n+1 updates the seasonal state that is next used at n+1+m; our
            // `c_j` are computed from the recursion itself, and the unit test
            // `analytic_variance_matches_simulation_with_strong_seasonal_smoothing`
            // confirms them by simulation). With the small γ of these fits
            // that moves the interval widths by less than 0.1%.
            let seasonal_period = if spec.seasonal == SeasonalType::None { usize::MAX } else { period };
            for (k, level) in ["80", "95"].iter().enumerate() {
                let lo = nums(case.get(&format!("lower{level}")).unwrap());
                let up = nums(case.get(&format!("upper{level}")).unwrap());
                for i in 0..h {
                    let rel = if i + 1 < seasonal_period { 1e-8 } else { 1e-3 };
                    let half = 0.5 * (up[i] - lo[i]);
                    let tol = rel * half.max(1e-12) + 1e-9 * fc.mean[i].abs();
                    assert!(
                        (fc.lower[k][i] - lo[i]).abs() <= tol,
                        "{name} lower {level} h={}: {} vs {}",
                        i + 1,
                        fc.lower[k][i],
                        lo[i]
                    );
                    assert!(
                        (fc.upper[k][i] - up[i]).abs() <= tol,
                        "{name} upper {level} h={}: {} vs {}",
                        i + 1,
                        fc.upper[k][i],
                        up[i]
                    );
                }
            }
        } else {
            // Multiplicative seasonality: we simulate; the reference uses an
            // approximate variance formula (or its own simulation), so only
            // the interval half-widths are compared, loosely.
            assert!(fc.simulated);
            for (k, level) in ["80", "95"].iter().enumerate() {
                let lo = nums(case.get(&format!("lower{level}")).unwrap());
                let up = nums(case.get(&format!("upper{level}")).unwrap());
                for i in 0..h {
                    let want = up[i] - lo[i];
                    let got = fc.upper[k][i] - fc.lower[k][i];
                    assert!(
                        (got - want).abs() <= 0.12 * want,
                        "{name} {level}% width h={}: got {got}, reference {want}",
                        i + 1
                    );
                }
            }
        }
    }
}

#[test]
fn estimation_reaches_the_reference_likelihood() {
    let all = reference();
    for case in all.get("cases").unwrap().as_array().unwrap() {
        let name = case.get("name").unwrap().as_str().unwrap();
        let y = nums(case.get("series").unwrap());
        let period = case.get("period").unwrap().as_usize().unwrap();
        let spec = spec_of(case);
        let fit = fit_ets(&y, spec, &EtsOptions::new(period)).unwrap();
        let want = gaussian_loglik(num(case.get("loglik").unwrap()), y.len());
        assert!(
            fit.log_likelihood >= want - 1e-3,
            "{name}: log-likelihood {} below the reference {want}",
            fit.log_likelihood
        );
        assert_eq!(fit.estimated_parameters, case.get("n_par").unwrap().as_usize().unwrap());
    }
}

#[test]
fn automatic_selection_is_at_least_as_good_as_the_reference_choice() {
    let all = reference();
    for case in all.get("auto").unwrap().as_array().unwrap() {
        let name = case.get("name").unwrap().as_str().unwrap();
        let y = nums(case.get("series").unwrap());
        let period = case.get("period").unwrap().as_usize().unwrap();
        let sel = select_ets(&y, &SelectionOptions::new(period)).unwrap();
        let n = y.len();
        let want_loglik_shift = gaussian_loglik(0.0, n);
        let want = num(case.get("aicc").unwrap()) - 2.0 * want_loglik_shift;
        let got = sel.best.aicc.unwrap();
        assert!(got <= want + 0.1, "{name}: AICc {got} worse than the reference's {want}");
    }
}
