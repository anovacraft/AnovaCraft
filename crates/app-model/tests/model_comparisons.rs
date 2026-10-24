//! Spline terms, slices of interactions and slope comparisons through the
//! linear model, GLM and mixed model reports, against R 4.3.3 (glm, splines,
//! emmeans 1.10.0, lmerTest 3.1-3; slice F tests of the mixed model from
//! lmerTest `contest`); the script is
//! `tests/data/model_comparisons_reference.R`. Report tables round to six
//! significant digits, hence the tolerances.
#![allow(clippy::excessive_precision)]

use app_model::report::{SectionContent, SectionKind, TextTable};
use app_model::{
    GlmAnalysis, GlmRequest, LinearModelAnalysis, LinearModelSpec, MixedAnalysis, MixedFamily, MixedRequest,
    RandomEffectSpec, ReportOptions,
};
use stat_data::{Column, DataTable, RowStateManager};
use stat_discrete::Family;
use stat_linear::SplineSpec;
use stat_linear::marginal::PairAdjust;

const GLM_PAIRS_EST: [f64; 6] = [
    -0.53899650073268768,
    -0.90190199442205587,
    -0.3629054936893682,
    -0.28768207245178096,
    0.32542240043462778,
    0.61310447288640879,
];
const GLM_PAIRS_LOWER: [f64; 6] = [
    -1.6536484569457914,
    -1.9135974866840562,
    -1.1975116925833302,
    -0.97820147975614491,
    -0.52762869908128973,
    -0.15773105008432808,
];
const GLM_PAIRS_P: [f64; 6] = [
    0.49350060638282489,
    0.092015349931919399,
    0.56476636124196089,
    0.59179557748732736,
    0.64404936938496582,
    0.14921053010674212,
];
const GLM_SLICE_P: [f64; 2] = [0.10204759522648126, 0.16266475650418205];
const GLM_SPLINE_COEF: [f64; 5] =
    [0.59000719543869229, 0.76520744474760327, 1.1301994487640012, 1.8672374987330023, 2.111762819027327];
const GLM_SPLINE_DEV: [f64; 1] = [2.1072994940833052];
const GLM_SPLINE_LR: [f64; 1] = [25.499305060395244];
const MIXED_PAIRS_DF: [f64; 6] = [
    13.673733866731567,
    13.214096273340116,
    11.411046506104267,
    11.415679794575178,
    12.850084889855257,
    11.954231556539829,
];
const MIXED_PAIRS_EST: [f64; 6] = [
    -0.63936515620941292,
    -3.2475111251037942,
    -2.6081459688943811,
    -2.1769256729439705,
    -0.55237849629435276,
    1.6245471766496176,
];
const MIXED_PAIRS_P: [f64; 6] = [
    0.44069077973432247,
    0.0016590157663447739,
    0.0041987610617478634,
    0.020025215333392046,
    0.48065351481272722,
    0.065131984255684561,
];
const MIXED_PAIRS_SE: [f64; 6] = [
    0.80512461865251983,
    0.7184450716653652,
    0.65937875765333454,
    0.65842509926290271,
    0.76049783280105343,
    0.67213454829915531,
];
const MIXED_SLICE_F: [f64; 2] = [13.727691055540038, 6.3830393348207632];
const MIXED_SLICE_DF2: [f64; 2] = [12.324933604006352, 11.969038598365692];
const MIXED_SLICE_P: [f64; 2] = [0.00073117524552206961, 0.012982314530431454];
const MIXED_Y: [f64; 20] = [
    11.299999999999999,
    11.5,
    10.300000000000001,
    12.800000000000001,
    11.5,
    16.300000000000001,
    10.1,
    13.5,
    15.1,
    15.100000000000001,
    13.299999999999999,
    13.199999999999999,
    14.0,
    12.1,
    14.4,
    14.0,
    11.1,
    13.1,
    9.3000000000000007,
    15.9,
];

