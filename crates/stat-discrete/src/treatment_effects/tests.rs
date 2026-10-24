use super::*;

/// A small confounded data set: one covariate raises both the chance of
/// treatment and the outcome; the true effect is 2.
fn data(n: usize) -> (Matrix, Vec<f64>, Vec<f64>) {
    let mut rng = Rng::new(7);
    let (mut x, mut t, mut y) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let c = rng.normal();
        let tr = f64::from(rng.next_f64() < logistic(0.8 * c));
        x.push(vec![c]);
        t.push(tr);
        y.push(1.0 + 3.0 * c + 2.0 * tr + 0.5 * rng.normal());
    }
    (Matrix::from_rows(&x).unwrap(), t, y)
}

fn names() -> Vec<String> {
    vec!["c".to_owned()]
}

#[test]
fn weighting_removes_confounding() {
    let (x, t, y) = data(600);
    let naive = {
        let m = |g: f64| {
            let v: Vec<f64> = (0..y.len()).filter(|&i| t[i] == g).map(|i| y[i]).collect();
            v.iter().sum::<f64>() / v.len() as f64
        };
        m(1.0) - m(0.0)
    };
    assert!(naive > 3.0, "confounded difference {naive}");
    for method in [
        EffectMethod::Weighting,
        EffectMethod::DoublyRobust,
        EffectMethod::Matching(MatchingOptions { replace: true, ..MatchingOptions::default() }),
    ] {
        let options = TreatmentEffectOptions {
            method,
            estimand: if matches!(method, EffectMethod::Matching(_)) { Estimand::Att } else { Estimand::Ate },
            ..TreatmentEffectOptions::new(OutcomeKind::Continuous)
        };
        let f = estimate_treatment_effect(&x, &names(), &t, &y, &options).unwrap();
        let d = &f.effects[0];
        assert!(d.interval.0 < 2.0 && 2.0 < d.interval.1, "{method:?}: {d:?}");
        assert!((d.estimate - 2.0).abs() < 0.4, "{method:?}: {}", d.estimate);
    }
}

#[test]
fn doubly_robust_is_exact_with_a_correct_outcome_model() {
    // Without noise the outcome models fit exactly, so the estimate is the
    // true effect whatever the weights.
    let (x, t, _) = data(200);
    let y: Vec<f64> = (0..t.len()).map(|i| 1.0 + 3.0 * x[(i, 0)] + 2.0 * t[i]).collect();
    for estimand in Estimand::ALL {
        let options = TreatmentEffectOptions {
            method: EffectMethod::DoublyRobust,
            estimand,
            ..TreatmentEffectOptions::new(OutcomeKind::Continuous)
        };
        let f = estimate_treatment_effect(&x, &names(), &t, &y, &options).unwrap();
        assert!((f.effects[0].estimate - 2.0).abs() < 1e-9, "{estimand:?}: {}", f.effects[0].estimate);
    }
}

#[test]
fn stabilized_weights_change_the_weights_not_the_estimate() {
    let (x, t, y) = data(300);
    let run = |stabilized| {
        let o = TreatmentEffectOptions { stabilized, ..TreatmentEffectOptions::new(OutcomeKind::Continuous) };
        estimate_treatment_effect(&x, &names(), &t, &y, &o).unwrap()
    };
    let (a, b) = (run(false), run(true));
    assert!((a.effects[0].estimate - b.effects[0].estimate).abs() < 1e-12);
    assert!((a.effects[0].std_error - b.effects[0].std_error).abs() < 1e-12);
    // Stabilized ATE weights sum to the group sizes in expectation; their
    // sum over all rows is close to n.
    let s: f64 = b.weights.iter().sum();
    assert!((s / t.len() as f64 - 1.0).abs() < 0.15, "{s}");
    assert!(a.weights.iter().sum::<f64>() > s);
}

#[test]
fn bootstrap_intervals_agree_with_the_sandwich() {
    let (x, t, y) = data(300);
    let base = TreatmentEffectOptions::new(OutcomeKind::Continuous);
    let s = estimate_treatment_effect(&x, &names(), &t, &y, &base).unwrap();
    let b = estimate_treatment_effect(
        &x,
        &names(),
        &t,
        &y,
        &TreatmentEffectOptions { intervals: IntervalMethod::Bootstrap { replicates: 400, seed: 11 }, ..base },
    )
    .unwrap();
    assert_eq!(s.effects[0].estimate, b.effects[0].estimate);
    let ratio = b.effects[0].std_error / s.effects[0].std_error;
    assert!((0.8..1.25).contains(&ratio), "bootstrap / sandwich SE = {ratio}");
    assert_eq!(b.failed_replicates, 0);
}

