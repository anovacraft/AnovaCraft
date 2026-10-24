use super::*;
use crate::report::SectionKind;
use stat_data::parse_json;

const REFERENCE: &str = include_str!("../../../stat-inference/tests/data/nca_reference.json");

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1e-3)
}

fn sample_request(t: &DataTable) -> PkRequest {
    let col = |n: &str| t.find(n).unwrap();
    PkRequest {
        treatment: Some(col("Formulation")),
        dose: PkDose::Column(col("Dose (mg)")),
        ..PkRequest::new(col("Subject"), col("Time (h)"), col("Concentration (mg/L)"))
    }
}

/// Every profile of the sample matches the independent NumPy reference
/// (`stat-inference/tests/data/nca_reference.py`) under both area rules.
#[test]
fn sample_profiles_match_reference() {
    let reference = parse_json(REFERENCE).unwrap();
    let t = crate::sample::pk_profiles();
    let states = RowStateManager::new(t.n_rows());
    for method in AucMethod::ALL {
        let request = PkRequest { method, ..sample_request(&t) };
        let a = PkAnalysis::run(&t, &states, &request).unwrap();
        let results = a.results();
        assert_eq!(results.len(), 24);
        let mut checked = 0;
        for r in reference.get("results").unwrap().as_array().unwrap() {
            if r.get("method").unwrap().as_str() != Some(method.key()) {
                continue;
            }
            let subject = format!("S{:02}", r.get("subject").unwrap().as_f64().unwrap());
            let form = r.get("formulation").unwrap().as_str().unwrap();
            let (_, _, got) = results.iter().find(|(s, f, _)| *s == subject && *f == form).unwrap();
            let got = got.unwrap();
            let want = |k: &str| r.get(k).unwrap().as_f64().unwrap();
            assert!(close(got.cmax, want("cmax")));
            assert!(close(got.auc_last, want("auc_last")), "{subject} {form} AUClast");
            assert!(close(got.aumc_last, want("aumc_last")));
            assert!(close(got.lambda_z.unwrap().lambda, want("lambda_z")));
            assert!(close(got.half_life.unwrap(), want("half_life")));
            assert!(close(got.auc_inf.unwrap(), want("auc_inf")));
            assert!(close(got.mrt.unwrap(), want("mrt")));
            assert!(close(got.clearance.unwrap(), want("clearance")), "{subject} {form} CL");
            assert!(close(got.volume_z.unwrap(), want("volume_z")));
            checked += 1;
        }
        assert_eq!(checked, 24);
    }
}

#[test]
fn sections_and_summary() {
    let t = crate::sample::pk_profiles();
    let states = RowStateManager::new(t.n_rows());
    let a = PkAnalysis::run(&t, &states, &sample_request(&t)).unwrap();
    let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [
            SectionKind::PkSettings,
            SectionKind::PkParameters,
            SectionKind::PkSummary,
            SectionKind::PkTerminalPhase,
            SectionKind::PkProfilePlot,
            SectionKind::PkSemiLogPlot
        ]
    );
    // The summary's geometric mean of AUC∞ for R matches the reference.
    let reference = parse_json(REFERENCE).unwrap();
    let want = reference
        .get("summaries")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| {
            s.get("formulation").unwrap().as_str() == Some("R")
                && s.get("parameter").unwrap().as_str() == Some("auc_inf")
        })
        .unwrap()
        .get("geometric_mean")
        .unwrap()
        .as_f64()
        .unwrap();
    let SectionContent::Table(summary) = &a.sections()[2].content else { panic!() };
    let row = summary.rows.iter().find(|r| r[0] == "AUC 0–∞" && r[1] == "R").expect("AUC∞ row for R");
    let shown: f64 = row[3].parse().unwrap();
    assert!((shown - want).abs() < 1e-3 * want, "{shown} vs {want}");
}

#[test]
fn excluded_rows_and_errors() {
    let t = crate::sample::pk_profiles();
    let mut states = RowStateManager::new(t.n_rows());
    // Excluding all of one profile removes it.
    states.set_excluded(0..12, true);
    let a = PkAnalysis::run(&t, &states, &sample_request(&t)).unwrap();
    assert_eq!(a.results().len(), 23);
    let mut bad = sample_request(&t);
    bad.time = bad.concentration;
    assert!(PkAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &bad).is_err());
    let bad = PkRequest { dose: PkDose::Value(-1.0), ..sample_request(&t) };
    assert!(PkAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &bad).is_err());
}

#[test]
fn json_round_trip() {
    let t = crate::sample::pk_profiles();
    for request in [
        sample_request(&t),
        PkRequest {
            method: AucMethod::Linear,
            administration: Administration::IntravenousInfusion { duration: 0.5 },
            lambda_rule: LambdaZRule::TimeRange(4.0, 24.0),
            dose: PkDose::Value(50.0),
            treatment: None,
            ..sample_request(&t)
        },
        PkRequest {
            administration: Administration::IntravenousBolus,
            lambda_rule: LambdaZRule::LastPoints(4),
            ..sample_request(&t)
        },
    ] {
        let saved = crate::SavedReport::Pharmacokinetics { request };
        let json = crate::project::report_to_json(&saved, &t);
        assert_eq!(crate::project::report_from_json(&json, &t).unwrap(), saved);
    }
}
