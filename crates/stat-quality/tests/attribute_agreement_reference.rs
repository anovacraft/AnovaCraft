//! Attribute agreement statistics and the attribute gauge study compared
//! with published examples (Fleiss 1971; a SAS PROC FREQ kappa printout)
//! and with SciPy 1.18 / statsmodels 0.15 on a simulated study; see
//! `tests/data/attribute_agreement_reference.py`, which generated the data
//! and the reference values in `tests/data/attribute_agreement_reference.json`.

use stat_data::{JsonValue, parse_json};
use stat_quality::{
    AcceptancePoint, AgreementOptions, AgreementRatings, KappaEstimate, KappaWeights, PercentAgreement, VersusStandard,
    attribute_agreement, attribute_gauge_study, cohen_kappa, fleiss_kappa,
};

fn cases() -> JsonValue {
    parse_json(include_str!("data/attribute_agreement_reference.json")).unwrap()
}

fn num(v: &JsonValue, key: &str) -> f64 {
    v.get(key).and_then(JsonValue::as_f64).unwrap_or_else(|| panic!("no number {key}"))
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn matrix(v: &JsonValue) -> Vec<Vec<f64>> {
    v.as_array().unwrap().iter().map(nums).collect()
}

fn close(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol * b.abs().max(1.0), "{what}: {a} vs {b}");
}

fn check_cohen(k: &KappaEstimate, r: &JsonValue, what: &str) {
    close(k.kappa, num(r, "kappa"), 1e-10, what);
    close(k.se, num(r, "se"), 1e-9, what);
    close(k.se0, num(r, "se0"), 1e-9, what);
}

fn check_fleiss(k: &stat_quality::FleissKappa, r: &JsonValue, what: &str) {
    close(k.overall.kappa, num(r, "kappa"), 1e-10, what);
    close(k.overall.se0, num(r, "se0"), 1e-10, what);
    let cats = r.get("category_kappa").unwrap().as_array().unwrap();
    for (e, c) in k.by_category.iter().zip(cats) {
        match c.as_f64() {
            Some(v) => close(e.kappa, v, 1e-10, what),
            None => assert!(e.kappa.is_nan(), "{what}: unused category"),
        }
        close(e.se0, num(r, "category_se0"), 1e-12, what);
    }
}

fn check_percent(a: &PercentAgreement, r: &JsonValue, what: &str) {
    assert_eq!(a.matched as f64, num(r, "matched"), "{what}");
    close(a.percent, num(r, "percent"), 1e-12, what);
    close(a.lower, num(r, "lower"), 1e-8, what);
    close(a.upper, num(r, "upper"), 1e-8, what);
}

/// Fleiss (1971), Table 1: 30 patients each diagnosed by six psychiatrists
/// into five categories. Fleiss reports κ = 0.430 and category kappas
/// .245, .245, .520, .471, .566.
#[test]
fn fleiss_1971_psychiatric_diagnoses() {
    let c = cases();
    let r = c.get("fleiss_1971").unwrap();
    let k = fleiss_kappa(&matrix(r.get("counts").unwrap())).unwrap();
    close(k.overall.kappa, 0.430, 5e-4, "published κ");
    for (e, published) in k.by_category.iter().zip([0.245, 0.245, 0.520, 0.471, 0.566]) {
        close(e.kappa, published, 5e-4, "published category κ");
    }
    check_fleiss(&k, r, "Fleiss 1971");
    assert!(k.overall.z > 17.0 && k.overall.p_value < 1e-10);
    let w = c.get("fleiss_wiki").unwrap();
    let k = fleiss_kappa(&matrix(w.get("counts").unwrap())).unwrap();
    close(k.overall.kappa, 0.210, 5e-4, "10 × 14 example");
    check_fleiss(&k, w, "10 × 14 example");
}

/// SAS PROC FREQ on [[0,4,1],[0,8,0],[0,1,5]]: simple κ = 0.4842 (ASE
/// 0.1380, ASE under H0 0.1484); weighted κ = 0.4701 (ASE 0.1457, under H0
/// 0.1426). Quadratic weights against statsmodels.
#[test]
fn cohen_kappa_against_sas_and_statsmodels() {
    let c = cases();
    let r = c.get("sas").unwrap();
    let t = matrix(r.get("table").unwrap());
    let none = cohen_kappa(&t, KappaWeights::None, 0.95).unwrap();
    close(none.kappa, 0.4842, 1e-4, "SAS κ");
    close(none.se, 0.1380, 1e-4, "SAS ASE");
    close(none.se0, 0.1484, 1e-4, "SAS ASE under H0");
    close(none.z, 3.2626, 1e-4, "SAS z");
    let linear = cohen_kappa(&t, KappaWeights::Linear, 0.95).unwrap();
    close(linear.kappa, 0.4701, 1e-4, "SAS weighted κ");
    close(linear.se, 0.1457, 1e-4, "SAS weighted ASE");
    close(linear.se0, 0.1426, 1e-4, "SAS weighted ASE under H0");
    check_cohen(&none, r.get("none").unwrap(), "unweighted");
    check_cohen(&linear, r.get("linear").unwrap(), "linear");
    check_cohen(&cohen_kappa(&t, KappaWeights::Quadratic, 0.95).unwrap(), r.get("quadratic").unwrap(), "quadratic");
    // The interval is κ ± 1.96 ASE.
    close(none.lower, 0.2137, 1e-4, "SAS lower limit");
    close(none.upper, 0.7547, 1e-4, "SAS upper limit");
}

