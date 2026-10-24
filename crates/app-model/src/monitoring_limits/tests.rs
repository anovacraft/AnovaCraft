use super::*;
use crate::sample::clean_room;

fn col(t: &DataTable, name: &str) -> usize {
    t.find(name).unwrap()
}

fn base(t: &DataTable, value: &str) -> MonitoringRequest {
    MonitoringRequest { order: Some(col(t, "Date")), ..MonitoringRequest::new(col(t, value), Some(col(t, "Location"))) }
}

fn location<'a>(a: &'a MonitoringAnalysis, prefix: &str) -> &'a Location {
    a.locations.iter().find(|l| l.name.starts_with(prefix)).unwrap()
}

#[test]
fn colony_counts_by_room_grade() {
    let t = clean_room();
    let rows = RowStateManager::new(t.n_rows());
    let r = MonitoringRequest { limits_by: LimitsBy::Column(col(&t, "Room grade")), ..base(&t, "Air colonies (CFU)") };
    let a = MonitoringAnalysis::run(&t, &rows, &r).unwrap();
    assert_eq!(a.locations.len(), 10);
    let names: Vec<&str> = a.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["A", "B", "C", "D", MISSING_LEVEL], "a missing grade is a level of its own");
    // The operator station's two contaminated days are action excursions,
    // and the material airlock's last days rise.
    let b02 = location(&a, "B-02");
    assert_eq!(b02.count(1), 2, "{:?} {:?}", b02.values, b02.action);
    assert!(location(&a, "C-03").count(4) >= 1);
    // Every level is a whole count from the chosen family's quantiles.
    for g in &a.groups {
        assert!(g.alert.fract() == 0.0 && g.action.fract() == 0.0 && g.alert <= g.action, "{}", g.name);
        assert!(g.chosen.is_some());
    }
    // Points carry their dataset rows, and the date labels them.
    let s = a.sections();
    let chart = s.iter().find(|x| x.kind == SectionKind::MonitoringCharts && x.title.starts_with("B-02")).unwrap();
    let SectionContent::ControlChart(c) = &chart.content else { panic!() };
    assert_eq!(c.points.len(), 40);
    assert_eq!(c.points[0].rows, [3]);
    assert_eq!(c.points[0].label.as_deref(), Some("2027-01-01"));
    assert!(c.points[30].rules.contains(&1));
    assert!(a.signal_rows().contains(&(30 * 10 + 3)));
    assert!(a.title().contains("by Location"));
    let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
    assert!(kinds.contains(&SectionKind::MonitoringFits) && kinds.contains(&SectionKind::MonitoringCandidates));
    // Signals only: fewer charts.
    let only = MonitoringAnalysis::run(&t, &rows, &MonitoringRequest { signals_only: true, ..r.clone() }).unwrap();
    let charts =
        |a: &MonitoringAnalysis| a.sections().iter().filter(|x| x.kind == SectionKind::MonitoringCharts).count();
    assert!(charts(&only) < charts(&a) && charts(&only) >= 2);
}

