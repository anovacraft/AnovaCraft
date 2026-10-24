use super::*;
use crate::project::{Project, SavedReport, load_project, save_project};
use crate::report_scope::ReportScope;
use crate::table_ops::{Source, TableOperation, TableOrigin};
use stat_core::rng::Rng;

/// 80 rows: `x` continuous, `g` text categories, `y = 2 + x + (g = b) + e`
/// with `y` missing more often for large `x` and some `x` and `g` missing.
fn table() -> DataTable {
    let mut rng = Rng::new(17);
    let n = 80;
    let (mut x, mut g, mut y) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..n {
        let xi = rng.normal();
        let gi = if rng.next_f64() < 0.4 { "b" } else { "a" };
        let yi = 2.0 + xi + if gi == "b" { 1.0 } else { 0.0 } + 0.5 * rng.normal();
        y.push(if rng.next_f64() < 0.15 + 0.2 * f64::from(xi > 0.5) { f64::NAN } else { yi });
        x.push(if i % 11 == 3 { f64::NAN } else { xi });
        g.push(if i % 13 == 5 { String::new() } else { gi.to_owned() });
    }
    DataTable::new("Trial", vec![Column::numeric("x", x), Column::text("g", g), Column::numeric("y", y)]).unwrap()
}

fn settings(m: usize) -> MiSettings {
    MiSettings { imputations: m, iterations: 5, ..MiSettings::default() }
}

#[test]
fn stacked_table_keeps_observed_values_and_fills_the_rest() {
    let t = table();
    let rows = RowStateManager::new(t.n_rows());
    let run = MiRun::run(&t, &rows, &[0, 1, 2], &settings(3), 7).unwrap();
    let (s, source) = run.stacked(&t, "stacked").unwrap();
    let n = t.n_rows();
    assert_eq!(s.n_rows(), 4 * n);
    assert_eq!(source.len(), 4 * n);
    let names: Vec<&str> = s.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Imputation", "Row", "x", "g", "y"]);
    assert_eq!(imputation_column(&s), Some(0));
    assert_eq!(imputation_column(&t), None);
    let imp = s.column(0).unwrap().as_numeric().unwrap();
    let y0 = t.column(2).unwrap().as_numeric().unwrap();
    let ys = s.column(4).unwrap().as_numeric().unwrap();
    let gs = s.column(3).unwrap();
    for k in 0..s.n_rows() {
        let r = source[k];
        assert_eq!(imp[k], (k / n) as f64);
        if y0[r].is_finite() {
            assert_eq!(ys[k], y0[r]);
        } else {
            assert_eq!(ys[k].is_finite(), imp[k] > 0.0, "row {k}");
        }
        let g0 = t.column(1).unwrap().cell_text(r);
        let g = gs.cell_text(k);
        if g0.is_empty() {
            assert_eq!(g.is_empty(), imp[k] == 0.0);
            assert!(g.is_empty() || g == "a" || g == "b");
        } else {
            assert_eq!(g, g0);
        }
    }
    // Imputation l of the stacked table is the run's l-th completed data set.
    let missing_y = (0..n).find(|&r| !y0[r].is_finite()).unwrap();
    assert_eq!(ys[2 * n + missing_y], run.result.imputations[1][missing_y][2]);
}

#[test]
fn categorical_numeric_columns_are_imputed_with_their_own_values() {
    let mut t = table();
    let codes: Vec<f64> =
        (0..t.n_rows()).map(|r| if r % 9 == 2 { f64::NAN } else { (r % 3 + 1) as f64 * 10.0 }).collect();
    let mut c = Column::numeric("dose", codes);
    c.level = MeasurementLevel::Ordinal;
    t = DataTable::new("Trial", t.columns().iter().cloned().chain([c]).collect()).unwrap();
    let rows = RowStateManager::new(t.n_rows());
    let run = MiRun::run(&t, &rows, &[0, 3], &settings(2), 3).unwrap();
    assert_eq!(run.methods[1], FcsMethod::Multinomial);
    let (s, _) = run.stacked(&t, "s").unwrap();
    let dose = s.column(s.find("dose").unwrap()).unwrap().as_numeric().unwrap();
    let n = t.n_rows();
    assert!(dose[n..].iter().all(|d| [10.0, 20.0, 30.0].contains(d)), "imputed doses are observed levels");
}

#[test]
fn excluded_rows_are_not_imputed_and_keep_their_state() {
    let t = table();
    let mut rows = RowStateManager::new(t.n_rows());
    let y = t.column(2).unwrap().as_numeric().unwrap();
    let gap = (0..t.n_rows()).find(|&r| !y[r].is_finite()).unwrap();
    rows.set_excluded([gap], true);
    let op =
        TableOperation::MultipleImputation { columns: vec!["x".into(), "y".into()], settings: settings(2), seed: 5 };
    let doc = op.run(Source { table: &t, rows: &rows }, &[], "Trial, 2 imputations").unwrap();
    let n = t.n_rows();
    let ys = doc.table.column(doc.table.find("y").unwrap()).unwrap().as_numeric().unwrap();
    for l in 0..3 {
        assert!(doc.rows.is_excluded(l * n + gap));
        assert!(ys[l * n + gap].is_nan());
    }
    assert!(!doc.notes.is_empty());
    let back = TableOrigin::from_json(&doc.origin.to_json()).unwrap();
    assert_eq!(back, doc.origin);
    assert_eq!(op.default_name(&[]), "2 imputations");
    assert!(doc.origin.describe().contains("2 imputations of 2 columns"));
}

