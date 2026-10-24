use super::*;
use crate::project::{report_from_json, report_to_json};
use crate::sample::device_a;

fn col(t: &DataTable, name: &str) -> usize {
    t.find(name).unwrap()
}

fn device_request(t: &DataTable) -> LifeStressRequest {
    LifeStressRequest {
        frequency: Some(col(t, "Units")),
        family: RegressionFamily::Lognormal,
        times: vec![50_000.0],
        ..LifeStressRequest::new(
            LifeDataLayout::Times {
                time: col(t, "Hours"),
                censor: Some(col(t, "Censored")),
                coding: StatusCoding::NonzeroIsCensored,
            },
            col(t, "Temperature (°C)"),
            10.0,
        )
    }
}

fn table_of(sections: &[ReportSection], kind: SectionKind) -> &TextTable {
    match &sections.iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("no {kind:?}")).content {
        SectionContent::Table(t) => t,
        other => panic!("{kind:?} is not a table: {other:?}"),
    }
}

fn cell(t: &TextTable, row_starts: &str, column: usize) -> f64 {
    let row = t.rows.iter().find(|r| r[0].starts_with(row_starts)).unwrap_or_else(|| panic!("no row {row_starts}"));
    row[column].replace('−', "-").parse().unwrap()
}

#[test]
fn device_a_report_has_every_section_and_published_estimates() {
    let t = device_a();
    let rows = RowStateManager::new(t.n_rows());
    let a = LifeStressAnalysis::run(&t, &rows, &device_request(&t)).unwrap();
    let s = a.sections();
    let kinds: Vec<SectionKind> = s.iter().map(|s| s.kind).collect();
    for k in [
        SectionKind::LifeStressSummary,
        SectionKind::LifeStressFitStatistics,
        SectionKind::LifeStressParameters,
        SectionKind::LifeStressDerived,
        SectionKind::LifeStressAcceleration,
        SectionKind::LifeStressUsePercentiles,
        SectionKind::LifeStressUseProbabilities,
        SectionKind::LifeStressPlot,
        SectionKind::LifeStressProbabilityPlot,
        SectionKind::LifeStressModelChecks,
        SectionKind::LifeStressLevelFits,
    ] {
        assert!(kinds.contains(&k), "missing {k:?}");
    }
    // Meeker & Escobar (1998, Example 19.6): Ea = 0.6279 eV, σ = 0.9778.
    let derived = table_of(&s, SectionKind::LifeStressDerived);
    assert!((cell(derived, "Activation energy", 1) - 0.6279).abs() < 5e-4);
    let summary = table_of(&s, SectionKind::LifeStressSummary);
    assert_eq!(summary.rows.len(), 4, "four temperatures");
    assert_eq!(a.fit().levels.len(), 4);
    // Acceleration factors: 1 at the use condition, increasing with temperature.
    let af = table_of(&s, SectionKind::LifeStressAcceleration);
    let factors: Vec<f64> = af.rows.iter().map(|r| r[1].replace('−', "-").parse::<f64>().unwrap()).collect();
    assert!(factors.windows(2).all(|w| w[1] > w[0]), "{factors:?}");
    // Every plot carries points tagged with data rows.
    for kind in [SectionKind::LifeStressPlot, SectionKind::LifeStressProbabilityPlot] {
        let SectionContent::Plot(p) = &s.iter().find(|x| x.kind == kind).unwrap().content else { panic!() };
        assert!(!p.points.is_empty());
        assert!(p.points.iter().all(|pt| pt.row < t.n_rows()));
    }
    assert!(a.title().contains("Temperature"));
}

#[test]
fn excluded_rows_and_errors() {
    let t = device_a();
    let mut rows = RowStateManager::new(t.n_rows());
    let request = device_request(&t);
    let all = LifeStressAnalysis::run(&t, &rows, &request).unwrap();
    // Exclude the 10 °C survivors: three levels remain.
    rows.set_excluded([0], true);
    let fewer = LifeStressAnalysis::run(&t, &rows, &request).unwrap();
    assert_eq!(fewer.fit().levels.len(), 3);
    assert!((fewer.fit().fit.log_likelihood - all.fit().fit.log_likelihood).abs() > 1e-6);

    let mut bad = request.clone();
    bad.use_stress = vec![-300.0];
    assert!(LifeStressAnalysis::run(&t, &rows, &bad).is_err(), "below absolute zero");
    let mut bad = request.clone();
    bad.use_stress.push(1.0);
    assert!(LifeStressAnalysis::run(&t, &rows, &bad).is_err(), "use conditions must match stresses");
    let mut bad = request.clone();
    bad.family = RegressionFamily::Normal;
    assert!(LifeStressAnalysis::run(&t, &rows, &bad).is_err(), "not a life distribution");
    let mut bad = request.clone();
    bad.frequency = Some(col(&t, "Hours"));
    assert!(LifeStressAnalysis::run(&t, &rows, &bad).is_err(), "a column in two roles");
    let mut bad = request;
    bad.confidence = 1.2;
    assert!(LifeStressAnalysis::run(&t, &rows, &bad).is_err());
}

