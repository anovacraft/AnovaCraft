//! Fits compared with an independent reference implementation
//! (`tests/data/reference_cases.py`): dense marginal-likelihood REML/ML for
//! normal models with Satterthwaite degrees of freedom computed in the
//! variance parametrisation; for other families a dense Laplace
//! approximation and near-exact 60-point adaptive quadrature.

use stat_core::Matrix;
use stat_data::{JsonValue, parse_json};
use stat_mixed::{MixedFamily, MixedFit, MixedModel, MixedOptions, RandomTerm, fit_mixed};

fn cases() -> JsonValue {
    parse_json(include_str!("data/reference_cases.json")).unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn rows(v: &JsonValue) -> Matrix {
    let r: Vec<Vec<f64>> = v.as_array().unwrap().iter().map(nums).collect();
    Matrix::from_rows(&r).unwrap()
}

fn model(case: &JsonValue) -> MixedModel {
    let d = case.get("data").unwrap();
    let family = MixedFamily::from_key(case.get("family").unwrap().as_str().unwrap()).unwrap();
    let random = d
        .get("terms")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            let g = t.get("g").unwrap().as_usize().unwrap();
            RandomTerm {
                group: t.get("group").unwrap().as_str().unwrap().to_owned(),
                levels: (0..g).map(|j| j.to_string()).collect(),
                codes: nums(t.get("codes").unwrap()).into_iter().map(|c| c as usize).collect(),
                effects: t
                    .get("effects")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e.as_str().unwrap().to_owned())
                    .collect(),
                z: rows(t.get("z").unwrap()),
                correlated: t.get("correlated").unwrap().as_bool().unwrap(),
            }
        })
        .collect();
    MixedModel {
        y: nums(d.get("y").unwrap()),
        x: rows(d.get("x").unwrap()),
        x_names: d.get("xn").unwrap().as_array().unwrap().iter().map(|e| e.as_str().unwrap().to_owned()).collect(),
        weights: d.get("w").map(nums).unwrap_or_default(),
        offset: Vec::new(),
        random,
        family,
        residual_groups: None,
        categories: Vec::new(),
    }
}

/// Compare a fit with a reference result; tolerances are relative to each
/// quantity's standard error (estimates) or absolute (−2 log L).
fn compare(name: &str, fit: &MixedFit, r: &JsonValue, dev_tol: f64, est_tol: f64) {
    let crit = r.get("crit").unwrap().as_f64().unwrap();
    eprintln!("{name}: -2LL {} vs {crit}", fit.minus_two_log_likelihood);
    assert!(
        (fit.minus_two_log_likelihood - crit).abs() < dev_tol,
        "{name}: -2LL {} vs {crit}",
        fit.minus_two_log_likelihood
    );
    let beta = nums(r.get("beta").unwrap());
    let se = nums(r.get("se").unwrap());
    for (j, f) in fit.fixed.iter().enumerate() {
        eprintln!("  {}: {} ({}) vs {} ({})", f.name, f.estimate, f.std_error, beta[j], se[j]);
        assert!((f.estimate - beta[j]).abs() < est_tol * se[j], "{name} β{j}: {} vs {}", f.estimate, beta[j]);
        assert!((f.std_error - se[j]).abs() < 2e-3 * se[j], "{name} SE{j}: {} vs {}", f.std_error, se[j]);
    }
    let comps = nums(r.get("comps").unwrap());
    for (c, want) in fit.variance_components.iter().zip(&comps) {
        eprintln!("  var {} {}: {} vs {want}", c.group, c.effect, c.variance);
        assert!(
            (c.variance - want).abs() < 1e-4 * want.max(0.01),
            "{name} variance {}: {} vs {want}",
            c.group,
            c.variance
        );
    }
    let cors = nums(r.get("cors").unwrap());
    for (c, want) in fit.correlations.iter().zip(&cors) {
        assert!((c.value - want).abs() < 1e-4, "{name} correlation: {} vs {want}", c.value);
    }
    if let Some(s) = r.get("scale").and_then(JsonValue::as_f64) {
        let got = fit.scale.as_ref().unwrap().value;
        eprintln!("  scale {got} vs {s}");
        assert!((got - s).abs() < 1e-4 * s, "{name} scale: {got} vs {s}");
    }
    if let Some(s2) = r.get("s2").and_then(JsonValue::as_f64) {
        let got = fit.residual.as_ref().unwrap().variance;
        assert!((got - s2).abs() < 1e-3 * s2, "{name} residual: {got} vs {s2}");
    }
    if let Some(dfs) = r.get("df") {
        for (f, want) in fit.fixed.iter().zip(nums(dfs)) {
            eprintln!("  df {}: {:?} vs {want}", f.name, f.df);
            assert!((f.df.unwrap() - want).abs() < 1e-3 * want, "{name} df {}: {:?} vs {want}", f.name, f.df);
        }
    }
}

