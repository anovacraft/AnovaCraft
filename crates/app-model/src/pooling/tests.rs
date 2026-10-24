//! Pooled analyses checked against R mice 3.16 on the same imputed data:
//! mice's five imputations of its `nhanes2` data (seed 123), stacked in
//! `tests/data/mice_nhanes2_stacked.csv`. Expected values are printed by
//! `tests/data/mice_reference.R`; logistic
//! models there are fitted to full convergence.

#![allow(clippy::excessive_precision)] // reference values are printed to 17 digits

use super::*;
use crate::compare::OneSampleRequest;
use crate::glm_analysis::GlmRequest;
use crate::report::ReportOptions;
use crate::surface_profiler::ProfilerOptions;
use stat_discrete::Family;

const IMPUTATION: usize = 0;
const AGE: usize = 2;
const BMI: usize = 3;
const HYP: usize = 4;
const CHL: usize = 5;

fn nhanes() -> (DataTable, RowStateManager) {
    let table = stat_data::read_csv("nhanes2", include_str!("../../tests/data/mice_nhanes2_stacked.csv")).unwrap();
    let rows = RowStateManager::new(table.n_rows());
    (table, rows)
}

fn close(got: f64, want: f64, tol: f64, what: &str) {
    let scale = want.abs().max(1e-300);
    assert!(((got - want) / scale).abs() < tol, "{what}: got {got}, want {want}");
}

fn linear_pieces() -> Pieces {
    let (table, rows) = nhanes();
    let spec =
        crate::LinearModelSpec { response: CHL, predictors: vec![AGE, BMI], effects: Vec::new(), splines: Vec::new() };
    let report = SavedReport::Linear { spec, options: ReportOptions::default() };
    pieces(&report, &table, &rows, IMPUTATION).unwrap()
}

fn position(p: &Pieces, name: &str) -> usize {
    p.names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no {name} in {:?}", p.names))
}

#[test]
fn linear_model_coefficient_matches_mice_pool() {
    let p = linear_pieces();
    assert_eq!(p.numbers, vec![1, 2, 3, 4, 5]);
    assert!(p.fits.iter().all(|f| f.n == 25 && f.df_complete == 21.0));
    let e = &estimates(&p).unwrap()[position(&p, "bmi")];
    close(e.estimate, 5.0919114040737021, 1e-10, "estimate");
    close(e.within, 3.1234189363821003, 1e-10, "ubar");
    close(e.between, 0.82731977756599828, 1e-9, "b");
    close(e.total, 4.1162026694612983, 1e-10, "t");
    close(e.df, 12.04777408594752, 1e-9, "df");
    close(e.riv, 0.31785160854187339, 1e-9, "riv");
    close(e.fmi, 0.34204278283636191, 1e-9, "fmi");
    close(e.std_error, 2.0288426921428133, 1e-10, "se");
    close(e.p_value, 0.027351510272947566, 1e-8, "p");
}

#[test]
fn linear_model_d1_and_d3_match_mice() {
    let p = linear_pieces();
    let tests = effect_tests(&p).unwrap();
    let age = tests.iter().find(|t| t.name == "age").unwrap();
    assert_eq!(age.parameters, 2);
    close(age.wald.statistic, 2.7957941685818337, 1e-9, "age D1");
    close(age.wald.df_den, 4.8160369694140162, 1e-9, "age D1 df");
    close(age.wald.p_value, 0.15636088210312116, 1e-8, "age D1 p");
    close(age.wald.riv, 1.2874928795702962, 1e-9, "age D1 riv");
    let d3 = age.likelihood_ratio.as_ref().unwrap();
    close(d3.statistic, 2.0461517502327413, 1e-9, "age D3");
    close(d3.df_den, 12.576697552481225, 1e-9, "age D3 df");
    close(d3.p_value, 0.1700845307109303, 1e-8, "age D3 p");
    close(d3.riv, 1.6153350338014891, 1e-9, "age D3 riv");
    let whole = tests.iter().find(|t| t.name == "Whole model").unwrap();
    assert_eq!(whole.parameters, 3);
    close(whole.wald.statistic, 2.7422735568558476, 1e-9, "model D1");
    close(whole.wald.df_den, 7.4328531756676881, 1e-9, "model D1 df");
    close(whole.wald.p_value, 0.11815476862413143, 1e-8, "model D1 p");
    let d3 = whole.likelihood_ratio.as_ref().unwrap();
    close(d3.statistic, 2.0096666760997648, 1e-9, "model D3");
    close(d3.df_den, 28.19579758037915, 1e-9, "model D3 df");
    close(d3.p_value, 0.13523199162891084, 1e-8, "model D3 p");
}

