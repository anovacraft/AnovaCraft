//! Least-squares means, slices of an interaction and slope comparisons
//! against R 4.3.3 with emmeans 1.10.0 (`joint_tests`, `emmeans`, `pairs`,
//! `emtrends`); the script is `tests/data/marginal_reference.R`. emmeans
//! prints joint-test F ratios to three decimals, hence their tolerance.
#![allow(clippy::excessive_precision)]

use stat_linear::marginal::{PairAdjust, WaldInference, group_slopes, slices};
use stat_linear::{ModelDesign, OlsOptions, Predictor, PredictorValues, effect_tests, fit_linear_model};

const EQUAL_SLOPES_F: [f64; 1] = [0.94071360488995304];
const EQUAL_SLOPES_P: [f64; 1] = [0.4136847875622725];
const LSMEANS: [f64; 6] = [
    11.028319034000628,
    11.470062493379936,
    14.116752462662852,
    12.453083359813578,
    14.500046075627578,
    12.726137061751931,
];
const LSMEANS_LOWER: [f64; 6] = [
    10.640331199443597,
    11.126350250284617,
    13.819450778552888,
    12.094037808629183,
    14.189026719688437,
    12.383468530141091,
];
const LSMEANS_SE: [f64; 6] = [
    0.17959334551367279,
    0.15909888438118971,
    0.13761618102566894,
    0.16619642675813767,
    0.14396587129179098,
    0.1586157670755588,
];
const PAIRS_BONFERRONI_LOWER: [f64; 6] = [
    -1.0885047523858264,
    -3.7011497482159719,
    -3.2229714595589933,
    -2.6234141575705641,
    -0.9035167209085474,
    1.1860673901671515,
];
const PAIRS_BONFERRONI_P: [f64; 6] = [
    0.25007859437677726,
    1.1121013530573317e-08,
    3.4469525380724238e-08,
    7.2285792236120538e-07,
    0.76681133293329351,
    4.5553265497714868e-06,
];
const PAIRS_HOLM_LOWER: [f64; 6] = [
    -1.0885047523858264,
    -3.7011497482159719,
    -3.2229714595589933,
    -2.6234141575705641,
    -0.9035167209085474,
    1.1860673901671515,
];
const PAIRS_HOLM_P: [f64; 6] = [
    0.083359531458925759,
    1.1121013530573317e-08,
    2.2979683587149491e-08,
    7.2285792236120538e-07,
    0.25560377764443115,
    3.0368843665143247e-06,
];
const PAIRS_NONE_LOWER: [f64; 6] = [
    -0.95058322839290899,
    -3.5704882978756949,
    -3.1000797184371489,
    -2.5004861743800517,
    -0.76907079574606518,
    1.3114243239927039,
];
const PAIRS_NONE_P: [f64; 6] = [
    0.083359531458925759,
    3.7070045101911053e-09,
    1.1489841793574745e-08,
    2.4095264078706846e-07,
    0.25560377764443115,
    1.5184421832571623e-06,
];
const PAIRS_TUKEY_EST: [f64; 6] = [
    -0.4417434593793077,
    -3.0884334286622228,
    -2.6466899692829151,
    -2.046962715814002,
    -0.2730537019383541,
    1.7739090138756479,
];
const PAIRS_TUKEY_LOWER: [f64; 6] = [
    -1.0636555621977193,
    -3.6776085986132001,
    -3.2008301710981644,
    -2.601266339410591,
    -0.87929372606584166,
    1.2086528300596884,
];
const PAIRS_TUKEY_P: [f64; 6] = [
    0.18494810089834568,
    1.0407324668726403e-08,
    3.2533576388438235e-08,
    6.7941794612558226e-07,
    0.47982461988006819,
    4.2605811789719894e-06,
];
const PAIRS_TUKEY_SE: [f64; 6] = [
    0.23553376757776762,
    0.22313546707474852,
    0.20986684277144471,
    0.20992873471355172,
    0.22959835689333144,
    0.21407674495712137,
];
const SLICE_A_F: [f64; 3] = [29.748999999999999, 194.88800000000001, 43.843000000000004];
const SLICE_A_P: [f64; 3] = [0.00011040938262898307, 3.3378552264414065e-09, 1.6596618117922696e-05];
const SLICE_B_F: [f64; 2] = [124.745, 58.447000000000003];
const SLICE_B_DF2: [f64; 1] = [13.0];
const SLICE_B_P: [f64; 2] = [3.2839592525431747e-09, 3.1789625672002346e-07];
const SLOPE_PAIRS_EST: [f64; 3] = [0.19636760431597147, 1.1643731782393143, 0.96800557392334285];
const SLOPE_PAIRS_LOWER: [f64; 3] = [-1.8703834158343349, -1.2045425172233704, -1.2404462086544199];
const SLOPE_PAIRS_P: [f64; 3] = [0.96656575931748945, 0.42533479142225028, 0.50230989170101603];
const SLOPES: [f64; 3] = [1.9593884376492672, 1.7630208333332957, 0.79501525940995288];
const SLOPES_LOWER: [f64; 3] = [0.66291800560856085, 0.6732535674263509, -0.649855056172016];
const SLOPES_P: [f64; 3] = [0.0059121810039513391, 0.0037537180467300184, 0.25761169767073];
const SLOPES_SE: [f64; 3] = [0.60447523259263714, 0.50810053607937566, 0.67366620826273127];

