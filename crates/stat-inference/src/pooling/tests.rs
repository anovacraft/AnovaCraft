//! Reference values from R 4.3 with mice 3.16 (`pool.scalar`,
//! `barnard.rubin`) and mitml (`.D1`, the D₁ statistic `mice::D1` calls);
//! the script is `crates/app-model/tests/data/mice_reference.R`.

#![allow(clippy::excessive_precision)] // reference values are printed to 17 digits

use super::*;

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
}

const Q: [f64; 5] = [1.21, 1.35, 0.98, 1.10, 1.42];
const U: [f64; 5] = [0.040, 0.052, 0.047, 0.038, 0.045];

#[test]
fn scalar_pooling_matches_mice_without_complete_data_df() {
    let p = pool_scalar(&Q, &U, f64::INFINITY, 0.0, 0.95).unwrap();
    close(p.estimate, 1.212, 1e-14);
    close(p.within, 0.044400000000000002, 1e-14);
    close(p.between, 0.032169999999999997, 1e-13);
    close(p.total, 0.083003999999999994, 1e-13);
    close(p.df, 18.492406046561204, 1e-12);
    close(p.riv, 0.86945945945945924, 1e-13);
    close(p.fmi, 0.51486304155469176, 1e-12);
    close(p.p_value, 0.00050323165445406747, 1e-9);
    close(p.interval.0, 0.60786885345059727, 1e-10);
}

#[test]
fn scalar_pooling_matches_mice_with_barnard_rubin_df() {
    let p = pool_scalar(&Q, &U, 20.0, 0.0, 0.95).unwrap();
    close(p.df, 6.3917607702768233, 1e-12);
    close(p.fmi, 0.57899734992042018, 1e-12);
    close(p.p_value, 0.0049020631985628475, 1e-9);
    close(p.interval.0, 0.51737708853751119, 1e-10);
    close(p.lambda, (1.0 + 0.2) * p.between / p.total, 1e-15);
}

#[test]
fn no_between_variance_floors_lambda() {
    let p = pool_scalar(&[2.0; 4], &[0.5; 4], f64::INFINITY, 0.0, 0.95).unwrap();
    assert_eq!(p.between, 0.0);
    close(p.df, 3.0 / 1e-8, 1e-12);
    close(p.total, 0.5, 1e-15);
}

fn wald_inputs() -> (Vec<Vec<f64>>, Vec<Vec<Vec<f64>>>) {
    let q1 = [1.0, 2.0, 3.0, 1.5, 2.5];
    let q2 = [0.5, 0.2, 0.9, 0.4, 0.7];
    let est = (0..5).map(|l| vec![q1[l], q2[l]]).collect();
    let cov = (1..=5)
        .map(|l| {
            let l = f64::from(l);
            vec![vec![0.30 + 0.01 * l, 0.05], vec![0.05, 0.10 + 0.02 * l]]
        })
        .collect();
    (est, cov)
}

#[test]
fn wald_test_matches_mitml_d1() {
    let (est, cov) = wald_inputs();
    let t = pool_wald(&est, &cov, &[0.0, 0.0], f64::INFINITY).unwrap();
    close(t.statistic, 2.696164938542883, 1e-12);
    close(t.df_den, 13.857460112495563, 1e-12);
    close(t.riv, 1.3161829025844933, 1e-12);
    close(t.p_value, 0.10255969706947436, 1e-9);
    assert_eq!(t.df_num, 2);
    let t = pool_wald(&est, &cov, &[0.0, 0.0], 30.0).unwrap();
    close(t.df_den, 5.9470532439401556, 1e-12);
    close(t.p_value, 0.14674298633753224, 1e-9);
}

#[test]
fn one_parameter_wald_is_the_squared_scalar_t() {
    let est: Vec<Vec<f64>> = Q.iter().map(|&q| vec![q]).collect();
    let cov: Vec<Vec<Vec<f64>>> = U.iter().map(|&u| vec![vec![u]]).collect();
    let w = pool_wald(&est, &cov, &[0.0], f64::INFINITY).unwrap();
    let s = pool_scalar(&Q, &U, f64::INFINITY, 0.0, 0.95).unwrap();
    // D₁ divides by (1 + r)Ū where Rubin's T uses Ū + (1 + 1/m)B: equal for k = 1.
    close(w.statistic, s.t * s.t, 1e-12);
}

#[test]
fn likelihood_ratio_test_follows_meng_rubin() {
    let own = [10.0, 12.0, 9.0, 11.0];
    let pooled = [9.5, 11.0, 8.0, 10.5];
    let t = pool_likelihood_ratio(&own, &pooled, 2).unwrap();
    let r = 5.0 / (2.0 * 3.0) * (10.5 - 9.75);
    close(t.riv, r, 1e-15);
    close(t.statistic, 9.75 / (2.0 * (1.0 + r)), 1e-15);
    close(t.df_den, 4.0 + (6.0 - 4.0) * (1.0 + (1.0 - 2.0 / 6.0) / r).powi(2), 1e-15);
}

#[test]
fn correlations_pool_on_the_z_scale() {
    let r = [0.40, 0.45, 0.38];
    let c = pool_correlation(&r, &[53, 53, 53], 0.95).unwrap();
    let zbar = r.iter().map(|v: &f64| v.atanh()).sum::<f64>() / 3.0;
    close(c.estimate, zbar.tanh(), 1e-15);
    assert!(c.interval.0 < c.estimate && c.estimate < c.interval.1);
    let r2 = pool_r_squared(&[0.16, 0.2025, 0.1444], &[53, 53, 53], 0.95).unwrap();
    close(r2.estimate, zbar.tanh().powi(2), 1e-12);
}

#[test]
fn bad_inputs_are_refused() {
    assert!(pool_scalar(&[1.0], &[1.0], f64::INFINITY, 0.0, 0.95).is_err());
    assert!(pool_scalar(&[1.0, 2.0], &[1.0], f64::INFINITY, 0.0, 0.95).is_err());
    assert!(pool_scalar(&[1.0, 2.0], &[-1.0, 1.0], f64::INFINITY, 0.0, 0.95).is_err());
    assert!(pool_correlation(&[1.0, 0.5], &[10, 10], 0.95).is_err());
    assert!(pool_likelihood_ratio(&[1.0, 2.0], &[1.0, 2.0], 0).is_err());
}
