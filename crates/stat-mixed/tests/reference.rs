//! Fits checked against closed-form results and published values.

use stat_core::Matrix;
use stat_mixed::{MixedFamily, MixedModel, MixedOptions, RandomTerm, fit_mixed};

/// Davies & Goldsmith's dye yields: 6 batches × 5 preparations (in lme4 as
/// `Dyestuff`).
fn dyestuff() -> (Vec<f64>, Vec<usize>) {
    let y = [
        1545., 1440., 1440., 1520., 1580., 1540., 1555., 1490., 1560., 1495., 1595., 1550., 1605., 1510., 1560., 1445.,
        1440., 1595., 1465., 1545., 1595., 1630., 1515., 1635., 1625., 1520., 1455., 1450., 1480., 1445.,
    ];
    (y.to_vec(), (0..30).map(|i| i / 5).collect())
}

fn intercept_model(y: Vec<f64>, codes: Vec<usize>, g: usize, family: MixedFamily) -> MixedModel {
    let n = y.len();
    MixedModel {
        y,
        x: Matrix::from_columns(&[vec![1.0; n]]).unwrap(),
        x_names: vec!["Intercept".into()],
        weights: Vec::new(),
        offset: Vec::new(),
        random: vec![RandomTerm {
            group: "Batch".into(),
            levels: (0..g).map(|j| format!("{j}")).collect(),
            codes,
            effects: vec!["Intercept".into()],
            z: Matrix::from_columns(&[vec![1.0; n]]).unwrap(),
            correlated: true,
        }],
        family,
        residual_groups: None,
        categories: Vec::new(),
    }
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-12), "{what}: {a} vs {b}");
}

#[test]
fn dyestuff_reml_and_ml_match_anova_estimators() {
    let (y, codes) = dyestuff();
    let model = intercept_model(y, codes, 6, MixedFamily::Normal);
    let fit = fit_mixed(&model, &MixedOptions::default()).unwrap();
    // Balanced one-way layout: REML = ANOVA estimators (MSE, (MSB − MSE)/n).
    let res = fit.residual.as_ref().unwrap();
    close(res.variance, 2451.25, 1e-6, "residual variance");
    close(fit.variance_components[0].variance, 1764.05, 1e-6, "batch variance");
    let b = &fit.fixed[0];
    close(b.estimate, 1527.5, 1e-9, "intercept");
    close(b.std_error, (11271.5f64 / 30.0).sqrt(), 1e-6, "intercept SE");
    // Satterthwaite df is exactly a − 1 = 5 here.
    close(b.df.unwrap(), 5.0, 1e-3, "df");
    // lme4: REML criterion 319.65.
    close(fit.minus_two_log_likelihood, 319.6543, 1e-5, "REML criterion");
    assert!(fit.warnings.is_empty(), "{:?}", fit.warnings);
    // BLUPs: shrunken batch deviations, σ²b / (σ²b + σ²e/n) × (ȳⱼ − ȳ).
    let shrink = 1764.05 / (1764.05 + 2451.25 / 5.0);
    for (j, m) in [1505.0, 1528.0, 1564.0, 1498.0, 1600.0, 1470.0].iter().enumerate() {
        close(fit.modes[0].values[j][0], shrink * (m - 1527.5), 1e-5, "BLUP");
    }
    // ML: σ²b = ((1 − 1/a) MSB − MSE)/n; lme4 deviance 327.3271.
    let ml = fit_mixed(&model, &MixedOptions { reml: false, ..MixedOptions::default() }).unwrap();
    close(ml.variance_components[0].variance, 1388.3333, 1e-5, "ML batch variance");
    close(ml.residual.as_ref().unwrap().variance, 2451.25, 1e-5, "ML residual");
    close(ml.minus_two_log_likelihood, 327.3271, 1e-5, "ML deviance");
    close(ml.aic, 327.3271 + 6.0, 1e-5, "AIC");
}
