//! Property tests of the exponential smoothing models.

use super::*;

fn seasonal_series(n: usize, m: usize, multiplicative: bool) -> Vec<f64> {
    // Deterministic pseudo-noise so the tests do not depend on a generator.
    (0..n)
        .map(|t| {
            let season = (std::f64::consts::TAU * t as f64 / m as f64).sin();
            let noise = ((t * 7919 % 101) as f64 / 101.0 - 0.5) * 2.0;
            let trend = 100.0 + 0.8 * t as f64;
            if multiplicative { trend * (1.0 + 0.2 * season) + noise } else { trend + 10.0 * season + noise }
        })
        .collect()
}

#[test]
fn simple_smoothing_with_alpha_one_is_the_random_walk() {
    let y = [3.0, 5.0, 4.0, 8.0, 6.0];
    let mut o = EtsOptions::new(1);
    o.alpha = Some(1.0);
    o.initial = Some(StateVector { level: 3.0, slope: 0.0, seasonal: vec![] });
    let fit = fit_ets(&y, EtsSpec::simple(), &o).unwrap();
    assert_eq!(fit.fitted, vec![3.0, 3.0, 5.0, 4.0, 8.0]);
    assert_eq!(fit.final_state.level, 6.0);
    assert_eq!(fit.estimated_parameters, 0);
    let fc = fit.forecast(3, &ForecastOptions::default()).unwrap();
    assert_eq!(fc.mean, vec![6.0; 3]);
    // Random walk: variance grows linearly, σ²·h.
    let ratio = fc.standard_errors[2].powi(2) / fc.standard_errors[0].powi(2);
    assert!((ratio - 3.0).abs() < 1e-12);
}

#[test]
fn simple_smoothing_matches_the_weighted_average_recursion() {
    let y = [10.0, 12.0, 11.0, 15.0, 13.0, 14.0];
    let alpha = 0.3;
    let mut o = EtsOptions::new(1);
    o.alpha = Some(alpha);
    o.initial = Some(StateVector { level: 10.0, slope: 0.0, seasonal: vec![] });
    let fit = fit_ets(&y, EtsSpec::simple(), &o).unwrap();
    let mut level: f64 = 10.0;
    for (t, &v) in y.iter().enumerate() {
        assert!((fit.fitted[t] - level).abs() < 1e-12);
        level = alpha * v + (1.0 - alpha) * level;
    }
    assert!((fit.final_state.level - level).abs() < 1e-12);
}

#[test]
fn holt_linear_extrapolates_an_exact_line() {
    let y: Vec<f64> = (0..20).map(|t| 5.0 + 2.0 * t as f64).collect();
    let mut o = EtsOptions::new(1);
    o.alpha = Some(0.5);
    o.beta = Some(0.1);
    o.initial = Some(StateVector { level: 3.0, slope: 2.0, seasonal: vec![] });
    let fit = fit_ets(&y, EtsSpec::holt_linear(), &o).unwrap();
    assert!(fit.residuals.iter().all(|e| e.abs() < 1e-12));
    let fc = fit.forecast(3, &ForecastOptions::default()).unwrap();
    for (h, m) in fc.mean.iter().enumerate() {
        assert!((m - (5.0 + 2.0 * (20 + h) as f64)).abs() < 1e-9);
    }
}

#[test]
fn damped_forecasts_converge_to_the_damped_sum() {
    let y: Vec<f64> = (0..30).map(|t| 50.0 + t as f64 + if t % 2 == 0 { 0.5 } else { -0.5 }).collect();
    let fit = fit_ets(&y, EtsSpec::damped_trend(), &EtsOptions::new(1)).unwrap();
    let phi = fit.parameters.phi.unwrap();
    assert!((0.8..=0.98).contains(&phi));
    let fc = fit.forecast(3000, &ForecastOptions::default()).unwrap();
    let limit = fit.final_state.level + phi / (1.0 - phi) * fit.final_state.slope;
    assert!((fc.mean[2999] - limit).abs() < 1e-6 * limit.abs());
}

