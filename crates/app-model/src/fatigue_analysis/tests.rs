use super::*;
use stat_data::parse_json;

const REFERENCE: &str = include_str!("../../../stat-reliability/tests/data/fatigue_reference.json");

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-12)
}

fn sample_request(t: &DataTable) -> FatigueRequest {
    let col = |n: &str| t.find(n).unwrap();
    FatigueRequest::new(col("Cycles"), Some(col("Run-out")), col("Stress (MPa)"))
}

/// The sample's fits through the report match the independent SciPy
/// reference (`stat-reliability/tests/data/fatigue_reference.py`).
#[test]
fn sample_fits_match_reference() {
    let reference = parse_json(REFERENCE).unwrap();
    let t = crate::sample::steel_fatigue();
    let states = RowStateManager::new(t.n_rows());
    for case in reference.get("cases").unwrap().as_array().unwrap() {
        let form = SnForm::from_key(case.get("form").unwrap().as_str().unwrap()).unwrap();
        let family = RegressionFamily::from_key(case.get("family").unwrap().as_str().unwrap()).unwrap();
        let scale = case.get("scale_stress").unwrap().as_bool().unwrap();
        let request = FatigueRequest { form, family, stress_dependent_scale: scale, ..sample_request(&t) };
        let a = FatigueAnalysis::run(&t, &states, &request).unwrap();
        let want_ll = case.get("log_likelihood").unwrap().as_f64().unwrap();
        assert!(close(a.fit().log_likelihood(), want_ll, 1e-8), "{form:?} {family:?}");
        let q = &case.get("quantiles").unwrap().as_array().unwrap()[0];
        let s = q.get("stress").unwrap().as_f64().unwrap();
        let p = q.get("p").unwrap().as_f64().unwrap();
        let got = a.fit().quantile(s, p, 0.95, false).estimate;
        assert!(close(got, q.get("estimate").unwrap().as_f64().unwrap(), 1e-5), "{form:?} {family:?} quantile");
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::FatigueLevels,
                SectionKind::FatigueFitStatistics,
                SectionKind::FatigueParameters,
                SectionKind::FatigueStrength,
                SectionKind::FatigueLives,
                SectionKind::FatigueSnPlot,
                SectionKind::FatigueResiduals,
                SectionKind::FatigueProbabilityPlot,
            ]
        );
    }
}

/// Run-outs are counted per level and excluded rows are left out.
#[test]
fn levels_and_exclusions() {
    let t = crate::sample::steel_fatigue();
    let mut states = RowStateManager::new(t.n_rows());
    let a = FatigueAnalysis::run(&t, &states, &sample_request(&t)).unwrap();
    let SectionContent::Table(levels) = &a.sections()[0].content else { panic!() };
    let first = levels.rows.iter().find(|r| r[0] == "200").expect("200 MPa row");
    assert_eq!(first[1..4], ["8", "6", "2"]);
    states.set_excluded([1, 7], true);
    let b = FatigueAnalysis::run(&t, &states, &sample_request(&t)).unwrap();
    assert_eq!(b.fit().data.observations.len(), 30);
    // The strength at 10⁷ cycles lies below the lowest tested stress's median.
    let strength = a.fit().stress_at_life(1e7, 0.5).unwrap();
    assert!(strength > 150.0 && strength < 220.0, "{strength}");
}

#[test]
fn bad_requests() {
    let t = crate::sample::steel_fatigue();
    let states = RowStateManager::new(t.n_rows());
    let mut r = sample_request(&t);
    r.stress = r.cycles;
    assert!(FatigueAnalysis::run(&t, &states, &r).is_err());
    let r = FatigueRequest { confidence: 1.2, ..sample_request(&t) };
    assert!(FatigueAnalysis::run(&t, &states, &r).is_err());
    let r = FatigueRequest { probabilities: vec![0.0], ..sample_request(&t) };
    assert!(FatigueAnalysis::run(&t, &states, &r).is_err());
}

#[test]
fn json_round_trip() {
    let t = crate::sample::steel_fatigue();
    for request in [
        sample_request(&t),
        FatigueRequest {
            form: SnForm::EnduranceLimit,
            family: RegressionFamily::Weibull,
            stress_dependent_scale: true,
            confidence: 0.9,
            probabilities: vec![0.01, 0.1],
            lives: vec![5e6],
            stresses: vec![180.0, 225.0],
            runout: None,
            ..sample_request(&t)
        },
    ] {
        let saved = crate::SavedReport::Fatigue { request };
        let json = crate::project::report_to_json(&saved, &t);
        assert_eq!(crate::project::report_from_json(&json, &t).unwrap(), saved);
    }
}
