//! Latent class tests. Reference values come from
//! `tests/data/latent_class_reference.py`: stepmix 3.0 (categorical
//! measurement, full-information likelihood with missing responses, 30
//! random starts, absolute tolerance 1e-13), with classes ordered by size,
//! and standard errors from an independent numerical observed information
//! (central differences of the analytic score, a different logit baseline)
//! with the delta method.

use super::*;

const DATA: &str = include_str!("../../tests/data/latent_class_data.csv");

/// Stouffer & Toby's four role-conflict items (Goodman 1974, Table 1).
fn stouffer_toby() -> Vec<Vec<Option<usize>>> {
    let counts = [42, 23, 6, 25, 6, 24, 7, 38, 1, 4, 1, 6, 2, 9, 2, 20];
    let mut rows = Vec::new();
    for (p, &c) in counts.iter().enumerate() {
        let pattern: Vec<Option<usize>> = (0..4).map(|j| Some((p >> (3 - j)) & 1)).collect();
        rows.extend(std::iter::repeat_n(pattern, c));
    }
    rows
}

fn simulated() -> Vec<Vec<Option<usize>>> {
    DATA.lines().skip(1).map(|l| l.split(',').map(|v| v.parse().ok()).collect()).collect()
}

const SIM_LEVELS: [usize; 6] = [2, 2, 2, 3, 3, 4];

fn close(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{what}: {a} vs {b}");
}

fn options(k: usize) -> LatentClassOptions {
    LatentClassOptions { starts: 20, tolerance: 1e-14, max_iterations: 100_000, ..LatentClassOptions::new(k) }
}

#[test]
#[allow(clippy::unreadable_literal)]
fn stouffer_toby_two_classes() {
    let fit = fit_latent_classes(&stouffer_toby(), &[2; 4], &options(2)).unwrap();
    close(fit.log_likelihood, -504.46767011842155, 1e-10, "ll");
    let pi = [0.7207533276152291, 0.2792466723847691];
    let rho = [
        [0.7135877961496228, 0.9931931710328737],
        [0.3296190675080415, 0.9397639656975303],
        [0.354016187886421, 0.9265304443219714],
        [0.13237233042443403, 0.7691314124297004],
    ];
    let se_pi = 0.05809250500802666;
    let se_rho = [
        [0.040358861428216, 0.02532421324535216],
        [0.04969834936160751, 0.0659955846887909],
        [0.04855280712046034, 0.06565374810870805],
        [0.03833304142994942, 0.09520712134156319],
    ];
    for c in 0..2 {
        close(fit.class_sizes[c], pi[c], 1e-6, "pi");
        close(fit.class_size_se[c], se_pi, 1e-4, "se pi");
        for j in 0..4 {
            close(fit.probabilities[j][c][0], rho[j][c], 1e-6, "rho");
            close(fit.probabilities[j][c][1], 1.0 - rho[j][c], 1e-6, "rho");
            close(fit.probability_se[j][c][0], se_rho[j][c], 1e-4, "se rho");
            close(fit.probability_se[j][c][1], se_rho[j][c], 1e-4, "se rho");
        }
    }
    // Goodman (1974) reports G² = 2.72 on 6 df for this model.
    let g = &fit.pattern_fit;
    close(g.g2, 2.7199218717790608, 1e-6, "G2");
    close(g.chi2, 2.7197625727894885, 1e-6, "X2");
    assert!((g.df - 6.0).abs() < 1e-12);
    assert_eq!((g.complete_rows, g.observed_patterns), (216, 16));
    assert!(g.g2_p > 0.8 && g.g2_p < 0.9, "{}", g.g2_p);
    assert_eq!(fit.n_parameters(), 9);
    close(fit.bic(), 2.0 * 504.46767011842155 + 9.0 * 216f64.ln(), 1e-10, "BIC");
    close(fit.aic(), 2.0 * 504.46767011842155 + 18.0, 1e-10, "AIC");
    assert!(fit.starts_at_best >= 1 && fit.converged);
}

