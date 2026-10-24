//! Bayesian regression and ANOVA against R's BayesFactor (0.9.12), from
//! `tests/data/linear_reference.R`: `regressionBF` (one g, numerical
//! integration: agreement to 1e-6 in ln BF), `anovaBF`/`lmBF` (several gs,
//! importance sampling: agreement within the combined Monte Carlo errors),
//! and `posterior` means (MCMC, 200,000 iterations).

use stat_bayes::linear::{
    ContinuousPrior, Effect, EffectKind, LinearSpec, ModelPrior, bayesian_linear_model, orthonormal_contrasts,
};

const MPG: [f64; 32] = [
    21.0, 21.0, 22.8, 21.4, 18.7, 18.1, 14.3, 24.4, 22.8, 19.2, 17.8, 16.4, 17.3, 15.2, 10.4, 10.4, 14.7, 32.4, 30.4,
    33.9, 21.5, 15.5, 15.2, 13.3, 19.2, 27.3, 26.0, 30.4, 15.8, 19.7, 15.0, 21.4,
];
const WT: [f64; 32] = [
    2.62, 2.875, 2.32, 3.215, 3.44, 3.46, 3.57, 3.19, 3.15, 3.44, 3.44, 4.07, 3.73, 3.78, 5.25, 5.424, 5.345, 2.2,
    1.615, 1.835, 2.465, 3.52, 3.435, 3.84, 3.845, 1.935, 2.14, 1.513, 3.17, 2.77, 3.57, 2.78,
];
const HP: [f64; 32] = [
    110.0, 110.0, 93.0, 110.0, 175.0, 105.0, 245.0, 62.0, 95.0, 123.0, 123.0, 180.0, 180.0, 180.0, 205.0, 215.0, 230.0,
    66.0, 52.0, 65.0, 97.0, 150.0, 150.0, 245.0, 175.0, 66.0, 91.0, 113.0, 264.0, 175.0, 335.0, 109.0,
];
const QSEC: [f64; 32] = [
    16.46, 17.02, 18.61, 19.44, 17.02, 20.22, 15.84, 20.0, 22.9, 18.3, 18.9, 17.4, 17.6, 18.0, 17.98, 17.82, 17.42,
    19.47, 18.52, 19.9, 20.01, 16.87, 17.3, 15.41, 17.05, 18.9, 16.7, 16.9, 14.5, 15.5, 14.6, 18.6,
];
const LEN: [f64; 60] = [
    4.2, 11.5, 7.3, 5.8, 6.4, 10.0, 11.2, 11.2, 5.2, 7.0, 16.5, 16.5, 15.2, 17.3, 22.5, 17.3, 13.6, 14.5, 18.8, 15.5,
    23.6, 18.5, 33.9, 25.5, 26.4, 32.5, 26.7, 21.5, 23.3, 29.5, 15.2, 21.5, 17.6, 9.7, 14.5, 10.0, 8.2, 9.4, 16.5, 9.7,
    19.7, 23.3, 23.6, 26.4, 20.0, 25.2, 25.8, 21.2, 14.5, 27.3, 25.5, 26.4, 22.4, 24.5, 24.8, 30.9, 26.4, 27.3, 29.4,
    23.0,
];

fn continuous(name: &str, x: &[f64]) -> Effect {
    Effect {
        name: name.into(),
        kind: EffectKind::Continuous,
        columns: vec![x.to_vec()],
        parents: vec![],
        parameters: vec![(name.into(), vec![1.0])],
    }
}

/// A factor coded by orthonormal contrasts, with level-effect parameters.
fn factor(name: &str, codes: &[usize], levels: &[&str]) -> Effect {
    let q = orthonormal_contrasts(levels.len());
    let columns = (0..levels.len() - 1).map(|j| codes.iter().map(|&c| q[c][j]).collect()).collect();
    Effect {
        name: name.into(),
        kind: EffectKind::Categorical,
        columns,
        parents: vec![],
        parameters: levels.iter().enumerate().map(|(i, l)| (format!("{name}-{l}"), q[i].clone())).collect(),
    }
}