const A: [&str; 20] = [
    "a1", "a1", "a1", "a1", "a2", "a2", "a2", "a2", "a2", "a3", "a3", "a3", "a3", "a3", "a3", "a1", "a2", "a3", "a1",
    "a2",
];
const B: [&str; 20] = [
    "b1", "b2", "b1", "b2", "b1", "b2", "b1", "b2", "b2", "b1", "b2", "b1", "b2", "b1", "b1", "b2", "b1", "b2", "b1",
    "b2",
];
const Z: [f64; 20] =
    [3.2, 4.1, 2.8, 5.0, 3.9, 4.4, 3.1, 2.7, 5.2, 4.8, 3.3, 3.9, 4.6, 2.9, 3.5, 4.0, 4.2, 3.7, 3.0, 4.9];
const Y: [f64; 20] = [
    10.1, 12.3, 9.8, 13.9, 11.2, 15.1, 10.9, 13.0, 16.2, 14.8, 12.1, 14.0, 13.5, 13.2, 14.1, 12.8, 11.9, 12.6, 10.4,
    15.6,
];

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{a} vs {b}");
}

fn labels(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

struct Fitted {
    design: ModelDesign,
    beta: Vec<f64>,
    cov: stat_core::Matrix,
    df: f64,
}

fn fit(predictors: &[Predictor<'_>], effects: &[Vec<(usize, u8)>]) -> Fitted {
    let rows: Vec<usize> = (0..20).collect();
    let design = ModelDesign::with_effects(predictors, &rows, effects).unwrap();
    let x = design.matrix(predictors, &rows).unwrap();
    let f = fit_linear_model(&x, &Y, &OlsOptions::default()).unwrap();
    Fitted { beta: f.coefficients(), cov: f.covariance().clone(), df: f.anova().error.df as f64, design }
}

#[test]
fn slices_and_pairs_match_emmeans() {
    let (a, b) = (labels(&A), labels(&B));
    let preds = [
        Predictor { name: "A", values: PredictorValues::Categorical(&a) },
        Predictor { name: "B", values: PredictorValues::Categorical(&b) },
        Predictor { name: "z", values: PredictorValues::Continuous(&Z) },
    ];
    let m = fit(&preds, &[vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)], vec![(2, 1)]]);
    let inf = WaldInference { beta: &m.beta, covariance: &m.cov, df: Some(m.df) };
    assert_eq!(m.df, SLICE_B_DF2[0]);
    let expected = [
        (PairAdjust::Tukey, PAIRS_TUKEY_P, PAIRS_TUKEY_LOWER),
        (PairAdjust::Holm, PAIRS_HOLM_P, PAIRS_HOLM_LOWER),
        (PairAdjust::Bonferroni, PAIRS_BONFERRONI_P, PAIRS_BONFERRONI_LOWER),
        (PairAdjust::None, PAIRS_NONE_P, PAIRS_NONE_LOWER),
    ];
    for (adjust, p, lower) in expected {
        let s = slices(&m.design, &inf, 0, 1, Some(adjust), 0.95).unwrap();
        assert_eq!(s.len(), 2);
        for (j, slice) in s.iter().enumerate() {
            assert!((slice.test.f - SLICE_B_F[j]).abs() <= 5e-4);
            close(slice.test.p_value, SLICE_B_P[j], 1e-9);
            for (i, mean) in slice.means.iter().enumerate() {
                close(mean.estimate, LSMEANS[3 * j + i], 1e-10);
                close(mean.std_error, LSMEANS_SE[3 * j + i], 1e-10);
                close(mean.interval.0, LSMEANS_LOWER[3 * j + i], 1e-10);
            }
            for (i, pair) in slice.pairs.iter().enumerate() {
                close(pair.estimate, PAIRS_TUKEY_EST[3 * j + i], 1e-10);
                close(pair.std_error, PAIRS_TUKEY_SE[3 * j + i], 1e-10);
                close(pair.p_value, p[3 * j + i], 1e-6);
                close(pair.interval.0, lower[3 * j + i], 1e-7);
            }
        }
    }
    let s = slices(&m.design, &inf, 1, 0, None, 0.95).unwrap();
    for (j, slice) in s.iter().enumerate() {
        assert!((slice.test.f - SLICE_A_F[j]).abs() <= 5e-4);
        close(slice.test.p_value, SLICE_A_P[j], 1e-9);
        assert!(slice.pairs.is_empty());
    }
}