#[test]
#[allow(clippy::unreadable_literal)]
fn simulated_items_with_missing_responses() {
    let data = simulated();
    assert_eq!(data.len(), 500);
    assert!(data.iter().flatten().any(Option::is_none));
    let fit = fit_latent_classes(&data, &SIM_LEVELS, &options(3)).unwrap();
    close(fit.log_likelihood, -2394.2808081864355, 1e-10, "ll");
    let pi = [0.4670806175789418, 0.28540882734948203, 0.24751055507157627];
    let se_pi = [0.03256273919118651, 0.06493957754281394, 0.06522842488664449];
    let rho3 = [
        [0.678371395231524, 0.21077497781019935, 0.11085362695827661],
        [0.2042806863728229, 0.6103901004232798, 0.1853292132038973],
        [0.08785500901488413, 0.3153408074258332, 0.5968041835592828],
    ];
    let rho5 = [
        [0.46234198768076007, 0.3794570281418576, 0.12830146228827966, 0.029899521889102686],
        [0.09782919378018858, 0.20271333760803031, 0.4750690695927044, 0.2243883990190768],
        [0.03825054122389694, 0.1281068543153111, 0.30843193682448483, 0.5252106676363072],
    ];
    let se5 = [
        [0.037077023704358565, 0.03578987182591433, 0.028259153767352208, 0.015023157900714788],
        [0.04248659697005505, 0.04768737500500651, 0.05677473203904745, 0.051326707108001364],
        [0.023268565574853257, 0.04655692225147967, 0.06285977871100794, 0.07890600828565494],
    ];
    let se1 = [0.03075826862400587, 0.11675506270348197, 0.07467509195479469];
    for c in 0..3 {
        close(fit.class_sizes[c], pi[c], 1e-5, "pi");
        close(fit.class_size_se[c], se_pi[c], 1e-4, "se pi");
        close(fit.probability_se[1][c][0], se1[c], 1e-4, "se rho item 2");
        for r in 0..3 {
            close(fit.probabilities[3][c][r], rho3[c][r], 1e-5, "rho item 4");
        }
        for r in 0..4 {
            close(fit.probabilities[5][c][r], rho5[c][r], 1e-5, "rho item 6");
            close(fit.probability_se[5][c][r], se5[c][r], 1e-4, "se rho item 6");
        }
    }
    // Missing responses: only complete rows enter the pattern table.
    assert!(fit.pattern_fit.complete_rows < 500);
    // Posterior probabilities sum to one; entropy R² in (0, 1).
    assert!(fit.posterior.iter().all(|h| (h.iter().sum::<f64>() - 1.0).abs() < 1e-12));
    let e = fit.entropy_r2();
    assert!(e > 0.0 && e < 1.0);
    let avg = fit.average_posterior();
    assert!((0..3).all(|c| avg[c][c] > 0.5));
    // Prediction of the fitted rows reproduces the posterior.
    let again = fit.predict(&data[..5]).unwrap();
    for (a, b) in again.iter().zip(&fit.posterior) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-12);
        }
    }
}

#[test]
#[allow(clippy::unreadable_literal)]
fn selection_by_bic_matches_reference_log_likelihoods() {
    let data = simulated();
    let sel = select_latent_classes(&data, &SIM_LEVELS, &[1, 2, 3, 4], &options(1)).unwrap();
    let reference = [-2651.0285366160506, -2420.5655337900753, -2394.2808081864355, -2382.753066228838];
    let mut bics = Vec::new();
    for (c, r) in sel.candidates.iter().zip(reference) {
        let fit = c.fit.as_ref().unwrap();
        close(fit.log_likelihood, r, 1e-8, "ll");
        bics.push(-2.0 * r + fit.n_parameters() as f64 * 500f64.ln());
    }
    // With 500 respondents BIC prefers two classes (the third, generating
    // class is not separated enough to pay for its 11 parameters); AIC
    // prefers three.
    assert_eq!(sel.chosen().k(), 2);
    assert!(bics[1] < bics[2] && bics[1] < bics[0] && bics[1] < bics[3]);
    let aic = |c: &LatentClassCandidate| c.fit.as_ref().unwrap().aic();
    assert!(aic(&sel.candidates[2]) < aic(&sel.candidates[1]));
}

