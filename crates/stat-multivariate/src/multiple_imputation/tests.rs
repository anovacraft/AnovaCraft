#![allow(clippy::excessive_precision)] // reference values are printed to 17 digits
use super::*;
use stat_core::distributions::{ContinuousDistribution, StudentsT};

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
}

#[test]
fn regression_draw_matches_mice_given_its_random_numbers() {
    // mice 3.16 `estimice` + `.norm.draw` with rchisq → 3.7 and rnorm → z
    // (script: crates/app-model/tests/data/mice_reference.R).
    let xs = [1.2, 2.5, 3.1, 4.8, 5.0, 6.7, 7.1, 8.4];
    let d = [0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0];
    let x: Vec<Vec<f64>> = xs.iter().zip(d).map(|(&a, b)| vec![1.0, a, b]).collect();
    let y = [2.3, 4.1, 4.9, 8.2, 7.7, 9.9, 11.8, 12.1];
    let draw = bayesian_regression_draw(&x, &y, 0.0, 3.7, &[0.4, -1.1, 0.25]).unwrap();
    for (a, b) in draw.coefficients.iter().zip([0.31473733147373584, 1.440260344026034, 0.64999999999999947]) {
        close(*a, b, 1e-12);
    }
    for (a, b) in draw.drawn.iter().zip([0.50866251692798647, 1.361798790590218, 0.90450476527435053]) {
        close(*a, b, 1e-12);
    }
    close(draw.sigma, 0.54313133251915247, 1e-12);
}

#[test]
fn donors_are_the_nearest_predicted_means() {
    let fitted = [1.0, 2.0, 2.4, 3.9, 5.5, 2.1];
    assert_eq!(pmm_donors(&fitted, 2.15, 3), vec![5, 1, 2]);
    assert_eq!(pmm_donors(&fitted, 10.0, 2), vec![4, 3]);
    // Equal distances go to the earlier row.
    assert_eq!(pmm_donors(&[1.0, 3.0], 2.0, 1), vec![0]);
    let mut rng = Rng::new(5);
    let values: Vec<f64> = (0..300).map(|_| rng.normal()).collect();
    let pool = DonorPool::new(&values);
    for _ in 0..200 {
        let t = 1.5 * rng.normal();
        for k in [1, 5, 12] {
            assert_eq!(pool.donors(t, k), pmm_donors(&values, t, k));
        }
    }
    assert_eq!(pool.donors(0.0, 1000).len(), 300);
}

/// `n` rows of (x, y, group) with y = 1 + x + e, group a two-level
/// category tied to x; x missing completely at random, y missing more often
/// for large x (missing at random given x).
fn simulated(n: usize, rng: &mut Rng) -> Vec<Vec<f64>> {
    simulated_with(n, rng, true)
}

/// As [`simulated`]; with `mar` false, y is missing completely at random
/// (30% of rows) instead.
fn simulated_with(n: usize, rng: &mut Rng, mar: bool) -> Vec<Vec<f64>> {
    (0..n)
        .map(|_| {
            let x = rng.normal();
            let y = 1.0 + x + rng.normal();
            let g = f64::from(u8::from(x + 0.7 * rng.normal() > 0.0));
            let p_y = if mar { 0.6 / (1.0 + (-(x - 0.5)).exp()) } else { 0.3 };
            let row_x = if rng.next_f64() < 0.15 { f64::NAN } else { x };
            let row_y = if rng.next_f64() < p_y { f64::NAN } else { y };
            let row_g = if rng.next_f64() < 0.1 { f64::NAN } else { g };
            vec![row_x, row_y, row_g]
        })
        .collect()
}

const KINDS: [ColumnKind; 3] = [ColumnKind::Continuous, ColumnKind::Continuous, ColumnKind::Categorical { levels: 2 }];

#[test]
fn chained_equations_keep_observed_values_and_are_reproducible() {
    let mut rng = Rng::new(11);
    let data = simulated(120, &mut rng);
    let mut options = FcsOptions::new(&data, &KINDS);
    options.m = 4;
    options.iterations = 5;
    let a = chained_equations(&data, &KINDS, &options).unwrap();
    let b = chained_equations(&data, &KINDS, &options).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        options.methods,
        [FcsMethod::PredictiveMeanMatching, FcsMethod::PredictiveMeanMatching, FcsMethod::Logistic]
    );
    assert_eq!(a.imputations.len(), 4);
    assert_eq!(a.chains.means[0].len(), 6);
    let observed_y: Vec<f64> = data.iter().map(|r| r[1]).filter(|v| !v.is_nan()).collect();
    for imp in &a.imputations {
        for (orig, done) in data.iter().zip(imp) {
            for j in 0..3 {
                assert!(!done[j].is_nan());
                if !orig[j].is_nan() {
                    assert_eq!(orig[j], done[j]);
                }
            }
            // Matched values are observed values; categories are level codes.
            assert!(observed_y.contains(&done[1]));
            assert!(done[2] == 0.0 || done[2] == 1.0);
        }
    }
    assert_ne!(a.imputations[0], a.imputations[1], "chains differ");
    options.seed = 2;
    assert_ne!(chained_equations(&data, &KINDS, &options).unwrap().imputations[0], a.imputations[0]);
}