fn check_versus(v: &VersusStandard, r: &JsonValue, what: &str) {
    check_percent(&v.agreement, r.get("agreement").unwrap(), what);
    assert_eq!(v.table, matrix(r.get("table").unwrap()), "{what}");
    check_cohen(&v.kappa, r.get("none").unwrap(), what);
    let (lin, quad) = v.weighted.as_ref().unwrap();
    check_cohen(lin, r.get("linear").unwrap(), what);
    check_cohen(quad, r.get("quadratic").unwrap(), what);
    for (e, rc) in v.by_category.iter().zip(r.get("category").unwrap().as_array().unwrap()) {
        check_cohen(e, rc, what);
    }
    let tau = v.tau.unwrap();
    let rt = r.get("tau").unwrap();
    close(tau.tau_b, num(rt, "tau_b"), 1e-10, what);
    close(tau.p_value, num(rt, "p_value"), 1e-8, what);
}

/// The simulated three-appraiser study, every comparison against SciPy /
/// statsmodels.
#[test]
fn simulated_study_against_scipy_and_statsmodels() {
    let c = cases();
    let s = c.get("study").unwrap();
    let raw: Vec<Vec<Vec<usize>>> = s
        .get("ratings")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|a| matrix(a).into_iter().map(|t| t.into_iter().map(|v| v as usize).collect()).collect())
        .collect();
    let n = raw[0].len();
    let data = AgreementRatings {
        categories: (1..=4).map(|i| i.to_string()).collect(),
        appraisers: vec!["A".into(), "B".into(), "C".into()],
        parts: (1..=n).map(|i| i.to_string()).collect(),
        ratings: raw,
        standard: Some(nums(s.get("standard").unwrap()).into_iter().map(|v| v as usize).collect()),
    };
    let r = attribute_agreement(&data, &AgreementOptions { confidence: 0.95, ordinal: true }).unwrap();
    for (w, rw) in r.within.iter().zip(s.get("within").unwrap().as_array().unwrap()) {
        check_percent(&w.agreement, rw.get("agreement").unwrap(), "within");
        check_fleiss(&w.kappa, rw, "within");
        let cw = w.concordance.unwrap();
        let rc = rw.get("concordance").unwrap();
        close(cw.w, num(rc, "w"), 1e-10, "within W");
        close(cw.chi_square, num(rc, "chi_square"), 1e-9, "within χ²");
        close(cw.p_value, num(rc, "p_value"), 1e-9, "within p");
    }
    for (v, rv) in r.versus_standard.iter().zip(s.get("versus").unwrap().as_array().unwrap()) {
        check_versus(v, rv, "versus standard");
    }
    check_versus(r.all_versus_standard.as_ref().unwrap(), s.get("all_versus").unwrap(), "all versus standard");
    let b = r.between.as_ref().unwrap();
    let rb = s.get("between").unwrap();
    check_percent(&b.agreement, rb.get("agreement").unwrap(), "between");
    check_fleiss(&b.kappa, rb, "between");
    let cw = b.concordance.unwrap();
    close(cw.w, num(rb.get("concordance").unwrap(), "w"), 1e-10, "between W");
    close(cw.p_value, num(rb.get("concordance").unwrap(), "p_value"), 1e-9, "between p");
    assert_eq!(cw.judges, 6, "each appraiser's trial is a judge");
    for (p, rp) in b.pairwise.iter().zip(rb.get("pairwise").unwrap().as_array().unwrap()) {
        check_cohen(&p.kappa, rp, "pairwise");
    }
    assert_eq!(b.pairwise.len(), 3);
    // Disagreement counts add up.
    for v in &r.versus_standard {
        assert_eq!(v.agreement.matched + v.consistently_misrated + v.mixed, n);
    }
}

/// AIAG analytic method against an independent NumPy implementation.
#[test]
fn attribute_gauge_against_numpy() {
    let c = cases();
    let g = c.get("gauge").unwrap();
    let refs = nums(g.get("reference").unwrap());
    let acc = nums(g.get("accepted").unwrap());
    let points: Vec<AcceptancePoint> = refs
        .iter()
        .zip(&acc)
        .map(|(&reference, &accepted)| AcceptancePoint { reference, accepted, trials: 20.0 })
        .collect();
    let s = attribute_gauge_study(&points, num(g, "limit")).unwrap();
    for (e, &p) in s.points.iter().zip(&nums(g.get("adjusted").unwrap())) {
        close(e.adjusted, p, 1e-15, "P′");
    }
    close(s.intercept, num(g, "intercept"), 1e-9, "intercept");
    close(s.slope, num(g, "slope"), 1e-9, "slope");
    close(s.median_acceptance, num(g, "median"), 1e-9, "x(0.5)");
    close(s.bias, num(g, "bias"), 1e-9, "bias");
    close(s.repeatability, num(g, "repeatability"), 1e-9, "repeatability");
    close(s.t, num(g, "t"), 1e-9, "t");
    close(s.p_value, num(g, "p_value"), 1e-9, "p");
    assert_eq!(s.side, stat_quality::LimitSide::Lower);
}