#[test]
fn positives_continuous_and_other_level_methods() {
    let t = clean_room();
    let rows = RowStateManager::new(t.n_rows());
    let positives = MonitoringRequest {
        kind: ReadingKind::Positives,
        trials: SubgroupSize::Column(col(&t, "Contact plates taken")),
        limits_by: LimitsBy::All,
        ..base(&t, "Contact plates positive")
    };
    let a = MonitoringAnalysis::run(&t, &rows, &positives).unwrap();
    // Plates taken are 5 or 10, so the levels vary by location and the
    // charts draw them per reading; values are proportions.
    let a01 = location(&a, "A-01");
    let c01 = location(&a, "C-01");
    assert!(a01.values.iter().all(|v| (0.0..=1.0).contains(v)));
    let on_grid = |v: f64, n: f64| ((v * n).round() - v * n).abs() < 1e-9;
    assert!(a01.alert.iter().chain(&a01.action).all(|v| on_grid(*v, 5.0)));
    assert!(c01.alert.iter().chain(&c01.action).all(|v| on_grid(*v, 10.0)));
    assert!((a.typical_trials - 7.5).abs() < 1e-12, "median of 5s and 10s");
    let missing = MonitoringRequest { trials: SubgroupSize::None, ..positives.clone() };
    assert!(MonitoringAnalysis::run(&t, &rows, &missing).is_err());

    let particles = MonitoringRequest {
        kind: ReadingKind::Continuous,
        method: LevelMethod::Sigma { alert: 2.0, action: 3.0 },
        ..base(&t, "Particles ≥0.5 µm per m³")
    };
    let a = MonitoringAnalysis::run(&t, &rows, &particles).unwrap();
    let d01 = location(&a, "D-01");
    let g = &a.groups[d01.group];
    assert!((g.alert - (g.mean + 2.0 * g.sd)).abs() < 1e-9 * g.alert);
    assert!(d01.rules.iter().any(|r| !r.is_empty()));
    let fitted =
        MonitoringRequest { method: LevelMethod::DEFAULT, family: Some("lognormal".into()), ..particles.clone() };
    let a = MonitoringAnalysis::run(&t, &rows, &fitted).unwrap();
    assert!(a.groups.iter().all(|g| g.candidates.len() == 1 && g.candidates[0].label == "Lognormal"));
    let wrong = MonitoringRequest { family: Some("poisson".into()), ..fitted };
    assert!(MonitoringAnalysis::run(&t, &rows, &wrong).is_err());

    // Typed levels: one group typed, the rest from the default entry.
    let typed = MonitoringRequest {
        limits_by: LimitsBy::Column(col(&t, "Room grade")),
        method: LevelMethod::Typed(vec![
            TypedLevels { group: "A".into(), alert: 0.0, action: 1.0 },
            TypedLevels { group: ANY_GROUP.into(), alert: 10.0, action: 25.0 },
        ]),
        ..base(&t, "Air colonies (CFU)")
    };
    let a = MonitoringAnalysis::run(&t, &rows, &typed).unwrap();
    assert_eq!((a.groups[0].alert, a.groups[0].action), (0.0, 1.0));
    assert_eq!((a.groups[1].alert, a.groups[1].action), (10.0, 25.0));
    let none = MonitoringRequest {
        method: LevelMethod::Typed(vec![TypedLevels { group: "A".into(), alert: 0.0, action: 1.0 }]),
        ..typed.clone()
    };
    assert!(MonitoringAnalysis::run(&t, &rows, &none).is_err(), "grade B has no levels");
    let upside = MonitoringRequest {
        method: LevelMethod::Typed(vec![TypedLevels { group: ANY_GROUP.into(), alert: 5.0, action: 1.0 }]),
        ..typed.clone()
    };
    assert!(MonitoringAnalysis::run(&t, &rows, &upside).is_err());
    let bad_q = MonitoringRequest { method: LevelMethod::Quantiles { alert: 0.99, action: 0.95 }, ..typed.clone() };
    assert!(MonitoringAnalysis::run(&t, &rows, &bad_q).is_err());
}

#[test]
fn level_columns_exclusion_and_json() {
    let mut t = clean_room();
    let n = t.n_rows();
    t.insert_column(t.n_cols(), stat_data::Column::numeric("Alert", vec![4.0; n])).unwrap();
    let mut action = vec![f64::NAN; n];
    action[5] = 12.0;
    t.insert_column(t.n_cols(), stat_data::Column::numeric("Action", action)).unwrap();
    let mut rows = RowStateManager::new(n);
    let r = MonitoringRequest {
        method: LevelMethod::Columns { alert: col(&t, "Alert"), action: col(&t, "Action") },
        limits_by: LimitsBy::All,
        rules: WarningRules { alerts_in_window: Some((3, 4)), rising_run: None },
        ..base(&t, "Air colonies (CFU)")
    };
    let a = MonitoringAnalysis::run(&t, &rows, &r).unwrap();
    assert!(a.locations.iter().all(|l| l.action.iter().all(|v| *v == 12.0)), "a missing level comes from its group");
    rows.set_excluded([0, 1, 2], true);
    let b = MonitoringAnalysis::run(&t, &rows, &r).unwrap();
    assert_eq!(b.n_excluded, 3);
    assert_eq!(location(&b, "A-01").values.len(), 39);
    for request in [
        r,
        MonitoringRequest {
            kind: ReadingKind::Positives,
            trials: SubgroupSize::Constant(5.0),
            limits_by: LimitsBy::Column(col(&t, "Room grade")),
            family: Some("binomial".into()),
            method: LevelMethod::Typed(vec![TypedLevels { group: "B".into(), alert: 0.2, action: 0.4 }]),
            signals_only: true,
            ..base(&t, "Contact plates positive")
        },
        MonitoringRequest { method: LevelMethod::Sigma { alert: 2.5, action: 3.5 }, ..MonitoringRequest::new(3, None) },
    ] {
        let back = MonitoringRequest::from_json(&request.to_json(&t), &t).unwrap();
        assert_eq!(back, request);
        let mut mapped = request.clone();
        mapped.map_columns(&|c| c);
        assert_eq!(mapped.columns(), request.columns());
    }
    let single = MonitoringAnalysis::run(&t, &RowStateManager::new(n), &MonitoringRequest::new(3, None)).unwrap();
    assert_eq!(single.locations.len(), 1);
    assert_eq!(single.title(), "Monitoring Limits: Air colonies (CFU)");
}
