//! Non-compartmental analysis against an independent NumPy implementation
//! written from the textbook formulas (`tests/data/nca_reference.py`), on
//! the invented "PK profiles" data (12 subjects × 2 formulations, oral dose,
//! with values below quantification as 0), for both area methods; plus a
//! hand-worked profile with closed-form values.

use stat_data::{JsonValue, parse_json};
use stat_inference::nca::{AucMethod, NcaSettings, geometric_summary, non_compartmental};

fn reference() -> JsonValue {
    parse_json(include_str!("data/nca_reference.json")).unwrap()
}

fn num(v: &JsonValue, key: &str) -> f64 {
    v.get(key).unwrap().as_f64().unwrap()
}

fn nums(v: &JsonValue, key: &str) -> Vec<f64> {
    v.get(key).unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-10 * b.abs().max(1e-3), "{what}: {a} vs {b}");
}

#[test]
fn every_profile_matches_numpy_for_both_methods() {
    let r = reference();
    let dose = num(&r, "dose");
    let rows = r.get("profiles").unwrap().as_array().unwrap();
    let results = r.get("results").unwrap().as_array().unwrap();
    assert_eq!(results.len(), 48);
    for res in results {
        let subject = num(res, "subject");
        let form = res.get("formulation").unwrap().as_str().unwrap();
        let method = AucMethod::from_key(res.get("method").unwrap().as_str().unwrap()).unwrap();
        let prof: Vec<&JsonValue> = rows
            .iter()
            .filter(|row| num(row, "subject") == subject && row.get("formulation").unwrap().as_str() == Some(form))
            .collect();
        // Reverse the rows to check the sort.
        let t: Vec<f64> = prof.iter().rev().map(|row| num(row, "time")).collect();
        let c: Vec<f64> = prof.iter().rev().map(|row| num(row, "conc")).collect();
        let settings = NcaSettings { method, dose: Some(dose), ..Default::default() };
        let out = non_compartmental(&t, &c, &settings).unwrap();
        let what = |p: &str| format!("subject {subject} {form} {} {p}", method.key());
        assert_eq!(out.n_points, 12);
        for (got, key) in [
            (out.cmax, "cmax"),
            (out.tmax, "tmax"),
            (out.clast, "clast"),
            (out.tlast, "tlast"),
            (out.auc_last, "auc_last"),
            (out.aumc_last, "aumc_last"),
        ] {
            close(got, num(res, key), &what(key));
        }
        let tp = out.lambda_z.expect("every reference profile has a terminal phase");
        assert_eq!(tp.n_points, res.get("n_lambda").unwrap().as_usize().unwrap(), "{}", what("n_lambda"));
        for (got, key) in [
            (tp.lambda, "lambda_z"),
            (tp.intercept, "intercept"),
            (tp.r_squared, "r_squared"),
            (tp.adj_r_squared, "adj_r_squared"),
            (tp.start, "lambda_start"),
            (tp.end, "lambda_end"),
            (out.half_life.unwrap(), "half_life"),
            (out.auc_inf.unwrap(), "auc_inf"),
            (out.auc_pct_extrapolated.unwrap(), "auc_pct_extrapolated"),
            (out.aumc_inf.unwrap(), "aumc_inf"),
            (out.mrt.unwrap(), "mrt"),
            (out.clearance.unwrap(), "clearance"),
            (out.volume_z.unwrap(), "volume_z"),
        ] {
            close(got, num(res, key), &what(key));
        }
    }
}

#[test]
fn geometric_summaries_match_numpy() {
    let r = reference();
    for s in r.get("summaries").unwrap().as_array().unwrap() {
        let g = geometric_summary(&nums(s, "values"));
        assert_eq!(g.n, 12);
        for (got, key) in [
            (g.geometric_mean, "geometric_mean"),
            (g.geometric_cv, "geometric_cv"),
            (g.arithmetic_mean, "arithmetic_mean"),
            (g.sd, "sd"),
            (g.median, "median"),
            (g.min, "min"),
            (g.max, "max"),
        ] {
            close(got.unwrap(), num(s, key), key);
        }
    }
}

#[test]
fn hand_worked_profile_has_closed_form_values() {
    let r = reference();
    let h = r.get("hand").unwrap();
    let (t, c) = (nums(h, "times"), nums(h, "conc"));
    for (method, key) in [(AucMethod::Linear, "linear"), (AucMethod::LinearUpLogDown, "log_down")] {
        let out = non_compartmental(&t, &c, &NcaSettings { method, ..Default::default() }).unwrap();
        let exp = h.get(key).unwrap();
        let tp = out.lambda_z.unwrap();
        close(tp.lambda, num(h, "lambda_z"), "lambda_z");
        close(tp.r_squared, 1.0, "r_squared");
        close(out.half_life.unwrap(), num(h, "half_life"), "half_life");
        close(out.auc_last, num(exp, "auc_last"), "auc_last");
        close(out.auc_inf.unwrap(), num(exp, "auc_inf"), "auc_inf");
        if let Some(m) = exp.get("aumc_last") {
            close(out.aumc_last, m.as_f64().unwrap(), "aumc_last");
        }
        assert!(out.clearance.is_none() && out.volume_z.is_none());
    }
}