#[test]
fn estimation_respects_the_usual_region_and_normalisation() {
    let y = seasonal_series(72, 12, false);
    let fit = fit_ets(&y, EtsSpec::holt_winters_additive(), &EtsOptions::new(12)).unwrap();
    let p = fit.parameters;
    let (beta, gamma) = (p.beta.unwrap(), p.gamma.unwrap());
    assert!(1e-4 <= beta && beta <= p.alpha && p.alpha <= 0.9999);
    assert!(gamma <= 1.0 - p.alpha + 1e-12);
    assert!(fit.initial.seasonal.iter().sum::<f64>().abs() < 1e-9);
    assert_eq!(fit.estimated_parameters, 3 + 2 + 11);
    // The noise has amplitude about 1: the fit should be close to it.
    assert!(fit.accuracy.rmse < 2.0, "rmse {}", fit.accuracy.rmse);
    let ym = seasonal_series(72, 12, true);
    let fit = fit_ets(&ym, EtsSpec::holt_winters_multiplicative(), &EtsOptions::new(12)).unwrap();
    assert!((fit.initial.seasonal.iter().sum::<f64>() - 12.0).abs() < 1e-9);
}

#[test]
fn simulated_intervals_agree_with_analytic_ones_for_a_linear_model() {
    let y = seasonal_series(60, 4, false);
    let fit = fit_ets(
        &y,
        EtsSpec::new(ErrorType::Multiplicative, TrendType::Additive, SeasonalType::Additive),
        &EtsOptions::new(4),
    )
    .unwrap();
    let analytic = fit.forecast(8, &ForecastOptions::default()).unwrap();
    assert!(!analytic.simulated);
    let sim = fit
        .forecast(
            8,
            &ForecastOptions { method: IntervalMethod::Simulation, paths: 20000, ..ForecastOptions::default() },
        )
        .unwrap();
    assert!(sim.simulated);
    for h in 0..8 {
        let rel = (sim.standard_errors[h] - analytic.standard_errors[h]).abs() / analytic.standard_errors[h];
        assert!(rel < 0.03, "h={h}: simulated {} analytic {}", sim.standard_errors[h], analytic.standard_errors[h]);
    }
    // Reproducible from the seed.
    let again = fit
        .forecast(
            8,
            &ForecastOptions { method: IntervalMethod::Simulation, paths: 20000, ..ForecastOptions::default() },
        )
        .unwrap();
    assert_eq!(sim, again);
}

#[test]
fn selection_excludes_unstable_models_and_needs_positive_data_for_multiplicative() {
    let y = seasonal_series(48, 12, true);
    let models = candidate_models(&y, &SelectionOptions::new(12));
    assert_eq!(models.len(), 15);
    assert!(!models.iter().any(|s| s.error == ErrorType::Additive && s.seasonal == SeasonalType::Multiplicative));
    let negative: Vec<f64> = y.iter().map(|v| v - 300.0).collect();
    let models = candidate_models(&negative, &SelectionOptions::new(12));
    assert_eq!(models.len(), 6);
    assert!(models.iter().all(|s| !s.is_multiplicative()));
    // Too short for seasonality.
    let models = candidate_models(&y[..20], &SelectionOptions::new(12));
    assert!(models.iter().all(|s| s.seasonal == SeasonalType::None));
}