#[test]
fn multinomial_and_bayesian_regression_draws() {
    let mut rng = Rng::new(3);
    let data: Vec<Vec<f64>> = (0..150)
        .map(|_| {
            let x = rng.normal();
            let c = if x < -0.5 {
                0.0
            } else if x < 0.6 {
                1.0
            } else {
                2.0
            };
            let y = 3.0 - x + 0.5 * rng.normal();
            let miss = |rng: &mut Rng, v: f64| if rng.next_f64() < 0.2 { f64::NAN } else { v };
            vec![x, miss(&mut rng, c), miss(&mut rng, y)]
        })
        .collect();
    let kinds = [ColumnKind::Continuous, ColumnKind::Categorical { levels: 3 }, ColumnKind::Continuous];
    let mut options = FcsOptions::new(&data, &kinds);
    assert_eq!(options.methods, [FcsMethod::None, FcsMethod::Multinomial, FcsMethod::PredictiveMeanMatching]);
    options.methods[2] = FcsMethod::BayesianRegression;
    options.m = 3;
    let mi = chained_equations(&data, &kinds, &options).unwrap();
    // The category follows x closely, so most imputed levels match the rule.
    let (mut right, mut total) = (0, 0);
    for imp in &mi.imputations {
        for (orig, done) in data.iter().zip(imp) {
            if orig[1].is_nan() {
                total += 1;
                let rule = if orig[0] < -0.5 {
                    0.0
                } else if orig[0] < 0.6 {
                    1.0
                } else {
                    2.0
                };
                right += usize::from(done[1] == rule);
                assert!([0.0, 1.0, 2.0].contains(&done[1]));
            }
        }
    }
    assert!(right as f64 > 0.75 * total as f64, "{right} of {total}");
    assert!(!mi.imputed[0] && mi.imputed[1] && mi.imputed[2]);
    assert!(mi.chains.means[0][3][0].is_nan());
}

#[test]
fn bad_settings_are_refused() {
    let data = vec![vec![1.0, f64::NAN], vec![2.0, 1.0], vec![3.0, 0.0]];
    let kinds = [ColumnKind::Continuous, ColumnKind::Categorical { levels: 2 }];
    let mut options = FcsOptions::new(&data, &kinds);
    options.methods[1] = FcsMethod::PredictiveMeanMatching;
    assert!(chained_equations(&data, &kinds, &options).is_err(), "method must suit the column");
    let bad = vec![vec![1.0, 3.0], vec![2.0, f64::NAN]];
    assert!(chained_equations(&bad, &kinds, &FcsOptions::new(&bad, &kinds)).is_err(), "level code out of range");
    let mut zero = FcsOptions::new(&data, &kinds);
    zero.m = 0;
    assert!(chained_equations(&data, &kinds, &zero).is_err());
    let none = vec![vec![f64::NAN], vec![f64::NAN]];
    let kinds1 = [ColumnKind::Continuous];
    assert!(chained_equations(&none, &kinds1, &FcsOptions::new(&none, &kinds1)).is_err());
}

#[test]
fn data_augmentation_draws_around_the_em_fit() {
    let mut rng = Rng::new(21);
    let data: Vec<Vec<f64>> = simulated(150, &mut rng).into_iter().map(|r| r[..2].to_vec()).collect();
    let options = DataAugmentationOptions { m: 5, iterations: 20, seed: 4 };
    let a = normal_data_augmentation(&data, &options).unwrap();
    assert_eq!(a, normal_data_augmentation(&data, &options).unwrap());
    assert_eq!(a.chains.means[0].len(), 21);
    for imp in &a.imputations {
        for (orig, done) in data.iter().zip(imp) {
            for j in 0..2 {
                if !orig[j].is_nan() {
                    assert_eq!(orig[j], done[j]);
                }
            }
        }
    }
    assert!(normal_data_augmentation(&data[..2], &options).is_err(), "too few rows");
}

/// Rubin's 95% interval from per-imputation estimates and variances.
fn rubin_covers(q: &[f64], u: &[f64], df_complete: f64, truth: f64) -> bool {
    let m = q.len() as f64;
    let qbar = q.iter().sum::<f64>() / m;
    let ubar = u.iter().sum::<f64>() / m;
    let b = q.iter().map(|v| (v - qbar).powi(2)).sum::<f64>() / (m - 1.0);
    let t = ubar + (1.0 + 1.0 / m) * b;
    let lambda = ((1.0 + 1.0 / m) * b / t).max(1e-4);
    let old = (m - 1.0) / (lambda * lambda);
    let obs = (df_complete + 1.0) / (df_complete + 3.0) * df_complete * (1.0 - lambda);
    let df = old * obs / (old + obs);
    let half = StudentsT::new(df).unwrap().quantile(0.975).unwrap() * t.sqrt();
    (qbar - truth).abs() <= half
}

