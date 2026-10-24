use super::*;

#[test]
fn quantile_levels_are_the_smallest_count_reaching_the_probability() {
    let fit = CountFit {
        family: CountFamily::Poisson,
        mean: 2.0,
        size: f64::INFINITY,
        zero_inflation: 0.0,
        log_likelihood: 0.0,
        n: 10,
    };
    // Poisson(2): P(X ≤ 5) = 0.9834, P(X ≤ 4) = 0.9473.
    assert_eq!(fit.quantile(0.975, 0.0), 5.0);
    assert!(fit.quantile(1.0, 0.0).is_nan());
    assert!((fit.expected(0.0) - 2.0).abs() < 1e-15);
    let bin = CountFit { family: CountFamily::Binomial, mean: 0.5, ..fit };
    assert_eq!(bin.quantile(0.9999, 3.0), 3.0, "a binomial level never exceeds the trials");
}

#[test]
fn boundary_fits_are_reported() {
    let equal = [2.0, 3.0, 2.0, 1.0, 2.0, 3.0, 2.0];
    assert!(fit_counts(&equal, CountFamily::NegativeBinomial).is_err(), "underdispersed");
    assert!(fit_counts(&equal, CountFamily::ZeroInflatedPoisson).is_err(), "no zeros");
    assert!(fit_counts(&equal, CountFamily::ZeroInflatedNegativeBinomial).is_err());
    assert!(fit_counts(&[1.5, 2.0], CountFamily::Poisson).is_err(), "fractional");
    assert!(fit_counts(&[1.0], CountFamily::Poisson).is_err(), "too few");
    assert!(fit_counts(&equal, CountFamily::Binomial).is_err());
    assert!(fit_positives(&[2.0, 2.0], &[1.0, 3.0], CountFamily::Binomial).is_err(), "more positives than samples");
    assert!(fit_positives(&[0.0, 0.0], &[5.0, 5.0], CountFamily::BetaBinomial).is_err());
    assert!(fit_positives(&[1.0, 1.0], &[5.0], CountFamily::Binomial).is_err());
    let c = fit_candidates(&equal, &[], ReadingKind::Counts);
    assert_eq!(best_candidate(&c).map(|i| c[i].label), Some("Poisson"));
    let negative = fit_candidates(&[-1.0, 2.0, 3.0], &[], ReadingKind::Continuous);
    assert!(negative.iter().all(|c| c.fit.is_err()) && best_candidate(&negative).is_none());
}

#[test]
fn early_warning_rules() {
    let values = [0.0, 3.0, 0.0, 4.0, 9.0, 1.0, 2.0, 3.0, 4.0];
    let alert = [2.0; 9];
    let action = [8.0; 9];
    let rules = WarningRules { alerts_in_window: Some((2, 3)), rising_run: Some(3) };
    let r = warning_rules(&values, &alert, &action, &rules);
    assert_eq!(r[1], [2]);
    assert_eq!(r[3], [2, 3], "second alert in three readings");
    assert_eq!(r[4], [1, 3], "action excursion, third above alert in three readings");
    assert_eq!(r[7], [2], "only two rises so far");
    assert_eq!(r[8], [2, 3, 4], "1 → 2 → 3 → 4: three rises");
    assert!(r[5].is_empty() && r[6].is_empty());
    let none = WarningRules { alerts_in_window: None, rising_run: None };
    assert_eq!(warning_rules(&values, &alert, &action, &none)[3], [2]);
}