#[test]
fn normal_models_match_the_dense_reference() {
    let all = cases();
    for name in ["lmm_slopes", "lmm_crossed"] {
        let case = all.get(name).unwrap();
        let m = model(case);
        let reml = fit_mixed(&m, &MixedOptions::default()).unwrap();
        compare(&format!("{name} REML"), &reml, case.get("reml").unwrap(), 1e-6, 1e-4);
        let ml = fit_mixed(&m, &MixedOptions { reml: false, ..MixedOptions::default() }).unwrap();
        compare(&format!("{name} ML"), &ml, case.get("ml").unwrap(), 1e-6, 1e-4);
    }
}

#[test]
fn laplace_fits_match_the_dense_reference() {
    let all = cases();
    for name in ["binomial_trials", "poisson", "gamma", "negative_binomial", "binary_crossed", "poisson_slope"] {
        let case = all.get(name).unwrap();
        let fit = fit_mixed(&model(case), &MixedOptions::default()).unwrap();
        compare(&format!("{name} Laplace"), &fit, case.get("laplace").unwrap(), 1e-6, 1e-4);
    }
}

#[test]
fn quadrature_fits_match_near_exact_integrals() {
    let all = cases();
    for name in ["binomial_trials", "poisson", "gamma", "negative_binomial"] {
        let case = all.get(name).unwrap();
        let options = MixedOptions { quadrature_points: 25, ..MixedOptions::default() };
        let fit = fit_mixed(&model(case), &options).unwrap();
        compare(&format!("{name} AGQ"), &fit, case.get("agq").unwrap(), 1e-6, 1e-4);
    }
}

#[test]
fn contrast_tests_agree_with_coefficient_tests() {
    let all = cases();
    // Normal: a one-row F equals t² with the same Satterthwaite df.
    let m = model(all.get("lmm_crossed").unwrap());
    let fit = fit_mixed(&m, &MixedOptions::default()).unwrap();
    for (j, f) in fit.fixed.iter().enumerate() {
        let mut l = vec![0.0; fit.fixed.len()];
        l[j] = 1.0;
        let t = fit.test_contrast(&[l]).unwrap();
        assert_eq!(t.num_df, 1);
        assert!((t.statistic - f.statistic.powi(2)).abs() < 1e-9 * t.statistic.max(1.0));
        assert!((t.den_df.unwrap() - f.df.unwrap()).abs() < 1e-6 * f.df.unwrap());
        assert!((t.p_value - f.p_value).abs() < 1e-9);
    }
    // Two rows: F between the single-row statistics' range, df combined.
    let both = fit.test_contrast(&[vec![0.0, 1.0, 0.0], vec![0.0, 0.0, 1.0]]).unwrap();
    assert_eq!(both.num_df, 2);
    let den = both.den_df.unwrap();
    assert!(den > 60.0 && den < 75.0, "{den}");
    // A dependent row is dropped.
    let dup = fit.test_contrast(&[vec![0.0, 1.0, 0.0], vec![0.0, 2.0, 0.0]]).unwrap();
    assert_eq!(dup.num_df, 1);
    assert!((dup.statistic - fit.fixed[1].statistic.powi(2)).abs() < 1e-6 * dup.statistic);
    assert!(fit.test_contrast(&[vec![0.0; 3]]).is_err());
    assert!(fit.test_contrast(&[vec![1.0]]).is_err());
    // Other families: Wald χ² = z².
    let g = fit_mixed(&model(all.get("binomial_trials").unwrap()), &MixedOptions::default()).unwrap();
    let t = g.test_contrast(&[vec![0.0, 1.0, 0.0, 0.0]]).unwrap();
    assert!(t.den_df.is_none() && (t.statistic - g.fixed[1].statistic.powi(2)).abs() < 1e-9 * t.statistic);
    let three =
        g.test_contrast(&[vec![0.0, 1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0, 0.0], vec![0.0, 0.0, 0.0, 1.0]]).unwrap();
    assert_eq!(three.num_df, 3);
    assert!(three.p_value > 0.0 && three.p_value < 0.05);
}