#[test]
fn settings_round_trip_through_json() {
    let s = MiSettings {
        engine: MiEngine::ChainedEquations,
        imputations: 7,
        iterations: 12,
        donors: 3,
        methods: vec![Some(FcsMethod::BayesianRegression), None, Some(FcsMethod::Logistic)],
        predictors: Some(vec![vec![false, true, true], vec![true, false, false], vec![true, true, false]]),
        visit_order: Some(vec![2, 0, 1]),
    };
    let json = JsonValue::Object(s.json_members());
    assert_eq!(MiSettings::from_json(&json).unwrap(), s);
    let normal = MiSettings { engine: MiEngine::NormalModel, iterations: 50, ..MiSettings::default() };
    assert_eq!(MiSettings::from_json(&JsonValue::Object(normal.json_members())).unwrap(), normal);
    assert_eq!(MiSettings::from_json(&JsonValue::Object(Vec::new())).unwrap(), MiSettings::default());
}

#[test]
fn bad_settings_are_reported() {
    let t = table();
    let rows = RowStateManager::new(t.n_rows());
    let normal = MiSettings { engine: MiEngine::NormalModel, ..settings(2) };
    let e = MiRun::run(&t, &rows, &[0, 1], &normal, 1).unwrap_err().to_string();
    assert!(e.contains("categorical"), "{e}");
    let wrong = MiSettings { methods: vec![Some(FcsMethod::Logistic)], ..settings(2) };
    let e = MiRun::run(&t, &rows, &[0, 1], &wrong, 1).unwrap_err().to_string();
    assert!(e.contains("cannot be imputed"), "{e}");
    assert!(MiRun::run(&t, &rows, &[], &settings(2), 1).is_err());
    assert!(MiRun::run(&t, &rows, &[0], &settings(0), 1).is_err());
}

fn linear() -> SavedReport {
    // On the stacked table: y (4) on x (2) and g (3).
    SavedReport::Linear {
        spec: crate::LinearModelSpec { response: 4, predictors: vec![2, 3], effects: Vec::new(), splines: Vec::new() },
        options: crate::ReportOptions::default(),
    }
}

#[test]
fn pooled_report_puts_pooled_sections_before_the_complete_case_analysis() {
    let t = table();
    let rows = RowStateManager::new(t.n_rows());
    let op = TableOperation::MultipleImputation {
        columns: vec!["x".into(), "g".into(), "y".into()],
        settings: settings(4),
        seed: 9,
    };
    let doc = op.run(Source { table: &t, rows: &rows }, &[], "Trial imputed").unwrap();
    let scope = ReportScope { pool: Some(IMPUTATION_COLUMN.to_owned()), ..ReportScope::default() };
    let pooled = crate::run_report(&linear().with_scope(scope.clone()), &doc.table, &doc.rows).unwrap();
    assert_eq!(pooled.sections[0].kind, SectionKind::PooledNote);
    assert!(pooled.sections.iter().any(|s| s.kind == SectionKind::PooledTests));
    // The sections after the pooled ones are the fit to the original rows.
    let first = pooled
        .sections
        .iter()
        .position(|s| {
            !s.title.contains("Pooled")
                && s.kind != SectionKind::PooledVariance
                && s.kind != SectionKind::PooledByImputation
                && s.kind != SectionKind::PooledFit
        })
        .unwrap();
    let mut original = doc.rows.clone();
    original.set_excluded(t.n_rows()..doc.table.n_rows(), true);
    let complete_case = crate::run_report(&linear(), &doc.table, &original).unwrap();
    assert_eq!(pooled.sections[first..], complete_case.sections[..]);
    assert!(scope.narrows() && !scope.is_filtered());

    // Saved in a project and reopened, the derived table and pooled report
    // are made again exactly from the seed.
    let project = Project {
        external: None,
        table: doc.table.clone(),
        rows: doc.rows.clone(),
        reports: vec![linear().with_scope(scope)],
        origin: Some(doc.origin.clone()),
        scripts: Vec::new(),
    };
    let loaded = load_project(&save_project(&project)).unwrap();
    assert_eq!(loaded.reports, project.reports);
    let origin = loaded.origin.unwrap();
    let again = origin.operation.run(Source { table: &t, rows: &rows }, &[], "Trial imputed").unwrap();
    assert_eq!(again.table, doc.table);
    let rerun = crate::run_report(&loaded.reports[0], &again.table, &again.rows).unwrap();
    assert_eq!(rerun.sections, pooled.sections);
}

#[test]
fn imputation_report_shows_diagnostics_and_offers_its_stacked_table() {
    let t = table();
    let rows = RowStateManager::new(t.n_rows());
    let request = crate::ImputationRequest {
        method: crate::ImputationMethod::ChainedEquations,
        multiple: settings(3),
        ..crate::ImputationRequest::new(vec![0, 1, 2])
    };
    let report = crate::ImputationReport::run(&t, &rows, &request).unwrap();
    let kinds: Vec<SectionKind> = report.sections().iter().map(|s| s.kind).collect();
    for k in [
        SectionKind::ImputationModels,
        SectionKind::ImputationPooledMeans,
        SectionKind::ImputationTrace,
        SectionKind::ImputationStrip,
        SectionKind::ImputationDensity,
        SectionKind::ImputationCategories,
    ] {
        assert!(kinds.contains(&k), "{k:?} in {kinds:?}");
    }
    assert!(report.saveable_columns().is_empty());
    let Some(TableOperation::MultipleImputation { columns, settings: s, seed }) = report.stacked_operation(&t) else {
        panic!("no stacked table")
    };
    assert_eq!(columns, ["x", "g", "y"]);
    assert_eq!((s.imputations, seed), (3, request.seed));
    // The request round-trips through a project.
    let json = request.to_json(&t);
    assert_eq!(crate::ImputationRequest::from_json(&json, &t).unwrap(), request);
}
