//! The principal-components monitoring chart checked against an independent
//! NumPy/SciPy implementation (`tests/data/pca_chart_reference.py`): the model
//! from the eigen-decomposition of the reference correlation or covariance
//! matrix, T² and SPE with their limits, contributions, EKF cross-validation
//! and the change-point scan from determinants.

use stat_data::{JsonValue, parse_json};
use stat_quality::{
    ComponentChoice, PcaChartOptions, SpeLimitMethod, mean_change_point, pca_chart, pca_cross_validation,
};

fn cases() -> JsonValue {
    parse_json(include_str!("data/pca_chart_reference.json")).unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-9), "{what}: {a} vs {b}");
}

fn all_close(a: &[f64], b: &[f64], rel: f64, what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: lengths");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        close(*x, *y, rel, &format!("{what}[{i}]"));
    }
}

#[test]
fn matches_numpy_reference() {
    for case in cases().as_array().unwrap() {
        let x: Vec<Vec<f64>> = case.get("x").unwrap().as_array().unwrap().iter().map(nums).collect();
        let m = case.get("m").unwrap().as_f64().unwrap() as usize;
        let standardize = case.get("standardize").unwrap().as_bool().unwrap();
        let a = case.get("components").unwrap().as_f64().unwrap() as usize;
        let alpha = case.get("alpha").unwrap().as_f64().unwrap();
        let folds = case.get("folds").unwrap().as_f64().unwrap() as usize;
        let f = |k: &str| case.get(k).unwrap().as_f64().unwrap();

        let press = pca_cross_validation(&x[..m], standardize, folds, m).unwrap();
        all_close(&press, &nums(case.get("press").unwrap()), 1e-9, "PRESS");

        let options =
            PcaChartOptions { standardize, components: ComponentChoice::Fixed(a), alpha, ..Default::default() };
        let chart = pca_chart(&x, Some(m), &options).unwrap();
        all_close(&chart.eigenvalues, &nums(case.get("eigenvalues").unwrap()), 1e-9, "eigenvalues");
        let t2: Vec<f64> = chart.t2.iter().map(|p| p.statistic).collect();
        all_close(&t2, &nums(case.get("t2").unwrap()), 1e-8, "T²");
        let spe: Vec<f64> = chart.spe.iter().map(|p| p.statistic).collect();
        all_close(&spe, &nums(case.get("spe").unwrap()), 1e-8, "SPE");
        close(chart.t2_reference_limit, f("t2_reference_limit"), 1e-9, "T² reference limit");
        close(chart.t2_monitoring_limit, f("t2_monitoring_limit"), 1e-9, "T² monitoring limit");
        close(chart.spe_limit, f("spe_jackson_mudholkar"), 1e-9, "Jackson–Mudholkar limit");
        let boxed = pca_chart(&x, Some(m), &PcaChartOptions { spe_method: SpeLimitMethod::ScaledChiSquare, ..options })
            .unwrap();
        close(boxed.spe_limit, f("spe_box"), 1e-8, "scaled χ² limit");

        let i = case.get("contribution_point").unwrap().as_f64().unwrap() as usize;
        let ct = chart.t2_contributions(i).unwrap();
        all_close(&ct, &nums(case.get("t2_contributions").unwrap()), 1e-8, "T² contributions");
        close(ct.iter().sum(), t2[i], 1e-10, "T² contributions add up");
        all_close(&chart.spe_contributions(i).unwrap(), &nums(case.get("spe_contributions").unwrap()), 1e-8, "SPE c.");

        let cp = mean_change_point(&chart.scores, 99, 7).unwrap();
        all_close(&cp.statistics, &nums(case.get("change_statistics").unwrap()), 1e-7, "change statistics");
        assert_eq!(cp.before, case.get("change_before").unwrap().as_f64().unwrap() as usize);

        let cv = pca_chart(
            &x,
            Some(m),
            &PcaChartOptions { components: ComponentChoice::CrossValidation { folds }, ..options },
        )
        .unwrap();
        assert_eq!(cv.components, case.get("best_components").unwrap().as_f64().unwrap() as usize);
    }
}
