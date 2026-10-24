use super::*;
use crate::project::SavedReport;
use crate::sample::{treatment_study, treatment_study_sample};

fn request(t: &DataTable) -> TreatmentEffectsRequest {
    let c = |n: &str| t.find(n).unwrap();
    let mut r = TreatmentEffectsRequest::new(
        c("Programme"),
        c("Fitness gain"),
        OutcomeKind::Continuous,
        ["Age", "Sex", "Severity", "Smoker", "Region"].iter().map(|n| c(n)).collect(),
    );
    r.treated_level = Some("Enrolled".to_owned());
    r
}

/// The table's coding gives the same propensity model and IPW estimate as
/// the engine's reference (statsmodels; `stat-discrete/tests/data`), whose
/// indicators are Sex = M, Smoker = Yes, Region = North and Region = South.
#[test]
fn report_matches_the_engine_reference() {
    let t = treatment_study();
    let a = TreatmentEffectsAnalysis::run(&t, &RowStateManager::new(t.n_rows()), &request(&t)).unwrap();
    assert_eq!(a.model_terms(), ["Age", "Sex = M", "Severity", "Smoker = Yes", "Region = North", "Region = South"]);
    // Reference: IPW ATE on fitness gain, 2.398898 ± 0.327893.
    let f = a.fit();
    assert!((f.mean_treated - 12.719_986_993_065_922).abs() < 1e-8);
    assert!((f.mean_control - 10.321_089_306_526_085).abs() < 1e-8);
    assert!((f.effects[0].std_error - 0.327_893_369_240_681_7).abs() < 1e-6);
    assert_eq!(f.n, (170, 230));
    let s = a.sections();
    let titles: Vec<&str> = s.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles[0], "Summary");
    assert!(titles.contains(&"Balance Plot") && titles.contains(&"Propensity Score Overlap"));
    // Balance lists every level of a categorical covariate, then the score.
    let names: Vec<&str> = a.balance().iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names.len(), 2 + 2 + 2 + 3 + 1, "{names:?}");
    assert_eq!(*names.last().unwrap(), "Propensity score");
    assert!(a.balance().iter().all(|b| b.adjusted_smd.abs() < 0.1), "weighting balances the covariates");
}

#[test]
fn treated_level_defaults_to_the_second_value_and_can_be_named() {
    let t = treatment_study();
    let rows = RowStateManager::new(t.n_rows());
    let mut r = request(&t);
    r.treated_level = None;
    let a = TreatmentEffectsAnalysis::run(&t, &rows, &r).unwrap();
    // "Usual care" sorts after "Enrolled".
    assert_eq!(a.fit().n, (230, 170));
    r.treated_level = Some("Nobody".to_owned());
    assert!(TreatmentEffectsAnalysis::run(&t, &rows, &r).is_err());
}

#[test]
fn subgroups_are_refitted_within_each_level() {
    let t = treatment_study();
    let rows = RowStateManager::new(t.n_rows());
    let mut r = request(&t);
    r.subgroups = vec![t.find("Age group").unwrap(), t.find("Sex").unwrap()];
    let a = TreatmentEffectsAnalysis::run(&t, &rows, &r).unwrap();
    let levels: Vec<(&str, &str)> = a.subgroups().iter().map(|s| (s.column.as_str(), s.level.as_str())).collect();
    assert_eq!(levels, [("Age group", "65 and over"), ("Age group", "Under 65"), ("Sex", "F"), ("Sex", "M")]);
    // Each subgroup estimate equals the analysis run on that subgroup alone.
    let sex = t.find("Sex").unwrap();
    let mut only_f = RowStateManager::new(t.n_rows());
    only_f.set_excluded((0..t.n_rows()).filter(|&i| t.column(sex).unwrap().level_text(i) != "F"), true);
    let mut plain = request(&t);
    plain.covariates.retain(|&c| c != sex);
    let alone = TreatmentEffectsAnalysis::run(&t, &only_f, &plain).unwrap();
    let in_report = a.subgroups()[2].fit.as_ref().unwrap();
    assert!((in_report.effects[0].estimate - alone.fit().effects[0].estimate).abs() < 1e-12);
    // Younger patients gain more from the programme (4 vs 1.5 points).
    let effect = |i: usize| a.subgroups()[i].fit.as_ref().unwrap().effects[0].estimate;
    assert!(effect(1) > effect(0) + 1.0, "{} vs {}", effect(1), effect(0));
    let s = a.sections();
    let SectionContent::Table(sub) = &s.iter().find(|s| s.kind == SectionKind::SubgroupEffects).unwrap().content else {
        panic!()
    };
    let tests: Vec<&Vec<String>> = sub.rows.iter().filter(|r| r[1] == "Test of equal effects").collect();
    assert_eq!(tests.len(), 2);
    assert!(tests[0][2].starts_with("Q = "), "{:?}", tests[0]);
    assert!(s.iter().any(|s| s.kind == SectionKind::SubgroupForest));
}

