//! Every assay curve template fitted from its own automatic starting values
//! and compared with an independent SciPy fit of the same model
//! (`tests/data/curve_templates_reference.py`; values in
//! `curve_templates_reference.json`): estimates, standard errors, the
//! weighted residual sum of squares and the derived quantities.

use app_model::curve_templates::{CurveTemplate, TemplateSettings};
use app_model::nonlinear_analysis::{NonlinearAnalysis, Weighting};
use stat_data::{Column, DataTable, JsonValue, RowStateManager, parse_json};

fn cases() -> JsonValue {
    parse_json(include_str!("data/curve_templates_reference.json")).unwrap()
}

fn nums(v: &JsonValue, key: &str) -> Vec<f64> {
    v.get(key).unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

/// The case's table, template settings and analysis.
fn analyse(key: &str, case: &JsonValue) -> NonlinearAnalysis {
    let template = CurveTemplate::from_key(key).unwrap_or_else(|| panic!("no template {key}"));
    let mut columns = vec![Column::numeric("X", nums(case, "x")), Column::numeric("Y", nums(case, "y"))];
    let mut second = None;
    if let Some(z) = case.get("z") {
        let items = z.as_array().unwrap();
        second = Some(columns.len());
        if items[0].as_str().is_some() {
            columns.push(Column::text("Z", items.iter().map(|v| v.as_str().unwrap().to_owned()).collect()));
        } else {
            columns.push(Column::numeric("Z", items.iter().map(|v| v.as_f64().unwrap()).collect()));
        }
    }
    let mut group = None;
    if let Some(g) = case.get("group") {
        group = Some(columns.len());
        columns
            .push(Column::text("Set", g.as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect()));
    }
    let n = columns[0].data.len();
    let table = DataTable::new("case", columns).unwrap();
    let mut settings = TemplateSettings::new(template, 0);
    settings.second = second;
    if let Some(c) = case.get("constants") {
        for (spec, slot) in template.all_constants().iter().zip(settings.constants.iter_mut()) {
            if let Some(v) = c.get(spec.key).and_then(JsonValue::as_f64) {
                *slot = v;
            }
        }
    }
    let mut request = settings.request(&table, 1, group).unwrap_or_else(|e| panic!("{key}: {e}"));
    let expected = match case.get("weighting").and_then(JsonValue::as_str) {
        Some("inverse_y2") => Weighting::InverseResponseSquared,
        Some("inverse_y") => Weighting::InverseResponse,
        _ => Weighting::Equal,
    };
    assert_eq!(request.weighting, expected, "{key}: default weighting");
    request.profile_intervals = false;
    NonlinearAnalysis::run(&table, &RowStateManager::new(n), &request).unwrap_or_else(|e| panic!("{key}: {e}"))
}

#[test]
fn templates_match_scipy() {
    let all = cases();
    let members = all.as_object().unwrap();
    assert_eq!(members.len(), CurveTemplate::ALL.len(), "one case per template");
    let mut failures = Vec::new();
    for (key, case) in members {
        let a = analyse(key, case);
        let fit = a.fit();
        assert!(fit.converged(), "{key}: did not converge ({:?})", fit.termination);
        let names: Vec<String> =
            case.get("names").unwrap().as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect();
        let (est, se) = (nums(case, "estimates"), nums(case, "std_errors"));
        for (k, name) in names.iter().enumerate() {
            let p =
                fit.parameters.iter().find(|p| p.name == *name).unwrap_or_else(|| panic!("{key}: no parameter {name}"));
            // Estimates to 10⁻⁶ relative, allowing a thousandth of a standard
            // error for the convergence tolerance; standard errors to 10⁻⁵
            // (central-difference against complex-step derivatives).
            if (p.estimate - est[k]).abs() > 1e-6 * est[k].abs() + 1e-3 * se[k] {
                failures.push(format!("{key} {name}: estimate {} vs {}", p.estimate, est[k]));
            }
            if (p.std_error - se[k]).abs() > 1e-5 * se[k] {
                failures.push(format!("{key} {name}: SE {} vs {}", p.std_error, se[k]));
            }
        }
        let sse = case.get("sse").unwrap().as_f64().unwrap();
        if (fit.sse - sse).abs() > 1e-8 * sse {
            failures.push(format!("{key}: SSE {} vs {sse}", fit.sse));
        }
        if let Some(d) = case.get("derived") {
            let values = a.template_derived();
            for (label, expected) in d.as_object().unwrap() {
                let expected = expected.as_f64().unwrap();
                match values.iter().find(|(l, _, _)| l == label) {
                    Some((_, _, v)) => {
                        if (v.estimate - expected).abs() > 1e-5 * expected.abs() {
                            failures.push(format!("{key} {label}: {} vs {expected}", v.estimate));
                        }
                        assert!(v.std_error.is_finite(), "{key} {label}: standard error");
                    }
                    None => failures.push(format!("{key}: no derived quantity {label}")),
                }
            }
        }
        if let Some(rp) = case.get("relative_potency").and_then(JsonValue::as_f64) {
            let sections = a.sections();
            let table = sections
                .iter()
                .find(|s| s.kind == app_model::report::SectionKind::RelativePotency)
                .and_then(|s| match &s.content {
                    app_model::report::SectionContent::Table(t) => Some(t.clone()),
                    _ => None,
                })
                .expect("relative potency table");
            let got: f64 = table.rows[1][2].parse().unwrap();
            if (got - rp).abs() > 1e-5 * rp {
                failures.push(format!("{key}: relative potency {got} vs {rp}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}