#[test]
fn linear_model_r_squared_matches_mice_pool_r_squared() {
    let p = linear_pieces();
    let n: Vec<usize> = p.fits.iter().map(|f| f.n).collect();
    let r2: Vec<f64> = p.fits.iter().map(|f| f.r_squared.unwrap().0).collect();
    let c = pool_r_squared(&r2, &n, LEVEL).unwrap();
    close(c.estimate, 0.4419414802881918, 1e-10, "R²");
    // mice rounds the normal quantile to 1.96 (interval 0.032908478478099225
    // to 0.79110713556703771); these use the exact quantile, as the app does.
    close(c.interval.0, 0.03291246247316907, 1e-9, "R² lower");
    close(c.interval.1, 0.79110291636688002, 1e-10, "R² upper");
    close(c.z.fmi, 0.59781026672996929, 1e-9, "R² fmi");
    let adj: Vec<f64> = p.fits.iter().map(|f| f.r_squared.unwrap().1).collect();
    let c = pool_r_squared(&adj, &n, LEVEL).unwrap();
    close(c.estimate, 0.35612414239947299, 1e-10, "adjusted R²");
}

#[test]
fn logistic_regression_matches_mice_pool() {
    let (table, rows) = nhanes();
    let request = GlmRequest {
        response: HYP,
        predictors: vec![BMI, CHL],
        family: Family::Binomial,
        link: None,
        event: None,
        effects: Vec::new(),
        splines: Vec::new(),
        slices: None,
        slopes: None,
    };
    let report = SavedReport::Glm { request, profilers: ProfilerOptions::default() };
    let p = pieces(&report, &table, &rows, IMPUTATION).unwrap();
    assert!(p.fits.iter().all(|f| f.df_complete == 22.0));
    let pooled = estimates(&p).unwrap();
    let expected = [
        (stat_linear::INTERCEPT_NAME, -2.4407446582195971, 32.491211961262593, 5.851845025879979, 0.62903017209988943),
        ("bmi", -0.071103157056658456, 0.026693835572441371, 8.3100825413795807, 0.50115321551423198),
        ("chl", 0.016451663859388864, 0.00028030808587338512, 8.8947279604646958, 0.47532967614554683),
    ];
    for (name, estimate, total, df, fmi) in expected {
        let e = &pooled[position(&p, name)];
        close(e.estimate, estimate, 1e-7, name);
        close(e.total, total, 1e-7, name);
        close(e.df, df, 1e-7, name);
        close(e.fmi, fmi, 1e-7, name);
    }
    let chl = effect_tests(&p).unwrap().into_iter().find(|t| t.name == "chl").unwrap();
    close(chl.wald.statistic, 0.9655705895854082, 1e-7, "chl D1");
    close(chl.wald.df_den, 4.0, 1e-12, "chl D1 df");
    close(chl.wald.p_value, 0.38142128491962274, 1e-7, "chl D1 p");
}

#[test]
fn one_sample_mean_matches_mice_pool_of_an_intercept_model() {
    let (table, rows) = nhanes();
    let report = SavedReport::OneSample { request: OneSampleRequest::new(CHL) };
    let p = pieces(&report, &table, &rows, IMPUTATION).unwrap();
    let e = &estimates(&p).unwrap()[0];
    close(e.estimate, 191.952, 1e-12, "mean");
    close(e.total, 76.020010666666479, 1e-10, "t");
    close(e.df, 20.854622392789221, 1e-9, "df");
    close(e.fmi, 0.12912878928730498, 1e-9, "fmi");
}