#[test]
fn invalid_input_is_rejected() {
    let o = EtsOptions::new(12);
    assert!(matches!(
        fit_ets(&[1.0, f64::NAN, 3.0, 4.0], EtsSpec::simple(), &EtsOptions::new(1)),
        Err(StatError::NonFiniteInput { .. })
    ));
    assert!(matches!(
        fit_ets(&[1.0, 2.0], EtsSpec::simple(), &EtsOptions::new(1)),
        Err(StatError::InsufficientData { .. })
    ));
    let y = seasonal_series(20, 12, false);
    assert!(matches!(fit_ets(&y, EtsSpec::holt_winters_additive(), &o), Err(StatError::InsufficientData { .. })));
    let neg = [1.0, -2.0, 3.0, 4.0, 5.0];
    assert!(
        fit_ets(
            &neg,
            EtsSpec::new(ErrorType::Multiplicative, TrendType::None, SeasonalType::None),
            &EtsOptions::new(1)
        )
        .is_err()
    );
    let mut bad = EtsOptions::new(1);
    bad.gamma = Some(0.1);
    assert!(fit_ets(&[1.0, 2.0, 3.0, 4.0, 5.0], EtsSpec::simple(), &bad).is_err());
    let mut bad = EtsOptions::new(1);
    bad.alpha = Some(1.5);
    assert!(fit_ets(&[1.0, 2.0, 3.0, 4.0, 5.0], EtsSpec::simple(), &bad).is_err());
    let fit = fit_ets(&[1.0, 2.0, 3.0, 2.0, 5.0], EtsSpec::simple(), &EtsOptions::new(1)).unwrap();
    assert!(fit.forecast(0, &ForecastOptions::default()).is_err());
    assert!(fit.forecast(2, &ForecastOptions { levels: vec![1.0], ..ForecastOptions::default() }).is_err());
}

#[test]
fn labels_and_names() {
    assert_eq!(EtsSpec::damped_trend().label(), "ETS(A,Ad,N)");
    assert_eq!(EtsSpec::holt_winters_multiplicative().label(), "ETS(M,A,M)");
    assert_eq!(EtsSpec::simple().name(), "Simple exponential smoothing");
    assert_eq!(EtsSpec::new(ErrorType::Multiplicative, TrendType::None, SeasonalType::Additive).name(), "ETS(M,N,A)");
}

#[test]
fn analytic_variance_matches_simulation_with_strong_seasonal_smoothing() {
    // With α = 0.3 and γ = 0.6 the seasonal term of c_j = w′F^{j−1}g matters:
    // at h = m the variance is σ²(1 + (m − 1)α²); placing γ one step early
    // would give σ²(1 + (m − 2)α² + (α + γ)²), 57% larger here.
    let m = 4;
    let y = seasonal_series(40, m, false);
    let mut o = EtsOptions::new(m);
    o.alpha = Some(0.3);
    o.gamma = Some(0.6);
    o.initial = Some(StateVector { level: 100.0, slope: 0.0, seasonal: vec![0.0, 10.0, 0.0, -10.0] });
    let spec = EtsSpec::new(ErrorType::Additive, TrendType::None, SeasonalType::Additive);
    let fit = fit_ets(&y, spec, &o).unwrap();
    let analytic = fit.forecast(2 * m + 1, &ForecastOptions::default()).unwrap();
    let sim = fit
        .forecast(
            2 * m + 1,
            &ForecastOptions { method: IntervalMethod::Simulation, paths: 40000, ..ForecastOptions::default() },
        )
        .unwrap();
    let ratio = analytic.standard_errors[m - 1].powi(2) / fit.sigma2;
    assert!((ratio - (1.0 + (m - 1) as f64 * 0.09)).abs() < 1e-12);
    for h in 0..2 * m + 1 {
        let rel = (sim.standard_errors[h] / analytic.standard_errors[h] - 1.0).abs();
        assert!(
            rel < 0.02,
            "h={}: simulated {} analytic {}",
            h + 1,
            sim.standard_errors[h],
            analytic.standard_errors[h]
        );
    }
}

#[test]
fn partially_fixed_parameters_stay_fixed_and_are_not_counted() {
    let y: Vec<f64> = (0..40).map(|t| 20.0 + 0.5 * t as f64 + ((t * 37 % 11) as f64 - 5.0) * 0.3).collect();
    let mut o = EtsOptions::new(1);
    o.phi = Some(0.9);
    o.beta = Some(0.05);
    let fit = fit_ets(&y, EtsSpec::damped_trend(), &o).unwrap();
    assert_eq!(fit.parameters.phi, Some(0.9));
    assert_eq!(fit.parameters.beta, Some(0.05));
    assert!(fit.parameters.alpha >= 0.05);
    // alpha, level and slope.
    assert_eq!(fit.estimated_parameters, 3);
    let free = fit_ets(&y, EtsSpec::damped_trend(), &EtsOptions::new(1)).unwrap();
    assert!(free.log_likelihood >= fit.log_likelihood - 1e-9);
}
