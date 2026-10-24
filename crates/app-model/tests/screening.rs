//! The screening report against R 4.3.3 and MASS (`lm`/`drop1`, `glm`,
//! `glm.nb`, `kruskal.test`, `p.adjust`); the script is
//! `tests/data/screening_reference.R`.
#![allow(clippy::excessive_precision)]

use app_model::report::{SectionContent, SectionKind};
use app_model::{FdrMethod, SavedReport, ScreeningAnalysis, ScreeningModel, ScreeningRequest};
use stat_data::{Column, DataTable, RowStateManager};

const Y1: [f64; 24] = [
    11.369999999999999,
    9.4399999999999995,
    10.359999999999999,
    10.630000000000001,
    10.4,
    9.8900000000000006,
    11.51,
    9.9100000000000001,
    12.02,
    9.9399999999999995,
    11.300000000000001,
    12.289999999999999,
    8.6099999999999994,
    9.7200000000000006,
    9.8699999999999992,
    10.640000000000001,
    9.7200000000000006,
    7.3399999999999999,
    7.5599999999999996,
    11.32,
    9.6899999999999995,
    8.2200000000000006,
    9.8300000000000001,
    11.210000000000001,
];
const Y2: [f64; 24] = [
    11.9,
    9.5700000000000003,
    9.7400000000000002,
    8.2400000000000002,
    10.460000000000001,
    9.3599999999999994,
    10.460000000000001,
    10.699999999999999,
    11.039999999999999,
    9.3900000000000006,
    10.5,
    8.2799999999999994,
    10.02,
    9.9499999999999993,
    8.3900000000000006,
    10.84,
    11.01,
    10.44,
    11.56,
    10.07,
    9.4299999999999997,
    11.23,
    9.9900000000000002,
    12.24,
];
const Y3: [f64; 24] = [
    9.5700000000000003,
    10.66,
    10.32,
    9.2200000000000006,
    11.58,
    10.640000000000001,
    10.09,
    10.279999999999999,
    10.68,
    10.09,
    7.0099999999999998,
    10.279999999999999,
    11.23,
    11.789999999999999,
    12.18,
    13.0,
    10.869999999999999,
    12.9,
    11.94,
    12.640000000000001,
    12.52,
    12.32,
    10.56,
    11.51,
];
const Y4: [f64; 24] = [
    10.619999999999999,
    9.0500000000000007,
    9.4600000000000009,
    10.58,
    10.77,
    10.460000000000001,
    9.1099999999999994,
    8.9000000000000004,
    11.51,
    10.26,
    10.09,
    9.8800000000000008,
    11.210000000000001,
    13.01,
    12.18,
    12.220000000000001,
    13.33,
    13.220000000000001,
    13.789999999999999,
    11.92,
    13.050000000000001,
    13.789999999999999,
    11.289999999999999,
    11.539999999999999,
];
const K1: [f64; 24] = [
    8.0, 5.0, 3.0, 5.0, 0.0, 4.0, 1.0, 14.0, 3.0, 8.0, 3.0, 0.0, 8.0, 2.0, 0.0, 8.0, 6.0, 2.0, 8.0, 3.0, 3.0, 0.0, 1.0,
    3.0,
];
const K2: [f64; 24] = [
    12.0, 2.0, 7.0, 4.0, 1.0, 8.0, 1.0, 6.0, 3.0, 3.0, 11.0, 4.0, 4.0, 6.0, 3.0, 4.0, 4.0, 7.0, 12.0, 2.0, 11.0, 1.0,
    2.0, 5.0,
];
const K3: [f64; 24] = [
    1.0, 0.0, 5.0, 5.0, 5.0, 2.0, 4.0, 6.0, 1.0, 7.0, 10.0, 2.0, 30.0, 11.0, 11.0, 10.0, 1.0, 10.0, 5.0, 6.0, 11.0,
    10.0, 7.0, 2.0,
];
const K4: [f64; 24] = [
    5.0, 3.0, 6.0, 6.0, 1.0, 5.0, 7.0, 5.0, 1.0, 5.0, 9.0, 7.0, 3.0, 6.0, 6.0, 24.0, 10.0, 14.0, 5.0, 22.0, 6.0, 5.0,
    16.0, 13.0,
];
const LIN_TRT_F: [f64; 4] = [7.2717424601074958, 1.1058744142618029, 23.789844755193975, 46.740924879611711];
const LIN_TRT_P: [f64; 4] = [0.013880349590196638, 0.30552132618800293, 9.113830781750195e-05, 1.2067575125345744e-06];
const LIN_BATCH_P: [f64; 4] = [0.64855707582844391, 0.53863782543212035, 0.35397352309603147, 0.61368924446669304];
const LIN_TRT_Q: [f64; 4] = [0.018507132786928851, 0.30552132618800293, 0.0001822766156350039, 4.8270300501382977e-06];
const LIN_BATCH_Q_BY: [f64; 4] = [1.0, 1.0, 1.0, 1.0];
const LIN_TRT_DIFF: [f64; 4] = [-1.2775000000000001, 0.46083333333333282, 1.9200000000000006, 2.4883333333333324];
const LIN_TRT_LOWER: [f64; 4] = [-2.2657079834258438, -0.45327501246530338, 1.0988696232388193, 1.7291151508700113];
const LIN_BATCH_RANGE: [f64; 4] = [0.52374999999999949, 0.5900000000000013, 0.70500000000000063, 0.4025000000000008];
const POI_TRT_LR: [f64; 4] = [1.0221863840657974, 0.008130170867154618, 27.687017167232341, 26.407085776898008];
const POI_TRT_P: [f64; 4] = [0.3120009535314191, 0.92815414201879243, 1.4261665389873584e-07, 2.7652420986817003e-07];
const POI_TRT_RATIO: [f64; 4] = [0.81481481481463902, 0.98387096774192651, 2.3749999999999698, 2.1666666666666679];
const POI_TRT_UPPER: [f64; 4] = [1.2131929901880294, 1.4010180357951283, 3.3275368468610185, 2.9419968657347555];
const NB_TRT_LR: [f64; 4] = [0.49151942036189666, 0.01430574273349805, 7.3350491348240041, 8.7064529944139508];
const NB_TRT_P: [f64; 4] = [0.48325030763450111, 0.90479477807023601, 0.0067622830937147087, 0.0031708557526380567];
const NB_THETA: [f64; 4] = [2.6169920374360989, 4.4209109219703002, 3.2520845074169258, 5.2257034550869736];
const KW_LIVER_H: [f64; 4] = [8.3076923076923066, 1.2608187134502915, 5.0256410256410291, 6.5641025641025621];
const KW_LIVER_P: [f64; 4] = [0.0039477518569034564, 0.26149617619114618, 0.024974679293054212, 0.010405619868873598];
const KW_LIVER_Q: [f64; 4] = [0.015791007427613826, 0.26149617619114618, 0.033299572390738948, 0.020811239737747195];
const KW_LUNG_H: [f64; 4] = [0.1025641025641022, 5.7692307692307665, 8.3368421052631572, 8.3076923076923066];
const KW_LUNG_P: [f64; 4] = [0.74877404170654771, 0.016309171877755033, 0.0038849066221194957, 0.0039477518569034564];
const KW_LUNG_Q: [f64; 4] = [0.74877404170654771, 0.021745562503673377, 0.0078955037138069128, 0.0078955037138069128];

