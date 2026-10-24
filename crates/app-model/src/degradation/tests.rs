use super::*;
use crate::project::{report_from_json, report_to_json};
use crate::sample::{adhesive_bonds, laser_degradation};
use stat_reliability::life_stress::{StressTransform, TemperatureUnit};

fn col(t: &DataTable, name: &str) -> usize {
    t.find(name).unwrap()
}

fn laser_request(t: &DataTable) -> DegradationRequest {
    DegradationRequest::new(
        DegradationLayout::RepeatedMeasures {
            unit: col(t, "Laser"),
            time: col(t, "Hours"),
            response: col(t, "Current increase (%)"),
        },
        10.0,
        FailureDirection::Rising,
    )
}

fn bond_request(t: &DataTable) -> DegradationRequest {
    DegradationRequest {
        path: PathModel::Linear { response: ScaleTransform::Log, time: ScaleTransform::Sqrt },
        stress: Some(StressColumn {
            column: col(t, "Temperature (°C)"),
            transform: StressTransform::Arrhenius,
            unit: TemperatureUnit::Celsius,
        }),
        use_stress: Some(40.0),
        times: vec![260.0],
        ..DegradationRequest::new(
            DegradationLayout::Destructive { time: col(t, "Weeks"), response: col(t, "Strength (N)") },
            50.0,
            FailureDirection::Falling,
        )
    }
}

fn kinds(s: &[ReportSection]) -> Vec<SectionKind> {
    s.iter().map(|s| s.kind).collect()
}

fn table_of(sections: &[ReportSection], kind: SectionKind) -> &TextTable {
    match &sections.iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("no {kind:?}")).content {
        SectionContent::Table(t) => t,
        other => panic!("{kind:?} is not a table: {other:?}"),
    }
}

#[test]
fn laser_pseudo_failure_times() {
    let t = laser_degradation();
    let rows = RowStateManager::new(t.n_rows());
    let a = DegradationAnalysis::run(&t, &rows, &laser_request(&t)).unwrap();
    assert_eq!(a.units().len(), 15);
    // Meeker & Escobar (1998, sec. 13.5): three lasers pass a 10% increase by 4000 hours.
    let reached = a
        .units()
        .iter()
        .filter(|u| matches!(u.path.as_ref().unwrap().crossing, Crossing::At(x) if x <= 4000.0))
        .count();
    assert_eq!(reached, 3);
    let s = a.sections();
    for k in [
        SectionKind::DegradationSummary,
        SectionKind::DegradationPathPlot,
        SectionKind::DegradationUnitPaths,
        SectionKind::DegradationLifeParameters,
        SectionKind::DegradationProbabilityPlot,
        SectionKind::DegradationPercentiles,
    ] {
        assert!(kinds(&s).contains(&k), "missing {k:?}");
    }
    assert_eq!(table_of(&s, SectionKind::DegradationUnitPaths).rows.len(), 15);
    let SectionContent::Plot(p) = &s.iter().find(|x| x.kind == SectionKind::DegradationPathPlot).unwrap().content
    else {
        panic!()
    };
    assert_eq!(p.points.len(), t.n_rows());
    assert_eq!(p.segments.len(), 15, "one fitted path per laser");
    assert!(p.line.as_ref().unwrap().iter().all(|q| q[1] == 10.0), "threshold line");
}

#[test]
fn random_effects_and_nonlinear_paths() {
    let t = laser_degradation();
    let rows = RowStateManager::new(t.n_rows());
    let mut request = laser_request(&t);
    request.method = DegradationMethod::RandomEffects;
    request.bootstrap = 20;
    request.times = vec![4000.0];
    let a = DegradationAnalysis::run(&t, &rows, &request).unwrap();
    let s = a.sections();
    for k in [
        SectionKind::DegradationRandomEffects,
        SectionKind::DegradationLifeCurve,
        SectionKind::DegradationPercentiles,
        SectionKind::DegradationProbabilities,
    ] {
        assert!(kinds(&s).contains(&k), "missing {k:?}");
    }
    // The bootstrap is seeded: the report is reproducible.
    let again = DegradationAnalysis::run(&t, &rows, &request).unwrap().sections();
    assert_eq!(s, again);

    // Power-law paths with pseudo failure times.
    let mut request = laser_request(&t);
    request.path = PathModel::Power;
    request.life_family = RegressionFamily::Lognormal;
    let a = DegradationAnalysis::run(&t, &rows, &request).unwrap();
    assert!(a.units().iter().all(|u| u.path.is_ok()));

    // Random effects need the linear path.
    request.method = DegradationMethod::RandomEffects;
    assert!(DegradationAnalysis::run(&t, &rows, &request).is_err());
}

