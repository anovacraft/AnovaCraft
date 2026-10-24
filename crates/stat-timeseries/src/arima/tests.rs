//! Unit tests of properties of the ARIMA likelihood, fit and forecasts.

use super::statespace::arma_autocovariance;
use super::*;
use stat_core::rng::Rng;

fn simulate_arma(phi: &[f64], theta: &[f64], n: usize, seed: u64) -> Vec<f64> {
    let mut rng = Rng::new(seed);
    let burn = 200;
    let e: Vec<f64> = (0..n + burn).map(|_| rng.normal()).collect();
    let mut w = vec![0.0; n + burn];
    for t in 0..n + burn {
        let mut v = e[t];
        for (j, &p) in phi.iter().enumerate() {
            if t > j {
                v += p * w[t - 1 - j];
            }
        }
        for (j, &q) in theta.iter().enumerate() {
            if t > j {
                v += q * e[t - 1 - j];
            }
        }
        w[t] = v;
    }
    w[burn..].to_vec()
}

/// −½ [n ln 2π + ln|Σ| + wᵀ Σ⁻¹ w] with Σ = σ² Γ, maximised over σ², from
/// the dense autocovariance matrix by Cholesky.
fn dense_concentrated_loglik(w: &[f64], phi: &[f64], theta: &[f64]) -> f64 {
    let n = w.len();
    let g = arma_autocovariance(phi, theta, n).unwrap();
    let mut l = vec![0.0; n * n];
    for j in 0..n {
        let mut d = g[0];
        for k in 0..j {
            d -= l[j * n + k] * l[j * n + k];
        }
        let dj = d.sqrt();
        l[j * n + j] = dj;
        for i in j + 1..n {
            let mut s = g[i - j];
            for k in 0..j {
                s -= l[i * n + k] * l[j * n + k];
            }
            l[i * n + j] = s / dj;
        }
    }
    let mut z = vec![0.0; n];
    for i in 0..n {
        let s: f64 = (0..i).map(|k| l[i * n + k] * z[k]).sum();
        z[i] = (w[i] - s) / l[i * n + i];
    }
    let quad: f64 = z.iter().map(|v| v * v).sum();
    let logdet: f64 = (0..n).map(|i| 2.0 * l[i * n + i].ln()).sum();
    let s2 = quad / n as f64;
    -0.5 * (n as f64 * (2.0 * std::f64::consts::PI * s2).ln() + logdet + n as f64)
}

#[test]
fn kalman_likelihood_equals_dense_gaussian_likelihood() {
    let w = simulate_arma(&[0.6, -0.2], &[0.4], 15, 7);
    let spec = ArimaSpec::new(2, 0, 1);
    let fit = evaluate_arima(&w, &spec, &[], &[0.6, -0.2, 0.4]).unwrap();
    let want = dense_concentrated_loglik(&w, &[0.6, -0.2], &[0.4]);
    assert!((fit.log_likelihood - want).abs() < 1e-10, "{} vs {want}", fit.log_likelihood);
    // Seasonal MA with differencing: the likelihood of the differenced data.
    let w = simulate_arma(&[], &[-0.3], 30, 11);
    let y: Vec<f64> = w
        .iter()
        .scan((0.0, Vec::<f64>::new()), |(_, hist), &v| {
            let t = hist.len();
            let lag = if t >= 4 { hist[t - 4] } else { 0.0 };
            hist.push(v + lag);
            Some(v + lag)
        })
        .collect();
    let spec = ArimaSpec::new(0, 0, 1).seasonal(0, 1, 1, 4);
    let fit = evaluate_arima(&y, &spec, &[], &[-0.3, 0.5]).unwrap();
    let wd: Vec<f64> = (4..y.len()).map(|t| y[t] - y[t - 4]).collect();
    let theta = expand_ma(&[-0.3], &[0.5], 4);
    let want = dense_concentrated_loglik(&wd, &[], &theta);
    assert!((fit.log_likelihood - want).abs() < 1e-10);
    assert_eq!(fit.n_used, y.len() - 4);
}

#[test]
fn random_walk_forecasts_are_flat_with_growing_variance() {
    let y = [1.0, 3.0, 2.0, 5.0, 4.0, 6.0, 5.5, 7.0];
    let fit = fit_arima(&y, &ArimaSpec::new(0, 1, 0), &[], &ArimaOptions::default()).unwrap();
    let diffs: Vec<f64> = y.windows(2).map(|w| w[1] - w[0]).collect();
    let s2 = diffs.iter().map(|d| d * d).sum::<f64>() / diffs.len() as f64;
    assert!((fit.sigma2 - s2).abs() < 1e-12);
    let fc = fit.forecast(3, &[], &[0.9]).unwrap();
    for h in 0..3 {
        assert!((fc.mean[h] - 7.0).abs() < 1e-12);
        assert!((fc.std_error[h] - (s2 * (h + 1) as f64).sqrt()).abs() < 1e-12);
    }
    // With drift: the drift is the mean difference; forecasts follow the line.
    let spec = ArimaSpec::new(0, 1, 0).with_constant(true);
    let fit = fit_arima(&y, &spec, &[], &ArimaOptions::default()).unwrap();
    let mean_diff = diffs.iter().sum::<f64>() / diffs.len() as f64;
    assert_eq!(fit.coefficient_names, ["drift"]);
    assert!((fit.regression[0] - mean_diff).abs() < 1e-6);
    let fc = fit.forecast(2, &[], &[]).unwrap();
    assert!((fc.mean[1] - (7.0 + 2.0 * fit.regression[0])).abs() < 1e-9);
}