#[test]
fn independent_slopes_offsets_and_weights() {
    let all = cases();
    let mut m = model(all.get("lmm_slopes").unwrap());
    let corr = fit_mixed(&m, &MixedOptions { reml: false, ..MixedOptions::default() }).unwrap();
    m.random[0].correlated = false;
    let diag = fit_mixed(&m, &MixedOptions { reml: false, ..MixedOptions::default() }).unwrap();
    assert!(diag.correlations.is_empty() && diag.theta.len() == 2);
    // Nested models: dropping the correlation cannot improve the likelihood.
    assert!(diag.minus_two_log_likelihood >= corr.minus_two_log_likelihood - 1e-8);
    assert_eq!(diag.n_parameters + 1, corr.n_parameters);
    // An offset of c shifts the intercept by −c and changes nothing else.
    let mut p = model(all.get("poisson").unwrap());
    let base = fit_mixed(&p, &MixedOptions::default()).unwrap();
    p.offset = vec![0.7; p.y.len()];
    let shifted = fit_mixed(&p, &MixedOptions::default()).unwrap();
    assert!((shifted.fixed[0].estimate - (base.fixed[0].estimate - 0.7)).abs() < 1e-5);
    assert!((shifted.minus_two_log_likelihood - base.minus_two_log_likelihood).abs() < 1e-7);
    assert!((shifted.fitted[3] - base.fitted[3]).abs() < 1e-5 * base.fitted[3]);
    // Normal weights are precisions: with all weights 4 the residual variance
    // per unit weight is 4 times larger; nothing else changes.
    let mut w = model(all.get("lmm_crossed").unwrap());
    let plain = fit_mixed(&w, &MixedOptions::default()).unwrap();
    w.weights = vec![4.0; w.y.len()];
    let weighted = fit_mixed(&w, &MixedOptions::default()).unwrap();
    let (r0, r1) = (plain.residual.as_ref().unwrap().variance, weighted.residual.as_ref().unwrap().variance);
    assert!((r1 - 4.0 * r0).abs() < 1e-6 * r0, "{r1} vs 4 × {r0}");
    for (a, b) in plain.variance_components.iter().zip(&weighted.variance_components) {
        assert!((a.variance - b.variance).abs() < 1e-5 * a.variance);
    }
    for (a, b) in plain.fixed.iter().zip(&weighted.fixed) {
        assert!((a.estimate - b.estimate).abs() < 1e-6 * a.std_error);
    }
    // Fitted values: conditional means follow the random effects; marginal ones do not.
    assert!(base.fitted.iter().zip(&base.fitted_marginal).any(|(a, b)| (a - b).abs() > 1e-3));
}
