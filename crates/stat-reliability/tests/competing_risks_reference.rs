//! Competing-risks routines compared with independent reference values
//! (`tests/data/competing_risks_reference.py`, which writes
//! `competing_risks_reference.json`): lifelines' Aalen–Johansen estimator,
//! independent NumPy implementations of Gray's test and of Fine–Gray
//! regression (cross-checked against lifelines' Cox regression on the
//! subdistribution risk set when nothing is censored), and a SciPy fit of a
//! Weibull per failure mode combined into a series system. The datasets are
//! simulated (seeded) for the tests.

use stat_core::distribution_fit::FitFamily;
use stat_data::{JsonValue, parse_json};
use stat_reliability::competing_risks::{
    CompetingObservation, SeriesSystem, aalen_johansen, fine_gray, fit_failure_modes, gray_test,
};

fn cases() -> JsonValue {
    parse_json(include_str!("data/competing_risks_reference.json")).unwrap()
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn close(got: f64, want: f64, rel: f64, what: &str) {
    let scale = want.abs().max(1e-12);
    assert!(
        (got - want).abs() <= rel * scale,
        "{what}: got {got}, reference {want} (relative error {:e})",
        (got - want).abs() / scale
    );
}

/// Observations and groups of a case.
fn observations(case: &JsonValue) -> (Vec<CompetingObservation>, Vec<usize>) {
    let w = nums(case.get("weights").unwrap());
    let mut obs = Vec::new();
    let mut groups = Vec::new();
    for (r, &weight) in case.get("rows").unwrap().as_array().unwrap().iter().zip(&w) {
        let r = r.as_array().unwrap();
        let cause = num(&r[1]);
        obs.push(CompetingObservation { time: num(&r[0]), cause: (cause >= 0.0).then_some(cause as usize), weight });
        groups.push(num(&r[2]) as usize);
    }
    (obs, groups)
}

#[test]
fn aalen_johansen_matches_lifelines() {
    let all = cases();
    for name in ["aj", "aj_weighted"] {
        let case = all.get(name).unwrap();
        let (obs, _) = observations(case);
        let k = num(case.get("n_causes").unwrap()) as usize;
        let aj = aalen_johansen(&obs, k).unwrap();
        let res = case.get("result").unwrap();
        let times = nums(res.get("times").unwrap());
        let rows =
            |key: &str| -> Vec<Vec<f64>> { res.get(key).unwrap().as_array().unwrap().iter().map(nums).collect() };
        let (inc, var, lo, hi) = (rows("incidence"), rows("variance"), rows("lower"), rows("upper"));
        assert_eq!(aj.steps.len(), times.len(), "{name}");
        for (s, step) in aj.steps.iter().enumerate() {
            close(step.time, times[s], 1e-15, "time");
            for j in 0..k {
                let what = format!("{name} cause {j} at {}", step.time);
                close(step.incidence[j], inc[s][j], 1e-12, &format!("{what} incidence"));
                if inc[s][j] > 0.0 {
                    close(step.variance[j], var[s][j], 1e-9, &format!("{what} variance"));
                    let (l, u) = aj.limits(j, s, 0.95).unwrap();
                    close(l, lo[s][j], 1e-9, &format!("{what} lower"));
                    close(u, hi[s][j], 1e-9, &format!("{what} upper"));
                }
            }
        }
    }
}

#[test]
fn gray_test_matches_independent_implementation() {
    let all = cases();
    for (name, n_groups) in [("gray2", 2), ("gray3", 3)] {
        let case = all.get(name).unwrap();
        let (obs, groups) = observations(case);
        for (cause, want) in case.get("tests").unwrap().as_array().unwrap().iter().enumerate() {
            let g = gray_test(&obs, &groups, n_groups, cause).unwrap();
            let what = format!("{name} cause {cause}");
            for (a, b) in g.scores.iter().zip(nums(want.get("scores").unwrap())) {
                assert!((a - b).abs() < 1e-10 * (1.0 + b.abs()), "{what} score {a} vs {b}");
            }
            let cov: Vec<Vec<f64>> = want.get("covariance").unwrap().as_array().unwrap().iter().map(nums).collect();
            for (ra, rb) in g.covariance.iter().zip(&cov) {
                for (a, b) in ra.iter().zip(rb) {
                    close(*a, *b, 1e-8, &format!("{what} covariance"));
                }
            }
            close(g.statistic, num(want.get("statistic").unwrap()), 1e-8, &format!("{what} statistic"));
            close(g.p_value, num(want.get("p_value").unwrap()), 1e-7, &format!("{what} p-value"));
            assert_eq!(g.df, num(want.get("df").unwrap()));
        }
    }
    // The simulated difference in cause 0 between the two groups is detected.
    let case = all.get("gray2").unwrap();
    let (obs, groups) = observations(case);
    assert!(gray_test(&obs, &groups, 2, 0).unwrap().p_value < 0.05);
}

fn covariates(case: &JsonValue) -> Vec<Vec<f64>> {
    case.get("x").unwrap().as_array().unwrap().iter().map(nums).collect()
}

#[test]
fn fine_gray_matches_independent_implementation() {
    let all = cases();
    for name in ["fine_gray", "fine_gray_uncensored"] {
        let case = all.get(name).unwrap();
        let (obs, _) = observations(case);
        let x = covariates(case);
        let fit = fine_gray(&obs, &x, 0).unwrap();
        let res = case.get("result").unwrap();
        for (i, b) in nums(res.get("beta").unwrap()).into_iter().enumerate() {
            close(fit.coefficients[i], b, 1e-8, &format!("{name} β{i}"));
        }
        for (i, se) in nums(res.get("robust_se").unwrap()).into_iter().enumerate() {
            close(fit.std_errors()[i], se, 1e-8, &format!("{name} robust SE {i}"));
        }
        for (i, se) in nums(res.get("naive_se").unwrap()).into_iter().enumerate() {
            close(fit.naive_covariance[i][i].sqrt(), se, 1e-8, &format!("{name} model SE {i}"));
        }
        let base = res.get("baseline").unwrap().as_array().unwrap();
        assert_eq!(fit.baseline.len(), base.len());
        for (got, want) in fit.baseline.iter().zip(base) {
            let want = nums(want);
            close(got[0], want[0], 1e-15, "baseline time");
            close(got[1], want[1], 1e-8, &format!("{name} baseline"));
        }
    }
    // Uncensored: Cox regression on the subdistribution risk set (lifelines,
    // Lin–Wei robust standard errors).
    let case = all.get("fine_gray_uncensored").unwrap();
    let (obs, _) = observations(case);
    let fit = fine_gray(&obs, &covariates(case), 0).unwrap();
    let cox = case.get("cox").unwrap();
    close(fit.coefficients[0], num(&cox.get("beta").unwrap().as_array().unwrap()[0]), 1e-7, "Cox β");
    close(fit.std_errors()[0], num(&cox.get("robust_se").unwrap().as_array().unwrap()[0]), 1e-7, "Cox robust SE");
}

#[test]
fn series_system_matches_independent_fits() {
    let all = cases();
    let case = all.get("series").unwrap();
    let (obs, _) = observations(case);
    let res = case.get("result").unwrap();
    let modes: Vec<_> = fit_failure_modes(&obs, 2, FitFamily::Weibull).into_iter().map(Result::unwrap).collect();
    for (m, want) in modes.iter().zip(res.get("modes").unwrap().as_array().unwrap()) {
        close(m.distribution.scale.recip(), num(want.get("shape").unwrap()), 1e-6, "mode shape");
        close(m.distribution.location.exp(), num(want.get("scale").unwrap()), 1e-6, "mode scale");
    }
    let sys = SeriesSystem { modes };
    let times = nums(res.get("times").unwrap());
    let f = nums(res.get("system").unwrap());
    let se = nums(res.get("system_se").unwrap());
    for (i, &t) in times.iter().enumerate() {
        let p = sys.probability_interval(t, 0.95).unwrap();
        close(p.estimate, f[i], 1e-6, &format!("F_sys({t})"));
        close(p.std_error, se[i], 2e-4, &format!("SE F_sys({t})"));
    }
}