fn spec(y: &[f64], effects: Vec<Effect>, r_cont: f64) -> LinearSpec {
    LinearSpec {
        y: y.to_vec(),
        effects,
        continuous_prior: ContinuousPrior::ZellnerSiow { r: r_cont },
        r_categorical: 0.5,
        model_prior: ModelPrior::Uniform,
        full_model_only: false,
        level: 0.95,
        draws: 40_000,
        seed: 11,
    }
}

const MEDIUM_CONT: f64 = std::f64::consts::SQRT_2 / 4.0;

#[test]
fn regression_bayes_factors() {
    for (r, want) in [
        (
            MEDIUM_CONT,
            [
                17.636_688_982_115_96,
                10.950_171_900_003_7,
                1.222_393_311_395_67,
                20.485_703_335_774_22,
                20.456_516_469_791_32,
                10.483_786_529_576_57,
                19.146_070_865_510_81,
            ],
        ),
        (
            1.0,
            [
                18.383_265_084_408_6,
                11.473_813_547_441_1,
                0.833_687_338_440_7,
                21.219_164_515_617_5,
                21.189_264_124_628_3,
                10.810_651_846_795,
                19.783_453_706_224,
            ],
        ),
    ] {
        let s = spec(&MPG, vec![continuous("wt", &WT), continuous("hp", &HP), continuous("qsec", &QSEC)], r);
        let res = bayesian_linear_model(&s).unwrap();
        assert_eq!(res.models.len(), 8);
        for (effects, w) in
            [vec![0], vec![1], vec![2], vec![0, 1], vec![0, 2], vec![1, 2], vec![0, 1, 2]].iter().zip(want)
        {
            let m = res.models.iter().find(|m| &m.effects == effects).unwrap();
            assert!((m.ln_bf - w).abs() < 1e-6, "r {r} {effects:?}: {} vs {w}", m.ln_bf);
            assert_eq!(m.error, 0.0);
        }
        let total: f64 = res.models.iter().map(|m| m.posterior).sum();
        assert!((total - 1.0).abs() < 1e-12);
    }
}

#[test]
fn regression_posterior_means() {
    // lmBF(mpg ~ wt + hp) posterior (MCMC, 200,000 draws): wt mean −3.78572,
    // sd 0.66351, 95% [−5.09284, −2.47252]; hp mean −0.0310418, sd 0.0094330.
    let s = LinearSpec {
        full_model_only: true,
        ..spec(&MPG, vec![continuous("wt", &WT), continuous("hp", &HP)], MEDIUM_CONT)
    };
    let res = bayesian_linear_model(&s).unwrap();
    let wt = res.best.iter().find(|e| e.label == "wt").unwrap();
    let hp = res.best.iter().find(|e| e.label == "hp").unwrap();
    assert!((wt.mean + 3.785_717_047_491_2).abs() < 0.01, "{}", wt.mean);
    assert!((wt.sd - 0.663_514_155_309_4).abs() < 0.01, "{}", wt.sd);
    assert!((wt.interval.0 + 5.092_839_197_770_65).abs() < 0.03);
    assert!((wt.interval.1 + 2.472_521_676_059_59).abs() < 0.03);
    assert!((hp.mean + 0.031_041_825_611_888_6).abs() < 1e-4);
    assert!((hp.sd - 0.009_432_965_202_816_8).abs() < 1e-4);
}

fn tooth() -> (Vec<usize>, Vec<usize>, Vec<f64>) {
    let supp: Vec<usize> = (0..60).map(|i| usize::from(i < 30)).collect(); // OJ = 0, VC = 1
    let dose: Vec<usize> = (0..60).map(|i| (i % 30) / 10).collect();
    let dose_n: Vec<f64> = dose.iter().map(|&d| [0.5, 1.0, 2.0][d]).collect();
    (supp, dose, dose_n)
}