fn table() -> DataTable {
    let trt: Vec<String> = (0..24).map(|i| if i < 12 { "ctl" } else { "trt" }.to_owned()).collect();
    let batch: Vec<String> = (0..24).map(|i| ["b1", "b2", "b3"][i % 3].to_owned()).collect();
    let tissue: Vec<String> = (0..24).map(|i| ["liver", "lung"][i % 2].to_owned()).collect();
    DataTable::new(
        "screen",
        vec![
            Column::numeric("y1", Y1.to_vec()),
            Column::numeric("y2", Y2.to_vec()),
            Column::numeric("y3", Y3.to_vec()),
            Column::numeric("y4", Y4.to_vec()),
            Column::numeric("k1", K1.to_vec()),
            Column::numeric("k2", K2.to_vec()),
            Column::numeric("k3", K3.to_vec()),
            Column::numeric("k4", K4.to_vec()),
            Column::text("trt", trt),
            Column::text("batch", batch),
            Column::text("tissue", tissue),
        ],
    )
    .unwrap()
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{a} vs {b}");
}

fn effect(a: &ScreeningAnalysis, subgroup: usize, response: usize, e: usize) -> app_model::screening::EffectResult {
    a.screens()[subgroup].responses[response].effects[e].clone().expect("tested")
}