#[test]
fn ar1_forecasts_decay_to_the_mean() {
    let y = simulate_arma(&[0.7], &[], 120, 3);
    let y: Vec<f64> = y.iter().map(|v| v + 10.0).collect();
    let spec = ArimaSpec::new(1, 0, 0).with_constant(true);
    let fit = fit_arima(&y, &spec, &[], &ArimaOptions::default()).unwrap();
    let (phi, mu) = (fit.ar[0], fit.regression[0]);
    let fc = fit.forecast(5, &[], &[0.95]).unwrap();
    let last = y[y.len() - 1];
    for h in 1..=5 {
        let mean = mu + phi.powi(h as i32) * (last - mu);
        let var = fit.sigma2 * (1.0 - phi.powi(2 * h as i32)) / (1.0 - phi * phi);
        assert!((fc.mean[h - 1] - mean).abs() < 1e-9);
        assert!((fc.std_error[h - 1] - var.sqrt()).abs() < 1e-9);
    }
    assert!((fit.ar[0] - 0.7).abs() < 0.15);
    assert!(fit.std_errors.iter().all(|s| s.is_finite() && *s > 0.0));
    // Residuals are the one-step prediction errors.
    for t in 1..y.len() {
        let pred = mu + phi * (y[t - 1] - mu);
        assert!((fit.fitted[t] - pred).abs() < 1e-9);
        assert!((fit.residuals[t] - (y[t] - pred)).abs() < 1e-9);
    }
}

#[test]
fn fitted_moving_average_is_invertible() {
    // Data from a non-invertible MA(1) (θ = 2) has the same distribution as
    // θ = 0.5 with four times the variance; the fit reports the invertible one.
    let w = simulate_arma(&[], &[2.0], 300, 5);
    let fit = fit_arima(&w, &ArimaSpec::new(0, 0, 1), &[], &ArimaOptions::default()).unwrap();
    assert!(fit.ma[0].abs() < 1.0, "{:?}", fit.ma);
    assert!((fit.ma[0] - 0.5).abs() < 0.12, "{:?}", fit.ma);
    assert!(fit.min_root_modulus() > 1.0);
}

#[test]
fn regression_with_arima_errors_recovers_slope() {
    let e = simulate_arma(&[0.5], &[], 200, 9);
    let x: Vec<f64> = (0..200).map(|t| (t as f64 * 0.3).sin()).collect();
    let y: Vec<f64> = e.iter().zip(&x).map(|(e, x)| 2.0 + 3.0 * x + e).collect();
    let spec = ArimaSpec::new(1, 0, 0).with_constant(true);
    let xreg = [Regressor { name: "x".into(), values: x }];
    let fit = fit_arima(&y, &spec, &xreg, &ArimaOptions::default()).unwrap();
    assert_eq!(fit.coefficient_names, ["ar1", "intercept", "x"]);
    assert!((fit.regression[1] - 3.0).abs() < 4.0 * fit.std_errors[2]);
    let future = vec![vec![0.0, 1.0]];
    let fc = fit.forecast(2, &future, &[0.8, 0.95]).unwrap();
    assert_eq!(fc.intervals.len(), 2);
    assert!(fc.intervals[0].upper[0] < fc.intervals[1].upper[0]);
    assert!(fit.forecast(2, &[], &[]).is_err());
    assert!(fit.forecast(3, &future, &[]).is_err());
    assert!(fit.forecast(2, &future, &[1.0]).is_err());
}

#[test]
fn exact_and_conditional_start_reach_the_same_maximum() {
    let y = simulate_arma(&[0.5], &[0.3], 150, 21);
    let spec = ArimaSpec::new(1, 0, 1).with_constant(true);
    let a = fit_arima(&y, &spec, &[], &ArimaOptions::default()).unwrap();
    let b = fit_arima(&y, &spec, &[], &ArimaOptions { method: EstimationMethod::Exact, ..ArimaOptions::default() })
        .unwrap();
    assert!((a.log_likelihood - b.log_likelihood).abs() < 1e-7);
    for (x, z) in a.coefficients.iter().zip(&b.coefficients) {
        assert!((x - z).abs() < 1e-3);
    }
    let k = a.parameter_count() as f64;
    let nu = a.n_used as f64;
    assert!((a.aic - (-2.0 * a.log_likelihood + 2.0 * k)).abs() < 1e-12);
    assert!((a.aicc - a.aic - 2.0 * k * (k + 1.0) / (nu - k - 1.0)).abs() < 1e-12);
    assert!((a.bic - (-2.0 * a.log_likelihood + k * nu.ln())).abs() < 1e-12);
}

