//! Exponential smoothing on values near the largest double (Antigravity
//! review F-06): the heuristic starting states overflow, which used to trip an
//! assertion in the box-constrained optimiser and crash the app.

use stat_timeseries::smoothing::{ErrorType, EtsOptions, EtsSpec, SeasonalType, TrendType, fit_ets};

#[test]
fn ets_on_values_near_the_overflow_limit_is_an_error_not_a_panic() {
    let spec = EtsSpec::new(ErrorType::Additive, TrendType::None, SeasonalType::Additive);
    assert!(fit_ets(&[1e308; 8], spec, &EtsOptions::new(4)).is_err());
}