#[test]
fn linear_screen_matches_lm() {
    let t = table();
    let req = ScreeningRequest::new(vec![0, 1, 2, 3], vec![8, 9]);
    let a = ScreeningAnalysis::run(&t, &RowStateManager::new(24), &req).unwrap();
    for j in 0..4 {
        let trt = effect(&a, 0, j, 0);
        close(trt.statistic, LIN_TRT_F[j], 1e-9);
        close(trt.p_value, LIN_TRT_P[j], 1e-9);
        close(trt.q_value, LIN_TRT_Q[j], 1e-9);
        close(trt.size, LIN_TRT_DIFF[j], 1e-9);
        let d = trt.difference.unwrap();
        assert_eq!(d.label, "trt − ctl");
        close(d.interval.0, LIN_TRT_LOWER[j], 1e-9);
        let batch = effect(&a, 0, j, 1);
        close(batch.p_value, LIN_BATCH_P[j], 1e-9);
        close(batch.size, LIN_BATCH_RANGE[j], 1e-9);
        assert!(batch.difference.is_none());
    }
    let by = ScreeningRequest { fdr: FdrMethod::BenjaminiYekutieli, ..req.clone() };
    let a2 = ScreeningAnalysis::run(&t, &RowStateManager::new(24), &by).unwrap();
    for j in 0..4 {
        close(effect(&a2, 0, j, 1).q_value, LIN_BATCH_Q_BY[j], 1e-9);
    }
    let s = a.sections();
    let kinds: Vec<SectionKind> = s.iter().map(|x| x.kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == SectionKind::ScreeningVolcano).count(), 2);
    assert!(kinds.contains(&SectionKind::ScreeningForest));
    assert_eq!(kinds.iter().filter(|k| **k == SectionKind::ScreeningDifferences).count(), 1, "trt only");
    // Smallest p-value first: y4, y3, y1, y2.
    let SectionContent::Table(results) = &s[1].content else { panic!() };
    let names: Vec<&str> = results.rows.iter().map(|r| r[0].as_str()).collect();
    assert_eq!(names, ["y4", "y3", "y1", "y2"]);
    let ranked = a.ranked_responses();
    assert_eq!(ranked[0].1, "y4");
    assert!(matches!(a.response_report(3), SavedReport::Linear { .. }));
    // Project-file round trip.
    let back = ScreeningRequest::from_json(&req.to_json(&t), &t).unwrap();
    assert_eq!(back, req);
}

