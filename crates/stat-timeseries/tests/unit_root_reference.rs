//! KPSS and augmented Dickey–Fuller tests compared with R (tseries
//! `kpss.test`, urca `ur.kpss` and `ur.df`) and statsmodels (`adfuller`,
//! `mackinnonp`, `mackinnoncrit`), all used as black boxes; see
//! `tests/data/unit_root_reference.R` and `.py`, which write
//! `unit_root_reference.json`.
//!
//! Lag selection: urca reports the selected regression on the common sample
//! of the lag search ([`AdfSample::Common`]) while statsmodels refits it on
//! every observation the selected lag allows ([`AdfSample::Maximal`]); and
//! urca searches lags from 1 while statsmodels starts at 0
//! ([`AdfOptions::min_lag`]). Each is matched under its options.

use stat_data::{JsonValue, parse_json};
use stat_timeseries::unit_root::{
    AdfDeterministic, AdfLags, AdfOptions, AdfSample, KpssLagRule, KpssNull, adf, kpss, kpss_lag,
    mackinnon_critical_value, mackinnon_p_value,
};

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn field(v: &JsonValue, key: &str) -> f64 {
    num(v.get(key).unwrap_or_else(|| panic!("missing {key}")))
}

fn text<'a>(v: &'a JsonValue, key: &str) -> &'a str {
    v.get(key).unwrap().as_str().unwrap()
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-300);
    assert!((got - want).abs() <= rel * scale, "{what}: got {got}, reference {want}");
}

fn data() -> JsonValue {
    parse_json(include_str!("data/unit_root_reference.json")).unwrap()
}

fn series(all: &JsonValue, name: &str) -> Vec<f64> {
    all.get("series").unwrap().get(name).unwrap().as_array().unwrap().iter().map(num).collect()
}

#[test]
fn kpss_matches_tseries_and_urca() {
    let all = data();
    let cases = all.get("kpss").unwrap().as_array().unwrap();
    assert_eq!(cases.len(), 24);
    for c in cases {
        let name = text(c, "series");
        let y = series(&all, name);
        let null = if text(c, "null") == "Level" { KpssNull::Level } else { KpssNull::Trend };
        let short = c.get("short").unwrap().as_bool().unwrap();
        let rule = if short { KpssLagRule::Short } else { KpssLagRule::Long };
        let lag = kpss_lag(y.len(), rule);
        assert_eq!(lag as f64, field(c, "lag"), "{name} tseries lag");
        assert_eq!(lag as f64, field(c, "urca_lag"), "{name} urca lag");
        let test = kpss(&y, null, Some(lag)).unwrap();
        let what = format!("{name} {null:?} short={short}");
        close(test.statistic, field(c, "statistic"), 1e-10, &what);
        close(test.statistic, field(c, "urca_statistic"), 1e-10, &what);
        // tseries prints its interpolated p-value to full precision.
        close(test.p_value, field(c, "p_value"), 1e-9, &format!("{what} p-value"));
        if short {
            assert_eq!(kpss(&y, null, None).unwrap().lag, lag);
        }
    }
}

#[test]
fn adf_statistics_match_urca() {
    let all = data();
    let cases = all.get("adf").unwrap().as_array().unwrap();
    assert_eq!(cases.len(), 90);
    for c in cases {
        let name = text(c, "series");
        let y = series(&all, name);
        let det = match text(c, "type") {
            "none" => AdfDeterministic::None,
            "drift" => AdfDeterministic::Constant,
            _ => AdfDeterministic::ConstantTrend,
        };
        let max = field(c, "max_lag") as usize;
        let selection = text(c, "selection");
        let lags = match selection {
            "Fixed" => AdfLags::Fixed(max),
            "AIC" => AdfLags::Aic(Some(max)),
            _ => AdfLags::Bic(Some(max)),
        };
        let test = adf(&y, &AdfOptions { deterministic: det, lags, sample: AdfSample::Common, min_lag: 1 }).unwrap();
        let what = format!("{name} {det:?} {selection} {max}");
        assert_eq!(test.lag as f64, field(c, "lag"), "{what} lag");
        assert_eq!(test.nobs as f64, field(c, "nobs"), "{what} nobs");
        close(test.statistic, field(c, "statistic"), 1e-9, &what);
        close(test.gamma, field(c, "gamma"), 1e-9, &what);
        close(test.gamma_std_error, field(c, "gamma_se"), 1e-9, &what);
    }
}

#[test]
fn adf_matches_statsmodels() {
    let all = data();
    let sm = all.get("statsmodels").unwrap();
    let cases = sm.get("adf").unwrap().as_array().unwrap();
    assert_eq!(cases.len(), 72);
    for c in cases {
        let name = text(c, "series");
        let y = series(&all, name);
        let det = match text(c, "regression") {
            "n" => AdfDeterministic::None,
            "c" => AdfDeterministic::Constant,
            _ => AdfDeterministic::ConstantTrend,
        };
        let max = field(c, "maxlag");
        let max = (max >= 0.0).then_some(max as usize);
        let lags = match text(c, "autolag") {
            "AIC" => AdfLags::Aic(max),
            "BIC" => AdfLags::Bic(max),
            _ => AdfLags::Fixed(max.unwrap()),
        };
        let test = adf(&y, &AdfOptions { deterministic: det, lags, sample: AdfSample::Maximal, min_lag: 0 }).unwrap();
        let what = format!("{name} {det:?} {lags:?}");
        assert_eq!(test.lag as f64, field(c, "lag"), "{what} lag");
        assert_eq!(test.nobs as f64, field(c, "nobs"), "{what} nobs");
        close(test.statistic, field(c, "statistic"), 1e-9, &what);
        close(test.p_value, field(c, "p_value"), 1e-8, &format!("{what} p-value"));
        let crit: Vec<f64> = c.get("crit").unwrap().as_array().unwrap().iter().map(num).collect();
        for (got, want) in test.critical_values.iter().zip(&crit) {
            close(*got, *want, 1e-12, &format!("{what} critical value"));
        }
    }
}

#[test]
fn mackinnon_functions_match_statsmodels() {
    let all = data();
    let sm = all.get("statsmodels").unwrap();
    let det = |s: &str| match s {
        "n" => AdfDeterministic::None,
        "c" => AdfDeterministic::Constant,
        _ => AdfDeterministic::ConstantTrend,
    };
    for c in sm.get("mackinnonp").unwrap().as_array().unwrap() {
        let tau = field(c, "tau");
        let want = field(c, "p_value");
        let got = mackinnon_p_value(tau, det(text(c, "regression")));
        if want == 0.0 || want == 1.0 {
            assert_eq!(got, want, "tau {tau}");
        } else {
            close(got, want, 1e-10, &format!("p-value at tau {tau}"));
        }
    }
    for c in sm.get("mackinnoncrit").unwrap().as_array().unwrap() {
        let nobs = field(c, "nobs");
        let nobs = (nobs > 0.0).then_some(nobs as usize);
        let crit: Vec<f64> = c.get("crit").unwrap().as_array().unwrap().iter().map(num).collect();
        for (level, want) in [0.01, 0.05, 0.10].iter().zip(&crit) {
            let got = mackinnon_critical_value(*level, det(text(c, "regression")), nobs).unwrap();
            close(got, *want, 1e-12, &format!("critical value {level} at {nobs:?}"));
        }
    }
}
