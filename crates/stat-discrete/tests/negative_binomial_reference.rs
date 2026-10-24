//! Negative binomial regression against R 4.3.3 `MASS::glm.nb` and
//! `anova.negbin`; the script is `tests/data/negative_binomial_reference.R`.
//! glm.nb stops at a relative change of about 1e-8 in its criterion, so
//! estimates agree to about six digits.
#![allow(clippy::excessive_precision)]

use stat_core::Matrix;
use stat_discrete::{NegativeBinomialOptions, fit_negative_binomial, negative_binomial_lr_test};

const X: [f64; 16] = [0.5, 1.1, 1.8, 2.2, 2.9, 3.4, 4.1, 4.5, 5.2, 5.8, 6.3, 7.0, 7.4, 8.1, 8.8, 9.3];
const Y: [f64; 16] = [1.0, 0.0, 3.0, 2.0, 1.0, 6.0, 4.0, 2.0, 9.0, 5.0, 3.0, 14.0, 6.0, 11.0, 20.0, 8.0];

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{a} vs {b}");
}

#[test]
fn matches_glm_nb() {
    let g: Vec<f64> = (0..16).map(|i| f64::from(u8::from(i % 2 == 1))).collect();
    let full = Matrix::from_columns(&[X.to_vec(), g]).unwrap();
    let m = fit_negative_binomial(&full, &Y, &NegativeBinomialOptions::default()).unwrap();
    let coef = [0.17532555858232943, 0.28215060973701561, -0.082473823755566097];
    let se = [0.38391139784077533, 0.056289223451964222, 0.27751449504822695];
    let pz = [0.647899380359654, 5.3723023051475864e-07, 0.76632340648791253];
    for (j, c) in m.coefficients.iter().enumerate() {
        close(c.estimate, coef[j], 1e-5);
        close(c.std_error, se[j], 1e-5);
        close(c.p_value, pz[j], 1e-4);
    }
    close(m.theta, 9.6957541483961975, 1e-4);
    close(m.theta_std_error, 8.9466364355816452, 1e-4);
    close(m.log_likelihood, -36.816662230090699, 1e-8);
    close(m.aic, 81.633324460181399, 1e-8);
    close(m.deviance, 15.834917105446015, 1e-5);
    let reduced =
        fit_negative_binomial(&Matrix::from_columns(&[X.to_vec()]).unwrap(), &Y, &NegativeBinomialOptions::default())
            .unwrap();
    let (chi2, df, p) = negative_binomial_lr_test(&m, &reduced).unwrap();
    assert_eq!(df, 1);
    close(chi2, 0.087831914102650899, 1e-5);
    close(p, 0.76695173664209915, 1e-5);
}

#[test]
fn rejects_negative_counts() {
    let x = Matrix::from_columns(&[vec![1.0, 2.0, 3.0, 4.0]]).unwrap();
    assert!(fit_negative_binomial(&x, &[1.0, -1.0, 2.0, 3.0], &NegativeBinomialOptions::default()).is_err());
}
