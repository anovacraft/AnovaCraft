//! Nested and expanded gauge R&R checked against independent references
//! (`tests/data/gauge_reference.py`): textbook ANOVA formulas for balanced
//! crossed and nested studies, and a dense NumPy REML (cross-checked with
//! statsmodels MixedLM) for unbalanced expanded studies.

use stat_data::{JsonValue, parse_json};
use stat_quality::{
    ComponentRole, ExpandedGaugeOptions, GaugeFactor, GaugeObservation, GaugeOptions, gauge_rr_anova,
    gauge_rr_expanded, gauge_rr_nested,
};

fn cases() -> JsonValue {
    parse_json(include_str!("data/gauge_reference.json")).unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn codes(v: &JsonValue) -> Vec<usize> {
    nums(v).into_iter().map(|c| c as usize).collect()
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-12), "{what}: {a} vs {b}");
}

fn factor(name: &str, codes: Vec<usize>, random: bool, nested_in: Vec<usize>) -> GaugeFactor {
    let k = codes.iter().max().map_or(0, |m| m + 1);
    GaugeFactor { name: name.to_owned(), levels: (1..=k).map(|i| i.to_string()).collect(), codes, random, nested_in }
}

fn component(s: &stat_quality::ExpandedGaugeStudy, source: &str) -> f64 {
    s.components.iter().find(|c| c.source == source).unwrap_or_else(|| panic!("no {source}")).summary.variance
}

/// Balanced crossed study: the ANOVA method reproduces the textbook sums of
/// squares, and REML on the same data gives the same (all positive)
/// components, as it must for balanced data.
#[test]
fn crossed_balanced_anova_and_reml_agree() {
    let c = cases();
    let d = c.get("crossed_balanced").unwrap();
    let y = nums(d.get("y").unwrap());
    let (p, o) = (codes(d.get("part").unwrap()), codes(d.get("operator").unwrap()));
    let obs: Vec<GaugeObservation> =
        (0..y.len()).map(|i| GaugeObservation { part: p[i], operator: o[i], value: y[i] }).collect();
    let a = gauge_rr_anova(&obs, &GaugeOptions::default()).unwrap();
    for (row, ss) in a.anova.iter().zip(nums(d.get("ss").unwrap())) {
        close(row.sum_of_squares, ss, 1e-10, "sum of squares");
    }
    let r = d.get("components").unwrap();
    let want = |k: &str| r.get(k).unwrap().as_f64().unwrap();
    close(a.components.part.variance, want("part"), 1e-10, "part");
    close(a.components.operator.variance, want("operator"), 1e-10, "operator");
    close(a.components.interaction.variance, want("interaction"), 1e-10, "interaction");
    close(a.components.repeatability.variance, want("repeatability"), 1e-10, "repeatability");

    let fs = vec![factor("Part", p, true, vec![]), factor("Operator", o, true, vec![])];
    let e = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
    close(component(&e, "Part"), want("part"), 1e-4, "REML part");
    close(component(&e, "Operator"), want("operator"), 1e-4, "REML operator");
    close(component(&e, "Part × Operator"), want("interaction"), 1e-4, "REML interaction");
    close(component(&e, "Repeatability"), want("repeatability"), 1e-4, "REML repeatability");
    close(e.gauge_rr.summary.variance, a.components.gauge_rr.variance, 1e-4, "gauge R&R");
    close(e.total.summary.variance, a.components.total.variance, 1e-4, "total");
    assert_eq!(e.distinct_categories, a.distinct_categories);
}

/// Balanced nested study: textbook nested sums of squares and EMS
/// estimators, and REML with parts nested in operators gives the same.
#[test]
fn nested_balanced_anova_and_reml_agree() {
    let c = cases();
    let d = c.get("nested_balanced").unwrap();
    let y = nums(d.get("y").unwrap());
    let (p, o) = (codes(d.get("part").unwrap()), codes(d.get("operator").unwrap()));
    let obs: Vec<GaugeObservation> =
        (0..y.len()).map(|i| GaugeObservation { part: p[i], operator: o[i], value: y[i] }).collect();
    let a = gauge_rr_nested(&obs, &GaugeOptions::default()).unwrap();
    for (row, ss) in a.anova.iter().zip(nums(d.get("ss").unwrap())) {
        close(row.sum_of_squares, ss, 1e-10, "sum of squares");
    }
    let r = d.get("components").unwrap();
    let want = |k: &str| r.get(k).unwrap().as_f64().unwrap();
    close(a.components.operator.variance, want("operator"), 1e-10, "operator");
    close(a.components.part.variance, want("part"), 1e-10, "part");
    close(a.components.repeatability.variance, want("repeatability"), 1e-10, "repeatability");

    // Parts labelled 1…4 within each operator: nesting makes them distinct.
    let within = codes(d.get("part_within").unwrap());
    let fs = vec![factor("Part", within, true, vec![1]), factor("Operator", o, true, vec![])];
    let e = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
    close(component(&e, "Part(Operator)"), want("part"), 1e-4, "REML part");
    close(component(&e, "Operator"), want("operator"), 1e-4, "REML operator");
    close(component(&e, "Repeatability"), want("repeatability"), 1e-4, "REML repeatability");
    assert_eq!(e.components.len(), 3, "no interaction term for nested factors");
}

