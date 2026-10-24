//! MANOVA against published values and statsmodels.
//!
//! Reference values come from `tests/data/manova_reference.py` (statsmodels
//! 0.15 `MANOVA.mv_test`, Sum coding, and an independent NumPy Box's M).

use stat_core::Matrix;
use stat_linear::manova::{MultivariateStatistic as S, box_m_test, fit_manova};
use stat_linear::{ModelDesign, Predictor, PredictorValues};

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-300)
}

fn chars(s: &str) -> Vec<String> {
    s.chars().map(|c| c.to_string()).collect()
}

/// Fit `responses ~ effects` through the same design coding the app uses.
fn fit(
    predictors: &[Predictor<'_>],
    effects: &[Vec<(usize, u8)>],
    y: &[Vec<f64>],
) -> (stat_linear::manova::ManovaFit, ModelDesign) {
    let rows: Vec<usize> = (0..y[0].len()).collect();
    let design = ModelDesign::with_effects(predictors, &rows, effects).unwrap();
    let x: Matrix = design.matrix(predictors, &rows).unwrap();
    let named: Vec<(String, std::ops::Range<usize>)> =
        design.effects().iter().map(|e| (e.name.clone(), e.columns.clone())).collect();
    (fit_manova(&x, y, &named).unwrap(), design)
}

const TEAR: [f64; 20] =
    [6.5, 6.2, 5.8, 6.5, 6.5, 6.9, 7.2, 6.9, 6.1, 6.3, 6.7, 6.6, 7.2, 7.1, 6.8, 7.1, 7.0, 7.2, 7.5, 7.6];
const GLOSS: [f64; 20] =
    [9.5, 9.9, 9.6, 9.6, 9.2, 9.1, 10.0, 9.9, 9.5, 9.4, 9.1, 9.3, 8.3, 8.4, 8.5, 9.2, 8.8, 9.7, 10.1, 9.2];
const OPACITY: [f64; 20] =
    [4.4, 6.4, 3.0, 4.1, 0.8, 5.7, 2.0, 3.9, 1.9, 5.7, 2.8, 4.1, 3.8, 1.6, 3.4, 8.4, 5.2, 6.9, 2.7, 1.9];

/// Johnson & Wichern, *Applied Multivariate Statistical Analysis*, 6th ed.,
/// Example 6.13: Wilks' Λ = 0.3819 (rate), 0.5230 (additive), 0.7771
/// (interaction); R's `summary.manova` gives Pillai 0.6181, F = 7.5543 on
/// 3 and 14 df, p = 0.003034 for rate. Full precision from statsmodels.
#[test]
fn plastic_film_two_way_manova() {
    let rate: Vec<String> = (0..20).map(|i| if i < 10 { "Low" } else { "High" }.to_owned()).collect();
    let additive: Vec<String> = (0..20).map(|i| if (i / 5) % 2 == 0 { "Low" } else { "High" }.to_owned()).collect();
    let predictors = [
        Predictor { name: "rate", values: PredictorValues::Categorical(&rate) },
        Predictor { name: "additive", values: PredictorValues::Categorical(&additive) },
    ];
    let effects = vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]];
    let y = vec![TEAR.to_vec(), GLOSS.to_vec(), OPACITY.to_vec()];
    let (m, _) = fit(&predictors, &effects, &y);
    assert_eq!(m.df_error(), 16);
    // (Wilks, Pillai, HL, F, p) per effect.
    let expected = [
        (0.381858384661, 0.618141615339, 1.61877188028, 7.55426877464, 0.00303404516026),
        (0.523034895419, 0.476965104581, 0.91191832277, 4.2556188396, 0.0247452809991),
        (0.777105757874, 0.222894242126, 0.286826136428, 1.33852197, 0.3017816451),
    ];
    for (t, (w, v, u, f, p)) in m.effect_tests().iter().zip(expected) {
        assert_eq!(t.df, 1);
        assert!(close(t.test(S::Wilks).value, w, 1e-10), "{} Wilks {}", t.name, t.test(S::Wilks).value);
        assert!(close(t.test(S::Pillai).value, v, 1e-10));
        assert!(close(t.test(S::HotellingLawley).value, u, 1e-10));
        assert!(close(t.test(S::Roy).value, u, 1e-10));
        for s in S::ALL {
            let r = t.test(s);
            assert!(r.exact);
            assert!(close(r.f.unwrap(), f, 1e-9), "{} {s:?} F {:?}", t.name, r.f);
            assert!(close(r.p_value.unwrap(), p, 1e-8));
            assert!((r.df1 - 3.0).abs() < 1e-12 && (r.df2 - 14.0).abs() < 1e-12);
        }
        // One nonzero eigenvalue; canonical correlation² = Pillai's V.
        assert_eq!(t.eigenvalues.len(), 1);
        assert!(close(t.canonical_correlations()[0].powi(2), v, 1e-10));
    }
    // Error SSCP (NumPy least squares).
    let e = [[1.764, 0.02, -3.07], [0.02, 2.628, -0.552], [-3.07, -0.552, 64.924]];
    for i in 0..3 {
        for j in 0..3 {
            assert!((m.error_sscp()[i][j] - e[i][j]).abs() < 1e-10);
        }
    }
    // Canonical variate: unit error variance, aᵀ(E/ν)a = 1.
    let a = &m.effect_tests()[0].canonical[0];
    let q: f64 =
        (0..3).flat_map(|i| (0..3).map(move |j| (i, j))).map(|(i, j)| a[i] * e[i][j] * a[j]).sum::<f64>() / 16.0;
    assert!(close(q, 1.0, 1e-10));
    // Univariate tests: the diagonal of H against E (one-response ANOVA F).
    let uni = &m.effect_tests()[0].univariate[0];
    let f_tear = uni.sum_of_squares / (1.764 / 16.0);
    assert!(close(uni.f.unwrap(), f_tear, 1e-12));
    assert!(close(uni.sum_of_squares, 1.7405, 1e-10), "tear SS for rate {}", uni.sum_of_squares);

    // Box's M across the four cells.
    let cells: Vec<Vec<Vec<f64>>> =
        (0..4).map(|c| (5 * c..5 * c + 5).map(|i| vec![TEAR[i], GLOSS[i], OPACITY[i]]).collect()).collect();
    let b = box_m_test(&cells).unwrap();
    assert!(close(b.m, 36.3054467729, 1e-9));
    assert!(close(b.chi_square, 24.0145403133, 1e-9));
    assert!((b.df - 18.0).abs() < 1e-12);
    assert!(close(b.p_value, 0.154551994196, 1e-8));
}