#[test]
fn one_class_is_the_independence_model() {
    let data = simulated();
    let fit = fit_latent_classes(&data, &SIM_LEVELS, &LatentClassOptions::new(1)).unwrap();
    // Marginal proportions of the answered responses, in closed form.
    for (j, &r) in SIM_LEVELS.iter().enumerate() {
        let answered: Vec<usize> = data.iter().filter_map(|row| row[j]).collect();
        for level in 0..r {
            let p = answered.iter().filter(|&&v| v == level).count() as f64 / answered.len() as f64;
            close(fit.probabilities[j][0][level], p, 1e-12, "marginal");
            let se = (p * (1.0 - p) / answered.len() as f64).sqrt();
            close(fit.probability_se[j][0][level], se, 1e-8, "binomial se");
        }
    }
    assert_eq!(fit.class_sizes, vec![1.0]);
    assert!((fit.entropy_r2() - 1.0).abs() < 1e-15);
}

#[test]
fn standard_errors_shrink_with_more_data() {
    let data = simulated();
    let fit = fit_latent_classes(&data, &SIM_LEVELS, &options(2)).unwrap();
    assert!(fit.class_size_se.iter().all(|s| s.is_finite() && *s > 0.0));
    assert!(fit.probability_se.iter().flatten().flatten().all(|s| s.is_finite() && *s > 0.0));
    let half: Vec<Vec<Option<usize>>> = data.iter().step_by(2).cloned().collect();
    let small = fit_latent_classes(&half, &SIM_LEVELS, &options(2)).unwrap();
    assert!(small.class_size_se[0] > fit.class_size_se[0]);
}

#[test]
fn smoothing_keeps_estimates_off_the_boundary() {
    // Two perfectly separated groups: maximum likelihood puts probabilities
    // at 0 and 1; smoothing pulls them inside.
    let mut data = vec![vec![Some(0), Some(0), Some(0)]; 30];
    data.extend(vec![vec![Some(1), Some(1), Some(1)]; 20]);
    data[0][1] = None;
    let ml = fit_latent_classes(&data, &[2; 3], &LatentClassOptions::new(2)).unwrap();
    close(ml.class_sizes[0], 0.6, 1e-8, "size");
    assert!(ml.probabilities[0][0][1] < 1e-8);
    assert!(ml.probability_se[0][0][1].is_nan(), "boundary estimates have no standard error");
    let smooth =
        fit_latent_classes(&data, &[2; 3], &LatentClassOptions { smoothing: 1.0, ..LatentClassOptions::new(2) })
            .unwrap();
    let p = smooth.probabilities[0][0][1];
    assert!(p > 0.01 && p < 0.1, "{p}");
    assert!(smooth.probability_se[0][0][1].is_finite());
    assert!(smooth.entropy_r2() > 0.95, "{}", smooth.entropy_r2());
}

#[test]
fn errors_and_identifiability() {
    let data = stouffer_toby();
    // Four binary items identify at most 15 parameters: 3 classes (14) fit, 4 (19) do not.
    assert!(fit_latent_classes(&data, &[2; 4], &LatentClassOptions::new(3)).is_ok());
    let err = fit_latent_classes(&data, &[2; 4], &LatentClassOptions::new(4)).unwrap_err().to_string();
    assert!(err.contains("19 parameters"), "{err}");
    assert!(fit_latent_classes(&data, &[2; 4], &LatentClassOptions::new(0)).is_err());
    assert!(fit_latent_classes(&data, &[2; 3], &LatentClassOptions::new(2)).is_err());
    assert!(fit_latent_classes(&data, &[2, 2, 2, 1], &LatentClassOptions::new(2)).is_err());
    let mut bad = data.clone();
    bad[0][0] = Some(2);
    assert!(fit_latent_classes(&bad, &[2; 4], &LatentClassOptions::new(2)).is_err());
    bad[0] = vec![None; 4];
    assert!(fit_latent_classes(&bad, &[2; 4], &LatentClassOptions::new(2)).is_err());
    assert!(
        fit_latent_classes(&data, &[2; 4], &LatentClassOptions { smoothing: -1.0, ..LatentClassOptions::new(2) })
            .is_err()
    );
    let sel = select_latent_classes(&data, &[2; 4], &[1, 2, 3, 4], &LatentClassOptions::new(1)).unwrap();
    assert!(sel.candidates[3].fit.is_err());
    assert_eq!(sel.chosen().k(), 2, "BIC prefers two classes for the Stouffer–Toby items");
    // Reproducible from the seed.
    let a = fit_latent_classes(&data, &[2; 4], &LatentClassOptions::new(2)).unwrap();
    let b = fit_latent_classes(&data, &[2; 4], &LatentClassOptions::new(2)).unwrap();
    assert_eq!(a, b);
}