#[test]
fn anova_bayes_factors() {
    let (supp, dose, _) = tooth();
    let s = factor("supp", &supp, &["OJ", "VC"]);
    let d = factor("dose", &dose, &["0.5", "1", "2"]);
    let q2 = orthonormal_contrasts(2);
    let q3 = orthonormal_contrasts(3);
    let inter = Effect {
        name: "supp:dose".into(),
        kind: EffectKind::Categorical,
        columns: (0..2).map(|j| (0..60).map(|i| q2[supp[i]][0] * q3[dose[i]][j]).collect()).collect(),
        parents: vec![0, 1],
        parameters: vec![],
    };
    let res = bayesian_linear_model(&spec(&LEN, vec![s, d, inter], MEDIUM_CONT)).unwrap();
    // anovaBF: supp 0.181285 (±0.00009%), dose 29.237181 (exact), supp + dose
    // 33.279557 (±0.6%), full 34.295123 (±0.7%), as ln BF.
    assert_eq!(res.models.len(), 5);
    for (effects, w, tol) in [
        (vec![0], 0.181_285_004_959_375, 1e-6),
        (vec![1], 29.237_180_943_122_01, 1e-6),
        (vec![0, 1], 33.279_557_060_718_36, 0.02),
        (vec![0, 1, 2], 34.295_123_348_257_63, 0.02),
    ] {
        let m = res.models.iter().find(|m| m.effects == effects).unwrap();
        assert!((m.ln_bf - w).abs() < tol, "{effects:?}: {} vs {w} (error {})", m.ln_bf, m.error);
    }
    // Interaction requires both main effects.
    assert!(res.models.iter().all(|m| !m.effects.contains(&2) || (m.effects.contains(&0) && m.effects.contains(&1))));
}

#[test]
fn unbalanced_one_way() {
    let (_, dose, _) = tooth();
    let drop = [0usize, 1, 2, 30, 44];
    let keep: Vec<usize> = (0..60).filter(|i| !drop.contains(i)).collect();
    let y: Vec<f64> = keep.iter().map(|&i| LEN[i]).collect();
    let codes: Vec<usize> = keep.iter().map(|&i| dose[i]).collect();
    let res = bayesian_linear_model(&spec(&y, vec![factor("dose", &codes, &["0.5", "1", "2"])], MEDIUM_CONT)).unwrap();
    let m = res.models.iter().find(|m| m.effects == [0]).unwrap();
    assert!((m.ln_bf - 24.773_531_465_615_5).abs() < 1e-6, "{}", m.ln_bf);
}

#[test]
fn level_effects_and_mixed_blocks() {
    let (supp, dose, dose_n) = tooth();
    // lmBF(len ~ supp + dose) posterior means: supp-OJ 1.67933, dose-0.5
    // −8.05386, dose-1 0.90360, dose-2 7.15026 (MCMC SDs 0.508, 0.72).
    let s = LinearSpec {
        full_model_only: true,
        ..spec(&LEN, vec![factor("supp", &supp, &["OJ", "VC"]), factor("dose", &dose, &["0.5", "1", "2"])], MEDIUM_CONT)
    };
    let res = bayesian_linear_model(&s).unwrap();
    for (label, mean, sd) in [
        ("supp-OJ", 1.679_331_416_100_82, 0.508_167_122_110_566),
        ("dose-0.5", -8.053_860_969_664_59, 0.724_140_225_895_347),
        ("dose-1", 0.903_602_299_691_08, 0.712_010_828_182_087),
        ("dose-2", 7.150_258_669_973_52, 0.721_132_490_266_709),
    ] {
        let e = res.best.iter().find(|e| e.label == label).unwrap();
        assert!((e.mean - mean).abs() < 0.01, "{label}: {} vs {mean}", e.mean);
        assert!((e.sd - sd).abs() < 0.01, "{label}: sd {} vs {sd}", e.sd);
    }
    // Factor plus a continuous covariate (two gs): lmBF 29.72685 ± 1.2%.
    let m = LinearSpec {
        full_model_only: true,
        ..spec(&LEN, vec![factor("supp", &supp, &["OJ", "VC"]), continuous("doseN", &dose_n)], MEDIUM_CONT)
    };
    let res = bayesian_linear_model(&m).unwrap();
    let full = res.models.iter().find(|x| x.effects.len() == 2).unwrap();
    assert!((full.ln_bf - 29.726_846_477_712_9).abs() < 0.03, "{} ± {}", full.ln_bf, full.error);
}