#[test]
fn slopes_match_emtrends() {
    let a = labels(&A);
    let preds = [
        Predictor { name: "A", values: PredictorValues::Categorical(&a) },
        Predictor { name: "z", values: PredictorValues::Continuous(&Z) },
    ];
    let effects = [vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]];
    let m = fit(&preds, &effects);
    let inf = WaldInference { beta: &m.beta, covariance: &m.cov, df: Some(m.df) };
    assert_eq!(stat_linear::marginal::slope_pairs(&m.design), vec![(0, 1)]);
    let s = group_slopes(&m.design, &inf, 0, 1, PairAdjust::Tukey, 0.95).unwrap();
    for (i, slope) in s.slopes.iter().enumerate() {
        close(slope.estimate, SLOPES[i], 1e-9);
        close(slope.std_error, SLOPES_SE[i], 1e-9);
        close(slope.interval.0, SLOPES_LOWER[i], 1e-9);
        close(slope.p_value, SLOPES_P[i], 1e-8);
    }
    for (i, pair) in s.pairs.iter().enumerate() {
        close(pair.estimate, SLOPE_PAIRS_EST[i], 1e-9);
        close(pair.p_value, SLOPE_PAIRS_P[i], 1e-6);
        close(pair.interval.0, SLOPE_PAIRS_LOWER[i], 1e-7);
    }
    close(s.equal_slopes.f, EQUAL_SLOPES_F[0], 1e-9);
    close(s.equal_slopes.p_value, EQUAL_SLOPES_P[0], 1e-9);
    // The same as the partial F test of the interaction.
    let rows: Vec<usize> = (0..20).collect();
    let x = m.design.matrix(&preds, &rows).unwrap();
    let options = OlsOptions::default();
    let f = fit_linear_model(&x, &Y, &options).unwrap();
    let t = effect_tests(&m.design, &x, &Y, &f, &options).unwrap();
    close(t[2].f_statistic.unwrap(), EQUAL_SLOPES_F[0], 1e-9);
}