#[test]
fn matching_without_replacement_uses_each_control_once() {
    let (x, t, y) = data(300);
    let o = TreatmentEffectOptions {
        method: EffectMethod::Matching(MatchingOptions { ratio: 2, replace: false, caliper: None }),
        estimand: Estimand::Att,
        ..TreatmentEffectOptions::new(OutcomeKind::Continuous)
    };
    let f = estimate_treatment_effect(&x, &names(), &t, &y, &o).unwrap();
    let sets: Vec<usize> = f.matched_set.iter().flatten().copied().collect();
    let n_sets = sets.iter().max().copied().unwrap();
    for s in 1..=n_sets {
        let members: Vec<usize> = (0..t.len()).filter(|&i| f.matched_set[i] == Some(s)).collect();
        assert_eq!(members.iter().filter(|&&i| t[i] == 1.0).count(), 1, "one treated row per set");
        assert!((1..=2).contains(&members.iter().filter(|&&i| t[i] == 0.0).count()));
    }
    // Control weights sum to the number of matched controls.
    let controls: Vec<usize> = (0..t.len()).filter(|&i| t[i] == 0.0 && f.weights[i] > 0.0).collect();
    let s: f64 = controls.iter().map(|&i| f.weights[i]).sum();
    assert!((s - controls.len() as f64).abs() < 1e-9);
}

#[test]
fn trimming_leaves_out_extreme_scores() {
    let (x, t, y) = data(300);
    let o = TreatmentEffectOptions { trim: 0.1, ..TreatmentEffectOptions::new(OutcomeKind::Continuous) };
    let f = estimate_treatment_effect(&x, &names(), &t, &y, &o).unwrap();
    let expected = f.propensity.scores.iter().filter(|&&e| !(0.1..=0.9).contains(&e)).count();
    assert_eq!(f.n_trimmed, expected);
    assert!(expected > 0);
    assert!((0..t.len()).all(|i| f.used[i] == (0.1..=0.9).contains(&f.propensity.scores[i])));
    assert!((0..t.len()).all(|i| f.used[i] || f.weights[i] == 0.0));
}

#[test]
fn invalid_inputs_are_errors() {
    let (x, t, y) = data(50);
    let o = TreatmentEffectOptions::new(OutcomeKind::Continuous);
    let mut bad = t.clone();
    bad[0] = 2.0;
    assert!(estimate_treatment_effect(&x, &names(), &bad, &y, &o).is_err());
    let binary = TreatmentEffectOptions::new(OutcomeKind::Binary);
    assert!(estimate_treatment_effect(&x, &names(), &t, &y, &binary).is_err(), "outcome not 0/1");
    let ate_without = TreatmentEffectOptions {
        method: EffectMethod::Matching(MatchingOptions { replace: false, ..MatchingOptions::default() }),
        ..o.clone()
    };
    assert!(estimate_treatment_effect(&x, &names(), &t, &y, &ate_without).is_err());
    let one: Vec<f64> = (0..t.len()).map(|i| f64::from(i == 0)).collect();
    assert!(estimate_treatment_effect(&x, &names(), &one, &y, &o).is_err(), "one treated row");
    assert!(estimate_treatment_effect(&x, &names(), &t, &y, &TreatmentEffectOptions { trim: 0.4, ..o }).is_err());
}

#[test]
fn heterogeneity_q_by_hand() {
    // Two estimates 1 ± 1 and 3 ± 1: mean 2, Q = 1 + 1 = 2 on 1 df.
    let (q, df, p) = heterogeneity_test(&[(1.0, 1.0), (3.0, 1.0)]).unwrap();
    assert!((q - 2.0).abs() < 1e-12);
    assert_eq!(df, 1);
    // P(χ²₁ > 2) = erfc(1).
    assert!((p - 0.157_299_207_050_285_1).abs() < 1e-12, "{p}");
    assert!(heterogeneity_test(&[(1.0, 1.0)]).is_none());
}
