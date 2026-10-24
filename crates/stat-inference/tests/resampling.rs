//! Bootstrap intervals and permutation tests against SciPy 1.18
//! (`tests/data/resampling_reference.py`):
//!
//! * BCa and percentile intervals computed from the *same* bootstrap
//!   replicates SciPy was given (stored in the JSON), so the interval
//!   formulas are compared exactly; the jackknife acceleration is recomputed
//!   here from the data.
//! * Exact permutation p-values against `scipy.stats.permutation_test` with
//!   full enumeration, and a Monte Carlo p-value against the exact one.

use stat_data::{JsonValue, parse_json};
use stat_inference::Alternative;
use stat_inference::bootstrap::{acceleration, jackknife_map, summarize};
use stat_inference::permutation::{
    CorrelationStatistic, GroupStatistic, PermutationOptions, correlation, paired, two_groups,
};

fn reference() -> JsonValue {
    parse_json(include_str!("data/resampling_reference.json")).unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn num(v: &JsonValue, key: &str) -> f64 {
    v.get(key).unwrap().as_f64().unwrap()
}

fn close(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol * b.abs().max(1.0), "{what}: {a} vs {b}");
}

#[test]
fn bca_and_percentile_intervals_match_scipy_on_identical_replicates() {
    let r = reference();
    for case in r.get("bca").unwrap().as_array().unwrap() {
        let name = case.get("name").unwrap().as_str().unwrap();
        let samples: Vec<Vec<f64>> = case.get("samples").unwrap().as_array().unwrap().iter().map(nums).collect();
        let pooled: Vec<f64> = samples.concat();
        let strata: Vec<usize> = samples.iter().enumerate().flat_map(|(h, s)| vec![h; s.len()]).collect();
        let mean = |idx: &[usize], h: usize| {
            let v: Vec<f64> = idx.iter().filter(|&&i| strata[i] == h).map(|&i| pooled[i]).collect();
            v.iter().sum::<f64>() / v.len() as f64
        };
        let statistic = |idx: &[usize]| -> f64 {
            match name {
                "mean" => mean(idx, 0),
                "std-dev" => {
                    let m = mean(idx, 0);
                    let n = idx.len() as f64;
                    (idx.iter().map(|&i| (pooled[i] - m).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
                }
                _ => mean(idx, 0) - mean(idx, 1),
            }
        };
        let all: Vec<usize> = (0..pooled.len()).collect();
        let estimate = statistic(&all);
        close(estimate, num(case, "estimate"), 1e-12, name);
        let jack = jackknife_map(pooled.len(), statistic).unwrap();
        let strata_arg = (samples.len() > 1).then_some(strata.as_slice());
        let a = acceleration(&jack, strata_arg).unwrap().unwrap();
        close(a, num(case, "acceleration"), 1e-10, name);
        let reps = nums(case.get("replicates").unwrap());
        let level = num(case, "level");
        let s = summarize(estimate, &reps, Some(&jack), strata_arg, level).unwrap();
        let bca = s.bca.unwrap();
        close(bca.bias_correction, num(case, "z0"), 1e-10, name);
        close(bca.lower_percentile, num(case, "alpha_lower"), 1e-10, name);
        close(bca.upper_percentile, num(case, "alpha_upper"), 1e-10, name);
        let want = nums(case.get("bca").unwrap());
        close(bca.lower, want[0], 1e-10, name);
        close(bca.upper, want[1], 1e-10, name);
        let want = nums(case.get("percentile").unwrap());
        close(s.percentile.0, want[0], 1e-12, name);
        close(s.percentile.1, want[1], 1e-12, name);
        close(s.mean, num(case, "mean"), 1e-12, name);
        close(s.std_error, num(case, "std_error"), 1e-10, name);
        assert_eq!((s.replicates, s.failed), (reps.len(), 0));
    }
}

#[test]
fn exact_permutation_p_values_match_scipy() {
    let r = reference();
    let alternatives =
        [("two-sided", Alternative::TwoSided), ("less", Alternative::Less), ("greater", Alternative::Greater)];
    let mut checked = 0;
    for case in r.get("permutation").unwrap().as_array().unwrap() {
        let name = case.get("name").unwrap().as_str().unwrap();
        if name == "monte-carlo-means" {
            continue;
        }
        let data: Vec<Vec<f64>> = case.get("data").unwrap().as_array().unwrap().iter().map(nums).collect();
        for (key, alternative) in alternatives {
            let o = PermutationOptions { alternative, ..PermutationOptions::default() };
            let t = match case.get("kind").unwrap().as_str().unwrap() {
                "two-groups-mean" => two_groups(&data[0], &data[1], GroupStatistic::MeanDifference, &o),
                "two-groups-median" => two_groups(&data[0], &data[1], GroupStatistic::MedianDifference, &o),
                "paired" => {
                    let d: Vec<f64> = data[0].iter().zip(&data[1]).map(|(a, b)| a - b).collect();
                    paired(&d, &o)
                }
                "pearson" => correlation(&data[0], &data[1], CorrelationStatistic::Pearson, &o),
                _ => {
                    // Spearman from the raw values (ranked inside).
                    let x = nums(case.get("x").unwrap());
                    let y = nums(case.get("y").unwrap());
                    correlation(&x, &y, CorrelationStatistic::Spearman, &o)
                }
            }
            .unwrap();
            assert!(t.exact, "{name}");
            close(t.observed, num(case, "observed"), 1e-12, name);
            close(t.p_value, num(case, key), 1e-12, &format!("{name} {key}"));
            checked += 1;
        }
    }
    assert_eq!(checked, 15);
}

#[test]
fn monte_carlo_p_value_agrees_with_full_enumeration() {
    let r = reference();
    let case = r
        .get("permutation")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c.get("name").unwrap().as_str() == Some("monte-carlo-means"));
    let case = case.unwrap();
    let data: Vec<Vec<f64>> = case.get("data").unwrap().as_array().unwrap().iter().map(nums).collect();
    let exact = num(case, "two-sided");
    // 184,756 splits exceed the default exact limit, so B random splits are drawn.
    let b = 40_000;
    let o = PermutationOptions { resamples: b, seed: 2026, ..PermutationOptions::default() };
    let t = two_groups(&data[0], &data[1], GroupStatistic::MeanDifference, &o).unwrap();
    assert!(!t.exact && t.evaluated() == b);
    assert_eq!(t.arrangements, 184_756.0);
    // Two-sided p is twice a tail estimated from B draws: SE ≈ 2·√(p/2 (1 − p/2)/B).
    let se = 2.0 * ((exact / 2.0) * (1.0 - exact / 2.0) / b as f64).sqrt();
    assert!((t.p_value - exact).abs() < 4.0 * se, "{} vs exact {exact} (SE {se})", t.p_value);
    // Raising the limit enumerates every split and reproduces SciPy exactly.
    let all = PermutationOptions { exact_limit: 200_000, ..o };
    let e = two_groups(&data[0], &data[1], GroupStatistic::MeanDifference, &all).unwrap();
    assert!(e.exact);
    close(e.p_value, exact, 1e-12, "full enumeration");
}