/// Mean of y (with its variance) and the slope of y on x (with its
/// variance) in a completed data set.
fn estimates(d: &[Vec<f64>]) -> [(f64, f64); 2] {
    let n = d.len() as f64;
    let (mx, my) = (d.iter().map(|r| r[0]).sum::<f64>() / n, d.iter().map(|r| r[1]).sum::<f64>() / n);
    let sxx: f64 = d.iter().map(|r| (r[0] - mx).powi(2)).sum();
    let sxy: f64 = d.iter().map(|r| (r[0] - mx) * (r[1] - my)).sum();
    let syy: f64 = d.iter().map(|r| (r[1] - my).powi(2)).sum();
    let slope = sxy / sxx;
    let rss = syy - slope * sxy;
    [(my, syy / (n - 1.0) / n), (slope, rss / (n - 2.0) / sxx)]
}

/// Share of `reps` simulated data sets whose pooled 95% intervals cover the
/// mean of y (1) and the slope (1), imputing with `impute`.
fn coverage(reps: usize, mar: bool, impute: impl Fn(&[Vec<f64>], u64) -> MultipleImputation + Sync) -> [f64; 2] {
    let n = 200;
    let hits: Vec<[bool; 2]> = (0..reps)
        .into_par_iter()
        .map(|rep| {
            let mut rng = Rng::new(10_000 + rep as u64);
            let data = simulated_with(n, &mut rng, mar);
            let mi = impute(&data, rep as u64);
            let per: Vec<[(f64, f64); 2]> = mi.imputations.iter().map(|d| estimates(d)).collect();
            let covers = |k: usize, truth: f64, dfc: f64| {
                let q: Vec<f64> = per.iter().map(|e| e[k].0).collect();
                let u: Vec<f64> = per.iter().map(|e| e[k].1).collect();
                rubin_covers(&q, &u, dfc, truth)
            };
            [covers(0, 1.0, n as f64 - 1.0), covers(1, 1.0, n as f64 - 2.0)]
        })
        .collect();
    [0, 1].map(|k| hits.iter().filter(|h| h[k]).count() as f64 / reps as f64)
}

/// Pooled 95% intervals from each engine (predictive mean matching,
/// Bayesian linear regression, normal data augmentation) cover the mean of
/// y and the slope in 93–97% of 500 replicates, with y missing completely
/// at random and missing at random given x. Complete-case analysis is
/// biased under the second (y is missing more often for large x).
///
/// With a much stronger relationship (slope 2, R² = 0.8) matching
/// attenuates the slope by about 2% because the largest predicted means
/// have few donors near them; R mice 3.16 shows the same (mean pooled
/// slope 1.952 over 200 replicates of that design, ours 1.965 over 1000).
#[test]
fn pooled_intervals_have_nominal_coverage() {
    let reps = 500;
    let fcs = |kinds: [ColumnKind; 3], method: FcsMethod| {
        move |data: &[Vec<f64>], seed: u64| {
            let mut o = FcsOptions::new(data, &kinds);
            o.m = 10;
            o.iterations = 8;
            o.seed = seed;
            o.methods[0] = method;
            o.methods[1] = method;
            chained_equations(data, &kinds, &o).unwrap()
        }
    };
    let da = |data: &[Vec<f64>], seed| {
        let d: Vec<Vec<f64>> = data.iter().map(|r| r[..2].to_vec()).collect();
        normal_data_augmentation(&d, &DataAugmentationOptions { m: 10, iterations: 20, seed }).unwrap()
    };
    for mar in [false, true] {
        let pmm = coverage(reps, mar, fcs(KINDS, FcsMethod::PredictiveMeanMatching));
        let norm = coverage(reps, mar, fcs(KINDS, FcsMethod::BayesianRegression));
        let da = coverage(reps, mar, da);
        for (name, c) in [("pmm", pmm), ("norm", norm), ("da", da)] {
            for (k, v) in c.iter().enumerate() {
                assert!((0.93..=0.97).contains(v), "{name}, MAR {mar}, estimand {k}: coverage {v}");
            }
        }
    }
    // Complete cases: the observed y average well below the true mean of 1.
    let cc: f64 = (0..50)
        .map(|rep| {
            let mut rng = Rng::new(10_000 + rep);
            let y: Vec<f64> = simulated(200, &mut rng).iter().map(|r| r[1]).filter(|v| !v.is_nan()).collect();
            y.iter().sum::<f64>() / y.len() as f64
        })
        .sum::<f64>()
        / 50.0;
    assert!(cc < 0.9, "{cc}");
}