const A: [&str; 20] = [
    "a1", "a1", "a1", "a1", "a2", "a2", "a2", "a2", "a2", "a3", "a3", "a3", "a3", "a3", "a3", "a1", "a2", "a3", "a1",
    "a2",
];
const B: [&str; 20] = [
    "b1", "b2", "b1", "b2", "b1", "b2", "b1", "b2", "b2", "b1", "b2", "b1", "b2", "b1", "b1", "b2", "b1", "b2", "b1",
    "b2",
];
const CNT: [f64; 20] =
    [3.0, 5.0, 2.0, 7.0, 4.0, 9.0, 3.0, 6.0, 8.0, 7.0, 4.0, 6.0, 5.0, 4.0, 6.0, 6.0, 5.0, 4.0, 2.0, 9.0];

fn text(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn number(cell: &str) -> f64 {
    let t = cell.trim().replace('−', "-");
    if let Some(rest) = t.strip_prefix("< ") {
        return rest.parse::<f64>().unwrap() / 2.0;
    }
    t.parse().unwrap_or_else(|_| panic!("not a number: {cell:?}"))
}

fn evaluate(t: &DataTable, formula: &str) -> Vec<f64> {
    app_model::formula::evaluate(t, None, formula, 0..t.n_rows())
        .unwrap()
        .values
        .into_iter()
        .map(|v| match v {
            app_model::formula::FormulaValue::Number(x) => x,
            other => panic!("{other:?}"),
        })
        .collect()
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{a} vs {b}");
}

fn table(sections: &[app_model::report::ReportSection], kind: SectionKind) -> TextTable {
    match &sections.iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("no {kind:?}")).content {
        SectionContent::Table(t) => t.clone(),
        other => panic!("{kind:?} is not a table: {other:?}"),
    }
}

fn column(t: &TextTable, heading: &str) -> Vec<f64> {
    let j = t.columns.iter().position(|c| c == heading).unwrap_or_else(|| panic!("no column {heading}"));
    t.rows.iter().map(|r| number(&r[j])).collect()
}

fn factors() -> DataTable {
    DataTable::new(
        "t",
        vec![Column::text("A", text(&A)), Column::text("B", text(&B)), Column::numeric("Count", CNT.to_vec())],
    )
    .unwrap()
}

#[test]
fn glm_slices_match_emmeans() {
    let t = factors();
    let request = GlmRequest {
        response: 2,
        predictors: vec![0, 1],
        family: Family::Poisson,
        link: None,
        event: None,
        effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
        splines: Vec::new(),
        slices: Some(PairAdjust::Tukey),
        slopes: None,
    };
    let a = GlmAnalysis::run(&t, &RowStateManager::new(20), &request).unwrap();
    let s = a.sections();
    let tests = table(&s, SectionKind::SliceTests);
    // The first two rows test A within B = b1 and B = b2.
    // p-values are shown to four decimals.
    for (p, r) in column(&tests, "p-value").iter().zip(GLM_SLICE_P) {
        assert!((p - r).abs() <= 5e-5, "{p} vs {r}");
    }
    let pairs = table(&s, SectionKind::SlicePairs);
    let (est, p, lower) = (column(&pairs, "Difference"), column(&pairs, "p-value"), column(&pairs, "Lower 95%"));
    for i in 0..6 {
        close(est[i], GLM_PAIRS_EST[i], 1e-5);
        assert!((p[i] - GLM_PAIRS_P[i]).abs() <= 5e-5, "{} vs {}", p[i], GLM_PAIRS_P[i]);
        close(lower[i], GLM_PAIRS_LOWER[i], 1e-5);
    }
}

#[test]
fn glm_spline_matches_glm_with_bs() {
    let x = vec![1.2, 2.5, 3.1, 4.8, 5.0, 6.7, 7.3, 8.9, 9.4, 10.6, 11.2, 12.8];
    let k = vec![2.0, 3.0, 2.0, 5.0, 6.0, 4.0, 7.0, 9.0, 8.0, 12.0, 11.0, 15.0];
    let t = DataTable::new("s", vec![Column::numeric("x", x), Column::numeric("k", k)]).unwrap();
    let request = GlmRequest {
        response: 1,
        predictors: vec![0],
        family: Family::Poisson,
        link: None,
        event: None,
        effects: Vec::new(),
        splines: vec![Some(SplineSpec::bspline(3, 1))],
        slices: None,
        slopes: None,
    };
    let a = GlmAnalysis::run(&t, &RowStateManager::new(12), &request).unwrap();
    for (c, r) in a.fit().coefficients.iter().zip(GLM_SPLINE_COEF) {
        close(c.estimate, r, 1e-7);
    }
    close(a.fit().deviance, GLM_SPLINE_DEV[0], 1e-8);
    let lr = table(&a.sections(), SectionKind::LikelihoodRatioTests);
    close(column(&lr, "χ²")[0], GLM_SPLINE_LR[0], 1e-5);
    assert!(a.title().contains("x (spline)"));
    // The saved formula reproduces the fitted means.
    let formula = a.prediction_formula().unwrap();
    assert!(formula.contains("bspline("), "{formula}");
    for (v, m) in evaluate(&t, &formula).iter().zip(&a.fit().fitted) {
        close(*v, *m, 1e-10);
    }
}