fn check_reference(e: &stat_quality::ExpandedGaugeStudy, d: &JsonValue) {
    let names: Vec<String> =
        d.get("names").unwrap().as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect();
    let got: Vec<&str> = e.components.iter().map(|c| c.source.as_str()).collect();
    assert_eq!(got, names);
    let v = nums(d.get("variances").unwrap());
    let cov: Vec<Vec<f64>> = d.get("covariance").unwrap().as_array().unwrap().iter().map(nums).collect();
    for (k, c) in e.components.iter().enumerate() {
        close(c.summary.variance, v[k], 2e-4, &c.source);
        close(c.std_error.unwrap(), cov[k][k].sqrt(), 2e-3, &format!("{} se", c.source));
    }
    // Standard error of the gauge R&R total: every component but the part's.
    let idx: Vec<usize> = (0..v.len())
        .filter(|&k| !matches!(e.components[k].role, ComponentRole::Part | ComponentRole::WithinPart))
        .collect();
    let var_sum: f64 = idx.iter().flat_map(|&i| idx.iter().map(move |&j| (i, j))).map(|(i, j)| cov[i][j]).sum();
    close(e.gauge_rr.std_error.unwrap(), var_sum.sqrt(), 2e-3, "gauge R&R se");
    let (lo, hi) = e.gauge_rr.interval.unwrap();
    assert!(lo < e.gauge_rr.summary.variance && e.gauge_rr.summary.variance < hi);
}

/// Unbalanced parts × operators × gauges (all random, with every two-factor
/// interaction): dense REML and statsmodels MixedLM.
#[test]
fn expanded_unbalanced_matches_dense_reml_and_statsmodels() {
    let c = cases();
    let d = c.get("expanded_unbalanced").unwrap();
    let y = nums(d.get("y").unwrap());
    let fs = vec![
        factor("Part", codes(d.get("part").unwrap()), true, vec![]),
        factor("Operator", codes(d.get("operator").unwrap()), true, vec![]),
        factor("Gauge", codes(d.get("gauge").unwrap()), true, vec![]),
    ];
    let e = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
    assert!(e.converged);
    check_reference(&e, d);
    for (c, sm) in e.components.iter().zip(nums(d.get("statsmodels").unwrap())) {
        close(c.summary.variance, sm, 1e-3, &format!("{} vs statsmodels", c.source));
    }
    assert_eq!(e.components[0].role, ComponentRole::Part);
    let repro: f64 = e.components[1..6].iter().map(|c| c.summary.variance).sum();
    close(e.reproducibility.summary.variance, repro, 1e-12, "reproducibility");
}

/// Destructive study with unequal parts per operator, a fixed fixture
/// crossed with everything, and missing readings.
#[test]
fn expanded_nested_with_fixed_factor_matches_dense_reml() {
    let c = cases();
    let d = c.get("expanded_fixed_nested").unwrap();
    let y = nums(d.get("y").unwrap());
    let fs = vec![
        factor("Part", codes(d.get("part").unwrap()), true, vec![1]),
        factor("Operator", codes(d.get("operator").unwrap()), true, vec![]),
        factor("Fixture", codes(d.get("fixture").unwrap()), false, vec![]),
    ];
    let e = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
    check_reference(&e, d);
    assert_eq!(e.fixed_tests.len(), 1);
    assert_eq!(e.fixed_tests[0].source, "Fixture");
    assert_eq!(e.fixed_tests[0].num_df, 1);
}

/// Parts × operators with a random location nested within each part and
/// missing readings (`tests/data/gauge_within_part_reference.py`, dense
/// REML): within-part variation is product variation, so it is added to
/// part-to-part variation and kept out of gauge R&R (AIAG MSA; Burdick,
/// Borror & Montgomery 2005).
#[test]
fn within_part_factor_counts_as_product_variation() {
    let d = parse_json(include_str!("data/gauge_within_part_reference.json")).unwrap();
    let y = nums(d.get("y").unwrap());
    let fs = vec![
        factor("Part", codes(d.get("part").unwrap()), true, vec![]),
        factor("Operator", codes(d.get("operator").unwrap()), true, vec![]),
        factor("Location", codes(d.get("location").unwrap()), true, vec![0]),
    ];
    let e = gauge_rr_expanded(&y, &fs, 0, &ExpandedGaugeOptions::default()).unwrap();
    assert!(e.converged);
    let got: Vec<&str> = e.components.iter().map(|c| c.source.as_str()).collect();
    let names: Vec<&str> = d.get("names").unwrap().as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(got, names);
    let v = nums(d.get("variances").unwrap());
    let cov: Vec<Vec<f64>> = d.get("covariance").unwrap().as_array().unwrap().iter().map(nums).collect();
    for (k, c) in e.components.iter().enumerate() {
        close(c.summary.variance, v[k], 2e-4, &c.source);
        close(c.std_error.unwrap(), cov[k][k].sqrt(), 2e-3, &format!("{} se", c.source));
    }
    assert_eq!(e.components[2].role, ComponentRole::WithinPart);
    assert_eq!(e.components[4].role, ComponentRole::Reproducibility, "operator × location is measurement error");
    let pair = |k: &str| nums(d.get(k).unwrap());
    for (what, t, want) in [
        ("part-to-part", &e.part, pair("part_to_part")),
        ("gauge R&R", &e.gauge_rr, pair("gauge_rr")),
        ("total", &e.total, pair("total")),
    ] {
        close(t.summary.variance, want[0], 2e-4, what);
        close(t.std_error.unwrap(), want[1], 2e-3, &format!("{what} se"));
    }
    close(e.within_part.summary.variance, v[2], 2e-4, "within-part total");
    let pct = d.get("percent_part_of_total_sd").unwrap().as_f64().unwrap();
    close(e.part.summary.percent_study_variation, pct, 1e-4, "% study variation of part-to-part");
}