#[test]
fn stress_dependent_scale_adds_a_test_of_constant_scale() {
    let t = device_a();
    let rows = RowStateManager::new(t.n_rows());
    let mut request = device_request(&t);
    request.family = RegressionFamily::Weibull;
    request.stress_dependent_scale = true;
    let a = LifeStressAnalysis::run(&t, &rows, &request).unwrap();
    let test = a.fit().scale_test.as_ref().expect("likelihood-ratio test of constant scale");
    assert_eq!(test.df, 1);
    assert!(test.p_value > 0.0 && test.p_value <= 1.0);
    let checks = table_of(&a.sections(), SectionKind::LifeStressModelChecks).clone();
    assert!(checks.rows.iter().any(|r| r[0].to_lowercase().contains("scale")), "{checks:?}");
}

#[test]
fn json_round_trip_and_column_mapping() {
    let t = device_a();
    let mut request = device_request(&t);
    request.interaction = false;
    request.stress_dependent_scale = true;
    request.stresses[0].unit = TemperatureUnit::Celsius;
    request.confidence = 0.9;
    request.probabilities = vec![0.05];
    let report = SavedReport::LifeStress { request: request.clone() };
    let v = report_to_json(&report, &t);
    assert_eq!(v.get("analysis").and_then(JsonValue::as_str), Some("life-stress"));
    assert_eq!(report_from_json(&v, &t).unwrap(), report);

    let mut two = request.clone();
    two.stresses.push(StressColumn {
        column: col(&t, "Units"),
        transform: StressTransform::InversePower,
        // Units are kept only for temperatures.
        unit: TemperatureUnit::Celsius,
    });
    two.frequency = None;
    two.use_stress.push(1.0);
    two.interaction = true;
    let report = SavedReport::LifeStress { request: two.clone() };
    assert_eq!(report_from_json(&report_to_json(&report, &t), &t).unwrap(), report);

    let mut cols = two.columns();
    cols.sort_unstable();
    assert_eq!(cols, vec![0, 1, 2, 3]);
    two.map_columns(&|c| c + 10);
    assert_eq!(two.stresses[1].column, col(&t, "Units") + 10);
}

#[test]
fn interval_layout_and_two_stresses_run() {
    // Inspection intervals at two temperatures and two voltages.
    let mut start = Vec::new();
    let mut end = Vec::new();
    let mut temp = Vec::new();
    let mut volts = Vec::new();
    let cells = [(60.0, 10.0, 3.0), (60.0, 20.0, 1.5), (90.0, 10.0, 1.0), (90.0, 20.0, 0.5)];
    for (c, v, scale) in cells {
        for k in 0..8 {
            let t0 = scale * (0.3 + 0.25 * f64::from(k));
            start.push(t0);
            end.push(t0 + 0.2 * scale);
            temp.push(c);
            volts.push(v);
        }
        start.push(4.0 * scale);
        end.push(f64::NAN);
        temp.push(c);
        volts.push(v);
    }
    let t = DataTable::new(
        "intervals",
        vec![
            stat_data::Column::numeric("From", start),
            stat_data::Column::numeric("To", end),
            stat_data::Column::numeric("Temp", temp),
            stat_data::Column::numeric("Volts", volts),
        ],
    )
    .unwrap();
    let mut request = LifeStressRequest::new(LifeDataLayout::Intervals { start: 0, end: 1 }, 2, 40.0);
    request.stresses.push(StressColumn {
        column: 3,
        transform: StressTransform::InversePower,
        unit: TemperatureUnit::Celsius,
    });
    request.use_stress.push(5.0);
    let rows = RowStateManager::new(t.n_rows());
    let a = LifeStressAnalysis::run(&t, &rows, &request).unwrap();
    assert_eq!(a.fit().levels.len(), 4);
    let s = a.sections();
    let derived = table_of(&s, SectionKind::LifeStressDerived);
    assert_eq!(derived.rows.len(), 2, "activation energy and power exponent");
    let summary = table_of(&s, SectionKind::LifeStressSummary);
    assert!(summary.rows.iter().all(|r| r[r.len() - 1] == "8"), "{summary:?}");
}