/// An unbalanced layout with a three-level factor (s = 2), so the four
/// statistics and their approximations differ, plus a covariate and a
/// contrast.
#[test]
fn unbalanced_layout_with_covariate_and_contrast() {
    let g = chars("cbbbcbcbaccabaabacacaacccacaaba");
    let f = chars("vvvuuuuuuvuuvuvuuuvvvvvuvuuuvvv");
    let x = vec![
        11.86, 10.63, 10.4, 7.38, 9.05, 9.43, 7.62, 10.65, 11.29, 9.66, 11.77, 7.58, 12.35, 10.78, 7.52, 6.19, 7.19,
        10.1, 14.11, 12.31, 10.66, 13.12, 9.47, 9.91, 9.48, 10.44, 10.04, 10.28, 10.99, 11.85, 14.22,
    ];
    let y = vec![
        vec![
            0.53, 0.4, 1.04, 0.46, 2.31, 1.19, -0.78, 1.35, 1.9, 1.06, 0.81, -1.32, 1.28, 0.1, 0.53, 0.95, 1.25, 0.09,
            3.35, 2.04, 1.19, 3.11, 1.05, 0.05, 0.59, 0.54, -1.68, 0.99, -1.72, 1.87, 0.77,
        ],
        vec![
            0.14, 0.27, 1.28, -0.06, 1.74, -1.18, 0.41, -0.51, 1.37, 1.36, 0.04, -1.6, 0.19, 0.39, -0.93, -1.4, 0.02,
            2.3, 2.35, 2.25, 1.76, -0.11, -0.17, 0.18, -0.52, 0.33, 1.71, 1.7, -0.28, 1.35, 0.83,
        ],
        vec![
            -1.12, 0.15, 0.79, -0.24, -0.9, 0.83, -0.68, 1.02, 0.5, 0.98, -0.89, -0.04, 0.75, 0.81, 0.7, 2.39, -0.3,
            1.29, 0.64, -0.05, 2.0, 0.76, -1.2, -0.49, 2.29, -1.47, 0.16, -2.53, 0.71, 1.12, -1.31,
        ],
    ];
    let predictors = [
        Predictor { name: "g", values: PredictorValues::Categorical(&g) },
        Predictor { name: "f", values: PredictorValues::Categorical(&f) },
        Predictor { name: "x", values: PredictorValues::Continuous(&x) },
    ];
    let effects = vec![vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)], vec![(2, 1)]];
    let (m, design) = fit(&predictors, &effects, &y);
    assert_eq!(m.df_error(), 24);
    let by_name = |name: &str| m.effect_tests().iter().find(|t| t.name == name).unwrap();
    // (value, F, df1, df2, p) per statistic, from statsmodels.
    type Row = (f64, f64, f64, f64, f64);
    let check = |name: &str, rows: [Row; 4]| {
        let t = by_name(name);
        for (s, (value, f, df1, df2, p)) in S::ALL.into_iter().zip(rows) {
            let r = t.test(s);
            assert!(close(r.value, value, 1e-9), "{name} {s:?} value {} vs {value}", r.value);
            assert!(close(r.f.unwrap(), f, 1e-8), "{name} {s:?} F {:?} vs {f}", r.f);
            assert!(close(r.df1, df1, 1e-12) && close(r.df2, df2, 1e-10), "{name} {s:?} df {} {}", r.df1, r.df2);
            assert!(close(r.p_value.unwrap(), p, 1e-7), "{name} {s:?} p {:?} vs {p}", r.p_value);
        }
    };
    check(
        "g",
        [
            (0.709118161734, 1.37514191552, 6.0, 44.0, 0.245825340835),
            (0.29567234659, 1.33003845788, 6.0, 46.0, 0.263277487932),
            (0.403446626219, 1.44978381129, 6.0, 27.625, 0.231940887937),
            (0.38594250217, 2.95889251663, 3.0, 23.0, 0.0535240343405),
        ],
    );
    check(
        "g × f",
        [
            (0.731151651834, 1.24292182114, 6.0, 44.0, 0.303325673432),
            (0.281960172509, 1.25823314531, 6.0, 46.0, 0.29519194767),
            (0.349772202772, 1.25690498882, 6.0, 27.625, 0.308989250958),
            (0.287367373689, 2.20314986495, 3.0, 23.0, 0.115006288569),
        ],
    );
    let exact = |value: f64, f: f64, p: f64| (value, f, 3.0, 22.0, p);
    check(
        "f",
        [
            exact(0.91558153186, 0.6761481581, 0.575887019251),
            exact(0.0844184681397, 0.6761481581, 0.575887019251),
            exact(0.0922020215591, 0.6761481581, 0.575887019251),
            exact(0.0922020215591, 0.6761481581, 0.575887019251),
        ],
    );
    check(
        "x",
        [
            exact(0.716096814737, 2.90736761996, 0.0574064687101),
            exact(0.283903185263, 2.90736761996, 0.0574064687101),
            exact(0.396459220904, 2.90736761996, 0.0574064687101),
            exact(0.396459220904, 2.90736761996, 0.0574064687101),
        ],
    );
    assert!(by_name("g").test(S::Wilks).exact, "min(p, h) = 2: Rao's F is exact");
    assert!(!by_name("g").test(S::Pillai).exact);
    // Contrast: least-squares mean of a minus b. Sum-to-zero coding with
    // the first level (a) at −1: a − b = −2β_b − β_c.
    let g_cols = design.effects()[0].columns.clone();
    let mut l = vec![0.0; 1 + design.column_names().len()];
    l[1 + g_cols.start] = -2.0;
    l[1 + g_cols.start + 1] = -1.0;
    let c = m.test_hypothesis("a − b", &[l]).unwrap();
    assert_eq!(c.df, 1);
    assert!(close(c.test(S::Wilks).value, 0.87243722208, 1e-9));
    assert!(close(c.test(S::Pillai).f.unwrap(), 1.07223803356, 1e-8));
    assert!(close(c.test(S::Roy).p_value.unwrap(), 0.381196764565, 1e-7));
    // The effect's own columns as a hypothesis reproduce its test.
    let rows: Vec<Vec<f64>> = g_cols
        .clone()
        .map(|j| {
            let mut r = vec![0.0; 1 + design.column_names().len()];
            r[1 + j] = 1.0;
            r
        })
        .collect();
    let again = m.test_hypothesis("g", &rows).unwrap();
    assert!(close(again.test(S::Wilks).value, 0.709118161734, 1e-9));
    assert!(m.test_hypothesis("bad", &[vec![1.0]]).is_err());
    // Residual correlations are a correlation matrix.
    let r = m.residual_correlations();
    assert!((r[0][0] - 1.0).abs() < 1e-12 && r[0][1].abs() < 1.0);
}