#[test]
fn destructive_bonds_with_arrhenius_rate() {
    let t = adhesive_bonds();
    let rows = RowStateManager::new(t.n_rows());
    let a = DegradationAnalysis::run(&t, &rows, &bond_request(&t)).unwrap();
    let s = a.sections();
    for k in [
        SectionKind::DegradationSummary,
        SectionKind::DegradationPathPlot,
        SectionKind::DegradationLifeParameters,
        SectionKind::DegradationLifeCurve,
        SectionKind::DegradationPercentiles,
        SectionKind::DegradationProbabilities,
    ] {
        assert!(kinds(&s).contains(&k), "missing {k:?}");
    }
    // The data were simulated with an activation energy of 0.7 eV.
    let params = table_of(&s, SectionKind::DegradationLifeParameters);
    let ea = params.rows.iter().find(|r| r[0].starts_with("Activation energy")).unwrap();
    let ea: f64 = ea[1].parse().unwrap();
    assert!((ea - 0.7).abs() < 0.15, "Ea {ea}");
    let SectionContent::Plot(p) = &s.iter().find(|x| x.kind == SectionKind::DegradationPathPlot).unwrap().content
    else {
        panic!()
    };
    let groups = p.groups.as_ref().unwrap();
    assert_eq!(groups.labels.len(), 3, "a fitted line per temperature");
    assert_eq!(groups.lines.len(), 3);

    // The response distribution must be on the real line.
    let mut bad = bond_request(&t);
    bad.response_family = RegressionFamily::Weibull;
    assert!(DegradationAnalysis::run(&t, &rows, &bad).is_err());
    // A stress without a use condition is refused.
    let mut bad = bond_request(&t);
    bad.use_stress = None;
    assert!(DegradationAnalysis::run(&t, &rows, &bad).is_err());
}

#[test]
fn json_round_trip_and_columns() {
    let t = laser_degradation();
    let mut request = laser_request(&t);
    request.method = DegradationMethod::RandomEffects;
    request.bootstrap = 50;
    request.confidence = 0.9;
    request.probabilities = vec![0.1];
    request.times = vec![5000.0];
    let report = SavedReport::Degradation { request: request.clone() };
    let v = report_to_json(&report, &t);
    assert_eq!(v.get("analysis").and_then(JsonValue::as_str), Some("degradation"));
    assert_eq!(report_from_json(&v, &t).unwrap(), report);
    let mut nonlinear = request.clone();
    nonlinear.path = PathModel::Asymptotic;
    nonlinear.method = DegradationMethod::PseudoFailureTimes;
    nonlinear.life_family = RegressionFamily::LogLogistic;
    let report = SavedReport::Degradation { request: nonlinear };
    assert_eq!(report_from_json(&report_to_json(&report, &t), &t).unwrap(), report);

    let b = adhesive_bonds();
    let request = bond_request(&b);
    let report = SavedReport::Degradation { request: request.clone() };
    assert_eq!(report_from_json(&report_to_json(&report, &b), &b).unwrap(), report);
    let mut cols = request.columns();
    cols.sort_unstable();
    assert_eq!(cols, vec![0, 1, 2]);
    let mut mapped = request;
    mapped.map_columns(&|c| c + 5);
    assert_eq!(mapped.stress.unwrap().column, col(&b, "Temperature (°C)") + 5);
}
