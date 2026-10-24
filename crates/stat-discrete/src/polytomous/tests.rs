use super::*;
use crate::glm::{Family, GlmOptions, fit_glm};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

fn levels(j: usize) -> Vec<String> {
    (0..j).map(|k| format!("c{k}")).collect()
}

/// A small ordered dataset: 4 categories, two predictors.
fn data() -> (Matrix, Vec<usize>) {
    let x1 =
        [-1.2, -0.8, -0.5, -0.3, 0.0, 0.1, 0.3, 0.4, 0.6, 0.9, 1.1, 1.4, -1.0, -0.2, 0.2, 0.7, 1.2, -0.6, 0.5, 1.6];
    let x2 = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
    let y = vec![0, 0, 1, 0, 1, 2, 1, 1, 2, 2, 3, 3, 1, 0, 2, 3, 3, 0, 2, 3];
    (Matrix::from_columns(&[&x1[..], &x2[..]]).unwrap(), y)
}

/// Two categories: the cumulative logit is the binary logistic model with
/// the signs of the intercept reversed, `P(Y = 1) = F(xᵀβ − α)`, and the
/// baseline-category logit is that model itself.
#[test]
fn two_categories_reduce_to_binary_logistic() {
    let (x, y4) = data();
    let y: Vec<usize> = y4.iter().map(|&k| usize::from(k >= 2)).collect();
    let yf: Vec<f64> = y.iter().map(|&k| k as f64).collect();
    let glm = fit_glm(&x, &yf, &GlmOptions::new(Family::Binomial)).unwrap();
    let ord = fit_ordinal_logistic(&x, &y, &levels(2), &PolytomousOptions::default()).unwrap();
    assert!(close(ord.thresholds[0].estimate, -glm.coefficients[0].estimate, 1e-8));
    assert!(close(ord.thresholds[0].std_error, glm.coefficients[0].std_error, 1e-6));
    for j in 0..2 {
        assert!(close(ord.slopes[j].estimate, glm.coefficients[j + 1].estimate, 1e-8));
        assert!(close(ord.slopes[j].std_error, glm.coefficients[j + 1].std_error, 1e-6));
    }
    assert!(close(ord.statistics.log_likelihood, glm.log_likelihood, 1e-10));
    assert!(ord.parallel_lines.is_none(), "no test with two categories");
    let nom = fit_nominal_logistic(&x, &y, &levels(2), 0, &PolytomousOptions::default()).unwrap();
    for (c, g) in nom.equations[0].coefficients.iter().zip(&glm.coefficients) {
        assert!(close(c.estimate, g.estimate, 1e-8) && close(c.std_error, g.std_error, 1e-6));
    }
    assert!(close(nom.statistics.aic, glm.aic, 1e-9));
}

/// Frequency weights equal repeating rows.
#[test]
fn weights_equal_repeated_rows() {
    let (x, y) = data();
    let w: Vec<f64> = (0..y.len()).map(|i| (i % 3 + 1) as f64).collect();
    let mut rows = Vec::new();
    let mut ys = Vec::new();
    for i in 0..y.len() {
        for _ in 0..(w[i] as usize) {
            rows.push(x.row(i));
            ys.push(y[i]);
        }
    }
    let xr = Matrix::from_rows(&rows).unwrap();
    let weighted = PolytomousOptions { weights: w, ..PolytomousOptions::default() };
    let a = fit_ordinal_logistic(&x, &y, &levels(4), &weighted).unwrap();
    let b = fit_ordinal_logistic(&xr, &ys, &levels(4), &PolytomousOptions::default()).unwrap();
    for (c, d) in a.thresholds.iter().chain(&a.slopes).zip(b.thresholds.iter().chain(&b.slopes)) {
        assert!(close(c.estimate, d.estimate, 1e-9) && close(c.std_error, d.std_error, 1e-8));
    }
    assert!(close(a.statistics.bic, b.statistics.bic, 1e-10));
    assert_eq!(a.statistics.n_rows, 20);
    assert!(close(a.statistics.total_weight, ys.len() as f64, 0.0));
    let (pa, pb) = (a.parallel_lines.unwrap(), b.parallel_lines.unwrap());
    assert!(close(pa.chi_square, pb.chi_square, 1e-8));
    let a = fit_nominal_logistic(&x, &y, &levels(4), 1, &weighted).unwrap();
    let b = fit_nominal_logistic(&xr, &ys, &levels(4), 1, &PolytomousOptions::default()).unwrap();
    assert!(close(a.statistics.log_likelihood, b.statistics.log_likelihood, 1e-10));
    for (e, f) in a.equations.iter().zip(&b.equations) {
        for (c, d) in e.coefficients.iter().zip(&f.coefficients) {
            assert!(close(c.estimate, d.estimate, 1e-8) && close(c.std_error, d.std_error, 1e-7));
        }
    }
}

