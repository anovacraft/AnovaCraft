//! Stratified 2 × 2 × K analysis compared with statsmodels 0.15
//! `StratifiedTable`, generalized CMH statistics with an independent NumPy
//! implementation (full Kronecker-product covariance), and the r × c exact
//! test with brute-force enumeration and an exhaustive convolution; see
//! `tests/data/stratified_reference.py`, which wrote
//! `tests/data/stratified_reference.json`.

use stat_data::{JsonValue, parse_json};
use stat_discrete::exact_rxc::{ExactMethod, ExactOptions, fisher_exact_rxc};
use stat_discrete::stratified::{TwoByTwo, generalized_cmh, mantel_haenszel};

fn cases() -> JsonValue {
    parse_json(include_str!("data/stratified_reference.json")).unwrap()
}

fn f(v: &JsonValue, key: &str) -> f64 {
    v.get(key).unwrap_or_else(|| panic!("no {key}")).as_f64().unwrap()
}

fn grid(v: &JsonValue) -> Vec<Vec<u64>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as u64).collect())
        .collect()
}

fn strata(v: &JsonValue) -> Vec<Vec<Vec<u64>>> {
    v.as_array().unwrap().iter().map(grid).collect()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
}

#[test]
fn mantel_haenszel_matches_statsmodels() {
    for c in cases().get("two_by_two").unwrap().as_array().unwrap() {
        let s: Vec<TwoByTwo> =
            strata(c.get("strata").unwrap()).iter().map(|t| [[t[0][0], t[0][1]], [t[1][0], t[1][1]]]).collect();
        let mh = mantel_haenszel(&s, 0.95).unwrap();
        close(mh.cmh.statistic, f(c, "cmh"), 1e-10);
        close(mh.cmh.p_value, f(c, "cmh_p"), 1e-9);
        close(mh.cmh_corrected.statistic, f(c, "cmh_corrected"), 1e-10);
        close(mh.cmh_corrected.p_value, f(c, "cmh_corrected_p"), 1e-9);
        close(mh.odds_ratio, f(c, "odds_ratio"), 1e-12);
        close(mh.log_odds_ratio_se.unwrap(), f(c, "log_or_se"), 1e-10);
        let (lo, hi) = mh.interval.unwrap();
        close(lo, f(c, "lower"), 1e-8);
        close(hi, f(c, "upper"), 1e-8);
        let bd = mh.breslow_day.unwrap();
        assert_eq!(bd.df, f(c, "breslow_day_df") as usize);
        close(bd.statistic, f(c, "breslow_day"), 1e-8);
        close(bd.p_value, f(c, "breslow_day_p"), 1e-8);
        let t = mh.breslow_day_tarone.unwrap();
        close(t.statistic, f(c, "tarone"), 1e-8);
        close(t.p_value, f(c, "tarone_p"), 1e-8);
    }
}

/// Includes the job satisfaction × income × gender table printed in R's
/// `mantelhaen.test` documentation (Agresti 2002, Table 7.8), whose general
/// association statistic is given there as 10.2 on 9 df (p = 0.3345).
#[test]
fn generalized_cmh_matches_independent_implementation() {
    for c in cases().get("general").unwrap().as_array().unwrap() {
        let name = c.get("name").unwrap().as_str().unwrap();
        let g = generalized_cmh(
            &strata(c.get("strata").unwrap()),
            &nums(c.get("row_scores").unwrap()),
            &nums(c.get("column_scores").unwrap()),
        )
        .unwrap();
        for (got, key) in [
            (g.correlation, "correlation"),
            (g.row_mean_scores, "row_mean_scores"),
            (g.general_association, "general_association"),
        ] {
            let want = c.get(key).unwrap();
            let got = got.unwrap_or_else(|| panic!("{name} {key}: singular"));
            assert_eq!(got.df, f(want, "df") as usize, "{name} {key}");
            close(got.statistic, f(want, "statistic"), 1e-9);
            close(got.p_value, f(want, "p"), 1e-8);
        }
        if name == "job_satisfaction" {
            let ga = g.general_association.unwrap();
            assert!((ga.statistic - 10.2).abs() < 0.005 && (ga.p_value - 0.3345).abs() < 5e-5);
        }
    }
}

#[test]
fn fisher_rxc_matches_enumeration() {
    for c in cases().get("fisher").unwrap().as_array().unwrap() {
        let t = grid(c.get("table").unwrap());
        let e = fisher_exact_rxc(&t, &ExactOptions::default()).unwrap();
        assert_eq!(e.method, ExactMethod::Network);
        close(e.p_value, f(c, "p"), 1e-10);
    }
}

/// R's `fisher.test` documentation's job satisfaction example (Agresti
/// 2002, p. 57): the 4 × 4 table of income by job satisfaction, p = 0.7827.
#[test]
fn fisher_rxc_job_satisfaction_example() {
    let job = vec![vec![1, 2, 1, 0], vec![3, 3, 6, 1], vec![10, 10, 14, 9], vec![6, 7, 12, 11]];
    let e = fisher_exact_rxc(&job, &ExactOptions::default()).unwrap();
    assert_eq!(e.method, ExactMethod::Network);
    assert!((e.p_value - 0.7827).abs() < 5e-5, "{}", e.p_value);
}