#[test]
fn count_screens_match_glm_and_glm_nb() {
    let t = table();
    let req =
        ScreeningRequest { model: ScreeningModel::Poisson, ..ScreeningRequest::new(vec![4, 5, 6, 7], vec![8, 9]) };
    let a = ScreeningAnalysis::run(&t, &RowStateManager::new(24), &req).unwrap();
    for j in 0..4 {
        let e = effect(&a, 0, j, 0);
        close(e.statistic, POI_TRT_LR[j], 1e-7);
        close(e.p_value, POI_TRT_P[j], 1e-6);
        let d = e.difference.unwrap();
        close(d.estimate, POI_TRT_RATIO[j], 1e-7);
        close(d.interval.1, POI_TRT_UPPER[j], 1e-6);
    }
    let nb = ScreeningRequest { model: ScreeningModel::NegativeBinomial, ..req };
    let a = ScreeningAnalysis::run(&t, &RowStateManager::new(24), &nb).unwrap();
    for j in 0..4 {
        let e = effect(&a, 0, j, 0);
        close(e.statistic, NB_TRT_LR[j], 1e-5);
        close(e.p_value, NB_TRT_P[j], 1e-5);
        close(a.screens()[0].responses[j].theta.unwrap(), NB_THETA[j], 1e-5);
    }
    // A negative binomial response opens as a screen of that response alone,
    // which lists its coefficients.
    let SavedReport::Screening { request: one } = a.response_report(6) else { panic!() };
    assert_eq!(one.responses, [6]);
    let single = ScreeningAnalysis::run(&t, &RowStateManager::new(24), &one).unwrap();
    assert!(single.sections().iter().any(|s| s.title.starts_with("Coefficients of k3")));
}

#[test]
fn kruskal_wallis_by_subgroup() {
    let t = table();
    let req = ScreeningRequest {
        model: ScreeningModel::KruskalWallis,
        subgroup: Some(10),
        ..ScreeningRequest::new(vec![0, 1, 2, 3], vec![8])
    };
    let a = ScreeningAnalysis::run(&t, &RowStateManager::new(24), &req).unwrap();
    assert_eq!(a.screens().len(), 2);
    assert_eq!(a.screens()[0].level, "liver");
    for j in 0..4 {
        let liver = effect(&a, 0, j, 0);
        close(liver.statistic, KW_LIVER_H[j], 1e-9);
        close(liver.p_value, KW_LIVER_P[j], 1e-9);
        close(liver.q_value, KW_LIVER_Q[j], 1e-9);
        let lung = effect(&a, 1, j, 0);
        close(lung.statistic, KW_LUNG_H[j], 1e-9);
        close(lung.p_value, KW_LUNG_P[j], 1e-9);
        close(lung.q_value, KW_LUNG_Q[j], 1e-9);
        // ε² = H/(n − 1) with 12 rows per tissue.
        close(lung.size, KW_LUNG_H[j] / 11.0, 1e-12);
    }
    let s = a.sections();
    assert_eq!(s.iter().filter(|x| x.kind == SectionKind::ScreeningVolcano).count(), 2, "one per tissue");
    assert!(matches!(a.response_report(0), SavedReport::Groups { .. }));
    let two = ScreeningRequest { factors: vec![8, 9], ..req };
    assert!(ScreeningAnalysis::run(&t, &RowStateManager::new(24), &two).is_err());
}

#[test]
fn errors_and_failed_fits() {
    let t = table();
    let rows = RowStateManager::new(24);
    assert!(ScreeningAnalysis::run(&t, &rows, &ScreeningRequest::new(vec![], vec![8])).is_err());
    assert!(ScreeningAnalysis::run(&t, &rows, &ScreeningRequest::new(vec![0], vec![])).is_err());
    assert!(ScreeningAnalysis::run(&t, &rows, &ScreeningRequest::new(vec![8], vec![9])).is_err(), "text response");
    // Negative counts cannot be fitted by Poisson regression: that response
    // reports why, the others are still screened.
    let mut t2 = t.clone();
    t2.insert_column(11, Column::numeric("neg", (0..24).map(|i| f64::from(i) - 5.0).collect())).unwrap();
    let req = ScreeningRequest { model: ScreeningModel::Poisson, ..ScreeningRequest::new(vec![4, 11], vec![8]) };
    let a = ScreeningAnalysis::run(&t2, &rows, &req).unwrap();
    assert!(a.screens()[0].responses[1].error.is_some());
    assert!(a.screens()[0].responses[0].effects[0].is_some());
    let s = a.sections();
    let SectionContent::Table(results) = &s[1].content else { panic!() };
    assert!(results.rows.iter().any(|r| r[2].starts_with("not available")));
}