#[test]
fn sections_list_pooled_results_and_errors_become_a_note() {
    let (table, rows) = nhanes();
    let spec =
        crate::LinearModelSpec { response: CHL, predictors: vec![AGE, BMI], effects: Vec::new(), splines: Vec::new() };
    let report = SavedReport::Linear { spec, options: ReportOptions::default() };
    let kinds: Vec<SectionKind> = pooled_sections(&report, &table, &rows, IMPUTATION).iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [
            SectionKind::PooledNote,
            SectionKind::PooledEstimates,
            SectionKind::PooledVariance,
            SectionKind::PooledTests,
            SectionKind::PooledFit,
            SectionKind::PooledByImputation
        ]
    );
    assert_eq!(imputation_count(&table, IMPUTATION, &rows), 5);
    // Only the original rows: nothing to pool.
    let v = table.column(IMPUTATION).unwrap().as_numeric().unwrap().to_vec();
    let mut only_original = rows.clone();
    only_original.set_excluded((0..v.len()).filter(|&r| v[r] > 0.0), true);
    let s = pooled_sections(&report, &table, &only_original, IMPUTATION);
    assert_eq!(s.len(), 1);
    assert!(matches!(&s[0].content, SectionContent::Note(t) if t.contains("at least two imputations")));
    assert!(can_pool(&report));
}

#[test]
fn correlations_pool_on_the_fisher_z_scale() {
    let (table, rows) = nhanes();
    let request = crate::CorrelationRequest::new(vec![BMI, CHL]);
    let report = SavedReport::Correlations { request };
    let p = pieces(&report, &table, &rows, IMPUTATION).unwrap();
    assert!(p.correlations && p.names == ["bmi with chl"]);
    let r: Vec<f64> = p.fits.iter().map(|f| f.values[0]).collect();
    let mean_z = r.iter().map(|r| r.atanh()).sum::<f64>() / r.len() as f64;
    let pooled = pool_correlation(&r, &[25; 5], LEVEL).unwrap();
    close(pooled.estimate, mean_z.tanh(), 1e-12, "pooled r");
}

#[test]
fn correlation_variance_table_uses_the_fisher_z_scale() {
    let (table, rows) = nhanes();
    let request = crate::CorrelationRequest::new(vec![BMI, CHL]);
    let report = SavedReport::Correlations { request };
    let s = pooled_sections(&report, &table, &rows, IMPUTATION);
    let find = |kind: SectionKind| s.iter().find(|x| x.kind == kind).unwrap();
    let SectionContent::Table(est) = &find(SectionKind::PooledEstimates).content else { panic!() };
    let variance = find(SectionKind::PooledVariance);
    assert!(variance.title.contains("Fisher z scale"), "{}", variance.title);
    let SectionContent::Table(var) = &variance.content else { panic!() };
    let fmi = |t: &TextTable| t.cell(0, "Fraction of missing information").unwrap().parse::<f64>().unwrap();
    close(fmi(var), fmi(est), 1e-12, "the two tables agree on the fraction of missing information");
    // The within-imputation variance is 1/(n − 3) on the z scale.
    let p = pieces(&report, &table, &rows, IMPUTATION).unwrap();
    let r: Vec<f64> = p.fits.iter().map(|f| f.values[0]).collect();
    let n: Vec<usize> = p.fits.iter().map(|f| (1.0 / f.covariance[0][0] + 3.0).round() as usize).collect();
    let z = pool_correlation(&r, &n, LEVEL).unwrap().z;
    assert_eq!(var.cell(0, "Within (Ū)"), Some(format_number(z.within, 4).as_str()));
    assert_eq!(var.cell(0, "Between (B)"), Some(format_number(z.between, 4).as_str()));
}