#[test]
fn refuses_degenerate_inputs() {
    let g = chars("aabb");
    let predictors = [Predictor { name: "g", values: PredictorValues::Categorical(&g) }];
    let y = vec![vec![1.0, 2.0, 3.0, 4.0], vec![2.0, 1.0, 5.0, 3.0], vec![0.5, 0.7, 0.1, 0.3]];
    let rows: Vec<usize> = (0..4).collect();
    let design = ModelDesign::new(&predictors, &rows).unwrap();
    let x = design.matrix(&predictors, &rows).unwrap();
    let effects = vec![("g".to_owned(), 0..1)];
    // n = 4 < k + p = 2 + 3.
    assert!(fit_manova(&x, &y, &effects).is_err());
    // A response that is a linear combination of others.
    let y2 = [y[0].clone(), y[1].clone()];
    let g8 = chars("aabbaabb");
    let p8 = [Predictor { name: "g", values: PredictorValues::Categorical(&g8) }];
    let rows8: Vec<usize> = (0..8).collect();
    let d8 = ModelDesign::new(&p8, &rows8).unwrap();
    let x8 = d8.matrix(&p8, &rows8).unwrap();
    let extend = |v: &Vec<f64>| {
        v.iter().chain(v.iter().map(|a| a * 1.1).collect::<Vec<_>>().iter()).copied().collect::<Vec<f64>>()
    };
    let mut ys: Vec<Vec<f64>> = y2.iter().map(extend).collect();
    ys.push(ys[0].iter().zip(&ys[1]).map(|(a, b)| a + 2.0 * b).collect());
    assert!(fit_manova(&x8, &ys, &effects).is_err());
    assert!(fit_manova(&x8, &[], &effects).is_err());
    assert!(box_m_test(&[vec![vec![1.0, 2.0]]]).is_err());
}