#[test]
fn invalid_inputs_are_reported() {
    let y: Vec<f64> = (0..40).map(|t| (t as f64).sin() + t as f64).collect();
    let opts = ArimaOptions::default();
    let spec = ArimaSpec::new(0, 2, 1).with_constant(true);
    assert!(matches!(fit_arima(&y, &spec, &[], &opts), Err(StatError::InvalidArgument { .. })));
    let spec = ArimaSpec::new(0, 0, 1).seasonal(1, 0, 0, 1);
    assert!(matches!(fit_arima(&y, &spec, &[], &opts), Err(StatError::InvalidArgument { .. })));
    let ones = [Regressor { name: "one".into(), values: vec![1.0; 40] }];
    let spec = ArimaSpec::new(0, 1, 1);
    assert!(matches!(fit_arima(&y, &spec, &ones, &opts), Err(StatError::InvalidArgument { .. })));
    let short = [Regressor { name: "x".into(), values: vec![1.0; 39] }];
    assert!(matches!(fit_arima(&y, &spec, &short, &opts), Err(StatError::DimensionMismatch { .. })));
    let mut bad = y.clone();
    bad[3] = f64::NAN;
    assert!(matches!(fit_arima(&bad, &spec, &[], &opts), Err(StatError::NonFiniteInput { .. })));
    assert!(matches!(
        fit_arima(&y[..4], &ArimaSpec::new(2, 1, 1), &[], &opts),
        Err(StatError::InsufficientData { .. })
    ));
    assert!(evaluate_arima(&y, &ArimaSpec::new(1, 0, 0), &[], &[1.5]).is_err());
    assert!(evaluate_arima(&y, &ArimaSpec::new(1, 0, 0), &[], &[0.5, 1.0]).is_err());
}

#[test]
fn labels() {
    assert_eq!(ArimaSpec::new(0, 1, 1).seasonal(0, 1, 1, 12).label(), "ARIMA(0,1,1)(0,1,1)[12]");
    assert_eq!(ArimaSpec::new(1, 1, 0).with_constant(true).label(), "ARIMA(1,1,0) with drift");
    assert_eq!(ArimaSpec::new(2, 0, 0).with_constant(true).label(), "ARIMA(2,0,0) with mean");
}

#[test]
fn stepwise_search_on_simulated_ar2() {
    let y = simulate_arma(&[0.6, -0.3], &[], 300, 13);
    let mut opts = AutoArimaOptions::new(1);
    opts.d = Some(0);
    let res = auto_arima(&y, &[], &opts).unwrap();
    let s = res.fit.spec;
    assert_eq!((s.p, s.d, s.q), (2, 0, 0), "{}", s.label());
    // Every candidate is distinct, and the chosen one has the smallest AICc.
    let best = res.candidates.iter().map(|c| c.aicc).fold(f64::INFINITY, f64::min);
    assert!((res.fit.aicc - best).abs() < 1e-12);
    for (i, a) in res.candidates.iter().enumerate() {
        assert!(res.candidates[..i].iter().all(|b| b.spec != a.spec));
    }
    // The exhaustive search over a small space agrees.
    opts.stepwise = false;
    opts.max_p = 2;
    opts.max_q = 2;
    let all = auto_arima(&y, &[], &opts).unwrap();
    assert_eq!(all.candidates.len(), 3 * 3 * 2);
    assert!(all.fit.aicc <= res.fit.aicc + 1e-9);
}

/// (0,1,0) on constant data leaves no residual variation: a clear message
/// instead of a convergence failure.
#[test]
fn exact_fit_is_reported() {
    let e = fit_arima(&[1.0; 8], &ArimaSpec::new(0, 1, 0), &[], &ArimaOptions::default()).unwrap_err();
    assert!(e.to_string().contains("fits the series exactly"), "{e}");
}

/// A huge seasonal period is refused before any polynomial is allocated.
#[test]
fn huge_period_is_refused() {
    let y: Vec<f64> = (0..30).map(|i| (i as f64).sin()).collect();
    for period in [usize::MAX, 100_000_000] {
        let spec = ArimaSpec::new(0, 0, 0).seasonal(1, 1, 0, period);
        assert!(matches!(fit_arima(&y, &spec, &[], &ArimaOptions::default()), Err(StatError::InsufficientData { .. })));
    }
}