/// The reference category changes the parametrization only:
/// `βⱼ(ref r) = βⱼ(ref 0) − β_r(ref 0)`, the same probabilities and ℓ.
#[test]
fn reference_category_reparametrizes() {
    let (x, y) = data();
    let a = fit_nominal_logistic(&x, &y, &levels(4), 0, &PolytomousOptions::default()).unwrap();
    let b = fit_nominal_logistic(&x, &y, &levels(4), 3, &PolytomousOptions::default()).unwrap();
    assert!(close(a.statistics.log_likelihood, b.statistics.log_likelihood, 1e-11));
    for (pa, pb) in a.probabilities.iter().zip(&b.probabilities) {
        for (u, v) in pa.iter().zip(pb) {
            assert!((u - v).abs() < 1e-9);
        }
    }
    let eq = |f: &NominalLogitFit, level: usize| f.equations.iter().find(|e| e.level == level).cloned();
    let last = eq(&a, 3).unwrap();
    for level in 1..3 {
        let (ea, eb) = (eq(&a, level).unwrap(), eq(&b, level).unwrap());
        for k in 0..3 {
            let want = ea.coefficients[k].estimate - last.coefficients[k].estimate;
            assert!(close(eb.coefficients[k].estimate, want, 1e-7));
        }
    }
    // Category 0 against reference 3 is the negated equation of 3 against 0.
    let e0 = eq(&b, 0).unwrap();
    for k in 0..3 {
        assert!(close(e0.coefficients[k].estimate, -last.coefficients[k].estimate, 1e-7));
        assert!(close(e0.coefficients[k].std_error, last.coefficients[k].std_error, 1e-6));
    }
}

#[test]
fn predictions_and_null_model() {
    let (x, y) = data();
    let fit = fit_ordinal_logistic(&x, &y, &levels(4), &PolytomousOptions::default()).unwrap();
    let pred = fit.predict(&x).unwrap();
    for (p, q) in pred.iter().zip(&fit.probabilities) {
        assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        for (a, b) in p.iter().zip(q) {
            assert!((a - b).abs() < 1e-15);
        }
    }
    assert!(fit.thresholds.windows(2).all(|t| t[1].estimate > t[0].estimate));
    assert_eq!(fit.thresholds[0].name, "c0 | c1");
    assert_eq!(fit.slopes[1].name, "x2");
    assert_eq!(fit.statistics.whole_model.df, 2);
    assert_eq!(fit.parallel_lines.unwrap().df, 4);
    assert!(fit.predict(&Matrix::zeros(1, 3)).is_err());
    // No predictors: ℓ equals the null log-likelihood, thresholds the
    // cumulative logits.
    let empty = Matrix::zeros(y.len(), 0);
    let null = fit_ordinal_logistic(&empty, &y, &levels(4), &PolytomousOptions::default()).unwrap();
    assert!(close(null.statistics.log_likelihood, null.statistics.null_log_likelihood, 1e-12));
    assert!(close(null.thresholds[0].estimate, (5.0f64 / 15.0).ln(), 1e-9), "5 of 20 in c0");
    assert!(null.parallel_lines.is_none());
    assert!(null.statistics.whole_model.chi_square.abs() < 1e-9);
    let nom = fit_nominal_logistic(&empty, &y, &levels(4), 0, &PolytomousOptions::default()).unwrap();
    assert!(close(nom.statistics.log_likelihood, null.statistics.log_likelihood, 1e-12));
    assert!(close(nom.equations[0].coefficients[0].estimate, 0.0, 1e-9), "5 and 5 in c0 and c1");
}

#[test]
fn invalid_inputs_are_refused() {
    let (x, y) = data();
    let o = PolytomousOptions::default();
    assert!(fit_ordinal_logistic(&x, &y, &levels(5), &o).is_err(), "an empty category");
    assert!(fit_ordinal_logistic(&x, &y, &levels(3), &o).is_err(), "a code out of range");
    assert!(fit_ordinal_logistic(&x, &y[1..], &levels(4), &o).is_err(), "row counts");
    assert!(fit_nominal_logistic(&x, &y, &levels(4), 4, &o).is_err(), "reference");
    let bad = PolytomousOptions { weights: vec![-1.0; 20], ..PolytomousOptions::default() };
    assert!(fit_ordinal_logistic(&x, &y, &levels(4), &bad).is_err());
    let twice = Matrix::from_columns(&[x.col(0), x.col(0)]).unwrap();
    assert!(fit_ordinal_logistic(&twice, &y, &levels(4), &o).is_err(), "dependent columns");
    assert!(fit_nominal_logistic(&twice, &y, &levels(4), 0, &o).is_err());
}

/// Completely separated categories: the estimates diverge and the fit says so.
#[test]
fn separation_is_reported() {
    let x = Matrix::from_columns(&[[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]]).unwrap();
    let y = [0, 0, 0, 1, 1, 1, 2, 2, 2];
    let o = PolytomousOptions::default();
    let fit = fit_ordinal_logistic(&x, &y, &levels(3), &o);
    if let Ok(f) = fit {
        assert!(!f.statistics.warnings.is_empty(), "{:?}", f.statistics);
    }
    let fit = fit_nominal_logistic(&x, &y, &levels(3), 0, &o);
    if let Ok(f) = fit {
        assert!(!f.statistics.warnings.is_empty());
    }
}