#[test]
fn saved_columns_follow_the_rows_used() {
    let t = treatment_study();
    let mut rows = RowStateManager::new(t.n_rows());
    rows.set_excluded([0, 5], true);
    let mut r = request(&t);
    r.method = EffectMethod::Matching(MatchingOptions::default());
    r.estimand = Estimand::Att;
    let a = TreatmentEffectsAnalysis::run(&t, &rows, &r).unwrap();
    let cols = a.saveable_columns();
    let keys: Vec<&str> = cols.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["propensity", "weight", "matched_set"]);
    assert!(cols[0].values[0].is_nan() && cols[0].values[5].is_nan() && cols[0].values[1].is_finite());
    // Each matched set holds one enrolled patient and one control.
    let sets = &cols[2].values;
    let max = sets.iter().filter(|v| v.is_finite()).fold(0.0f64, |a, &b| a.max(b));
    assert!(max > 100.0);
    let saved = SavedReport::TreatmentEffects { request: r };
    assert_eq!(crate::report_columns::report_columns(&saved, &t, &rows).unwrap().len(), 3);
}

#[test]
fn json_round_trip_keeps_every_setting() {
    let t = treatment_study();
    let mut r = request(&t);
    r.method = EffectMethod::Matching(MatchingOptions { ratio: 2, replace: true, caliper: None });
    r.estimand = Estimand::Att;
    r.stabilized = false;
    r.trim = 0.05;
    r.intervals = IntervalMethod::Bootstrap { replicates: 200, seed: 9 };
    r.level = 0.9;
    r.outcome_kind = OutcomeKind::Count;
    r.outcome = t.find("Clinic visits").unwrap();
    r.subgroups = vec![t.find("Region").unwrap()];
    let saved = SavedReport::TreatmentEffects { request: r.clone() };
    let json = crate::project::report_to_json(&saved, &t);
    let back = crate::project::report_from_json(&json, &t).unwrap();
    assert_eq!(back, saved);
}

#[test]
fn sample_reports_run() {
    let s = treatment_study_sample();
    let rows = RowStateManager::new(s.table.n_rows());
    for r in &s.reports {
        let c = crate::run_report(r, &s.table, &rows).unwrap();
        assert!(c.title.starts_with("Treatment Effect: "), "{}", c.title);
    }
}

#[test]
fn bad_requests_are_errors() {
    let t = treatment_study();
    let rows = RowStateManager::new(t.n_rows());
    let mut r = request(&t);
    r.covariates.clear();
    assert!(TreatmentEffectsAnalysis::run(&t, &rows, &r).is_err());
    let mut r = request(&t);
    r.treatment = t.find("Region").unwrap();
    r.covariates.retain(|&c| c != r.treatment);
    assert!(TreatmentEffectsAnalysis::run(&t, &rows, &r).is_err(), "three values");
    let mut r = request(&t);
    r.covariates.push(r.outcome);
    assert!(TreatmentEffectsAnalysis::run(&t, &rows, &r).is_err());
}