#[test]
fn linear_spline_report_and_formula() {
    let x = vec![1.2, 2.5, 3.1, 4.8, 5.0, 6.7, 7.3, 8.9, 9.4, 10.6, 11.2, 12.8];
    let y = vec![3.1, 4.9, 5.2, 7.9, 8.4, 9.1, 8.8, 9.9, 11.3, 10.2, 9.8, 12.5];
    let t = DataTable::new("s", vec![Column::numeric("x", x), Column::numeric("y", y)]).unwrap();
    let spec = LinearModelSpec {
        response: 1,
        predictors: vec![0],
        effects: Vec::new(),
        splines: vec![Some(SplineSpec::natural(2))],
    };
    let a = LinearModelAnalysis::run(&t, &RowStateManager::new(12), &spec).unwrap();
    let s = a.sections(&ReportOptions::default());
    let knots = table(&s, SectionKind::SplineKnots);
    assert_eq!(knots.rows[0][2], "3");
    let effects = table(&s, SectionKind::EffectTests);
    assert_eq!(effects.rows[0][1], "3");
    let formula = a.prediction_formula().unwrap();
    for (v, f) in evaluate(&t, &formula).iter().zip(a.fit().fitted_values()) {
        close(*v, *f, 1e-10);
    }
}

#[test]
fn mixed_slices_match_lmertest_satterthwaite() {
    let s_labels: Vec<String> = (0..20).map(|i| ["s1", "s2", "s3", "s4", "s5"][i % 5].to_owned()).collect();
    let t = DataTable::new(
        "m",
        vec![
            Column::text("A", text(&A)),
            Column::text("B", text(&B)),
            Column::text("S", s_labels),
            Column::numeric("y", MIXED_Y.to_vec()),
        ],
    )
    .unwrap();
    let request = MixedRequest {
        response: 3,
        predictors: vec![0, 1],
        effects: vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]],
        random: vec![RandomEffectSpec {
            group: 2,
            within: Vec::new(),
            intercept: true,
            slopes: Vec::new(),
            correlated: false,
        }],
        family: MixedFamily::Normal,
        trials: None,
        offset: None,
        reml: true,
        quadrature_points: 1,
        slices: Some(PairAdjust::Holm),
        slopes: None,
        frequency: None,
        weight: None,
        robust: false,
        residual_groups: None,
        df_method: app_model::DfMethod::Satterthwaite,
        order: Vec::new(),
    };
    let a = MixedAnalysis::run(&t, &RowStateManager::new(20), &request).unwrap();
    let s = a.sections();
    let tests = table(&s, SectionKind::SliceTests);
    let (f, df2, p) = (column(&tests, "F"), column(&tests, "Den DF"), column(&tests, "p-value"));
    for j in 0..2 {
        close(f[j], MIXED_SLICE_F[j], 1e-5);
        close(df2[j], MIXED_SLICE_DF2[j], 1e-3);
        close(p[j], MIXED_SLICE_P[j], 1e-3);
    }
    let pairs = table(&s, SectionKind::SlicePairs);
    let (est, se, df, pp) =
        (column(&pairs, "Difference"), column(&pairs, "Std Error"), column(&pairs, "DF"), column(&pairs, "p-value"));
    for i in 0..6 {
        close(est[i], MIXED_PAIRS_EST[i], 1e-5);
        close(se[i], MIXED_PAIRS_SE[i], 1e-4);
        close(df[i], MIXED_PAIRS_DF[i], 1e-3);
        close(pp[i], MIXED_PAIRS_P[i], 1e-3);
    }
}
