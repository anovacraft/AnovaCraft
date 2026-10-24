//! Ordinal and nominal logistic regression compared with statsmodels 0.15
//! (`OrderedModel` with the logit distribution, `MNLogit`) and with an
//! independent NumPy implementation of the parallel-lines score test; see
//! `tests/data/polytomous_reference.py`, which generated the data and the
//! reference values in `tests/data/polytomous_reference.json`.

use stat_core::Matrix;
use stat_data::{JsonValue, parse_json};
use stat_discrete::{PolytomousOptions, fit_nominal_logistic, fit_ordinal_logistic};

fn cases() -> JsonValue {
    parse_json(include_str!("data/polytomous_reference.json")).unwrap()
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn rows(v: &JsonValue) -> Matrix {
    let r: Vec<Vec<f64>> = v.as_array().unwrap().iter().map(nums).collect();
    Matrix::from_rows(&r).unwrap()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1e-3)
}

fn keep_columns(x: &Matrix, drop: &[usize]) -> Matrix {
    let cols: Vec<&[f64]> = (0..x.ncols()).filter(|j| !drop.contains(j)).map(|j| x.col(j)).collect();
    if cols.is_empty() { Matrix::zeros(x.nrows(), 0) } else { Matrix::from_columns(&cols).unwrap() }
}

struct Case {
    x: Matrix,
    y: Vec<usize>,
    levels: Vec<String>,
    names: Vec<String>,
}

fn case(c: &JsonValue) -> Case {
    let j = c.get("levels").unwrap().as_usize().unwrap();
    Case {
        x: rows(c.get("x").unwrap()),
        y: nums(c.get("y").unwrap()).into_iter().map(|v| v as usize).collect(),
        levels: (0..j).map(|k| format!("L{k}")).collect(),
        names: c.get("names").unwrap().as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect(),
    }
}

#[test]
fn proportional_odds_matches_statsmodels() {
    for c in cases().get("ordinal").unwrap().as_array().unwrap() {
        let name = c.get("name").unwrap().as_str().unwrap();
        let d = case(c);
        let options = PolytomousOptions { names: d.names.clone(), ..PolytomousOptions::default() };
        let fit = fit_ordinal_logistic(&d.x, &d.y, &d.levels, &options).unwrap();
        let r = c.get("fit").unwrap();
        assert!(fit.statistics.converged, "{name}");
        let ll = r.get("llf").unwrap().as_f64().unwrap();
        assert!(close(fit.statistics.log_likelihood, ll, 1e-9), "{name}: ℓ {} vs {ll}", fit.statistics.log_likelihood);
        let est: Vec<f64> = fit.thresholds.iter().chain(&fit.slopes).map(|c| c.estimate).collect();
        let want: Vec<f64> =
            nums(r.get("thresholds").unwrap()).into_iter().chain(nums(r.get("slopes").unwrap())).collect();
        for (k, (g, w)) in est.iter().zip(&want).enumerate() {
            assert!(close(*g, *w, 1e-6), "{name} θ{k}: {g} vs {w}");
        }
        // Standard errors: statsmodels (numerical Hessian, delta method for
        // the cut-points) to 1e-6, the analytic information to 1e-8.
        let se: Vec<f64> = fit.thresholds.iter().chain(&fit.slopes).map(|c| c.std_error).collect();
        let sm: Vec<f64> = nums(r.get("threshold_se_statsmodels").unwrap())
            .into_iter()
            .chain(nums(r.get("slope_se_statsmodels").unwrap()))
            .collect();
        let analytic = nums(r.get("se_analytic").unwrap());
        for k in 0..se.len() {
            assert!(close(se[k], sm[k], 1e-6), "{name} se{k}: {} vs statsmodels {}", se[k], sm[k]);
            assert!(close(se[k], analytic[k], 1e-8), "{name} se{k}: {} vs analytic {}", se[k], analytic[k]);
        }
        for (i, want) in r.get("probabilities_first_rows").unwrap().as_array().unwrap().iter().enumerate() {
            for (g, w) in fit.probabilities[i].iter().zip(nums(want)) {
                assert!((g - w).abs() < 1e-7, "{name} row {i}: {g} vs {w}");
            }
        }
        let ll0 = c.get("llnull").unwrap().as_f64().unwrap();
        assert!(close(fit.statistics.null_log_likelihood, ll0, 1e-12));
        let k = est.len() as f64;
        assert!(close(fit.statistics.aic, -2.0 * ll + 2.0 * k, 1e-9));
        assert!(close(fit.statistics.bic, -2.0 * ll + k * (d.y.len() as f64).ln(), 1e-9));
        assert!(close(fit.statistics.mcfadden_r2, 1.0 - ll / ll0, 1e-8));
        // Parallel lines: independent finite-difference score and information.
        let s = c.get("score_test").unwrap();
        let test = fit.parallel_lines.unwrap();
        let chi = s.get("chi_square").unwrap().as_f64().unwrap();
        assert_eq!(test.df, s.get("df").unwrap().as_usize().unwrap());
        assert!(close(test.chi_square, chi, 1e-6), "{name}: score {} vs {chi}", test.chi_square);
        let p = s.get("p_value").unwrap().as_f64().unwrap();
        assert!(close(test.p_value, p, 1e-5), "{name}: p {} vs {p}", test.p_value);
        // Likelihood-ratio tests per term: refits without the term's columns.
        for red in c.get("reduced").unwrap().as_array().unwrap() {
            let drop: Vec<usize> = nums(red.get("drop").unwrap()).into_iter().map(|v| v as usize).collect();
            let x = keep_columns(&d.x, &drop);
            let reduced = fit_ordinal_logistic(&x, &d.y, &d.levels, &PolytomousOptions::default()).unwrap();
            let want = red.get("llf").unwrap().as_f64().unwrap();
            assert!(close(reduced.statistics.log_likelihood, want, 1e-9), "{name} without {drop:?}");
        }
    }
}

#[test]
fn baseline_category_logits_match_statsmodels() {
    let c = cases();
    let c = c.get("nominal").unwrap();
    let d = case(c);
    let options = PolytomousOptions { names: d.names.clone(), ..PolytomousOptions::default() };
    for (reference, key, order) in [(0, "fit", [1usize, 2]), (2, "reference_2", [0, 1])] {
        let fit = fit_nominal_logistic(&d.x, &d.y, &d.levels, reference, &options).unwrap();
        let r = c.get(key).unwrap();
        assert!(fit.statistics.converged);
        let ll = r.get("llf").unwrap().as_f64().unwrap();
        assert!(close(fit.statistics.log_likelihood, ll, 1e-10), "ℓ {} vs {ll}", fit.statistics.log_likelihood);
        // statsmodels fits the null model iteratively (to about 1e-10).
        assert!(close(fit.statistics.null_log_likelihood, r.get("llnull").unwrap().as_f64().unwrap(), 1e-9));
        assert!(close(fit.statistics.mcfadden_r2, r.get("prsquared").unwrap().as_f64().unwrap(), 1e-8));
        let params = r.get("params").unwrap().as_array().unwrap();
        let bse = r.get("bse").unwrap().as_array().unwrap();
        // statsmodels' equations follow the recoded response; `order` maps
        // them to category indices.
        for (e, eq) in fit.equations.iter().enumerate() {
            assert_eq!(eq.level, order[e]);
            for (k, coef) in eq.coefficients.iter().enumerate() {
                let (b, se) = (nums(&params[e])[k], nums(&bse[e])[k]);
                assert!(close(coef.estimate, b, 1e-6), "ref {reference} eq {e} b{k}: {} vs {b}", coef.estimate);
                assert!(close(coef.std_error, se, 1e-6), "ref {reference} eq {e} se{k}: {} vs {se}", coef.std_error);
            }
        }
        // Probabilities, in statsmodels' (recoded) category order.
        let perm: Vec<usize> = if reference == 0 { vec![0, 1, 2] } else { vec![2, 0, 1] };
        for (i, want) in r.get("probabilities_first_rows").unwrap().as_array().unwrap().iter().enumerate() {
            for (code, w) in nums(want).into_iter().enumerate() {
                let g = fit.probabilities[i][perm[code]];
                assert!((g - w).abs() < 1e-8, "row {i}: {g} vs {w}");
            }
        }
    }
    for red in c.get("reduced").unwrap().as_array().unwrap() {
        let drop: Vec<usize> = nums(red.get("drop").unwrap()).into_iter().map(|v| v as usize).collect();
        let x = keep_columns(&d.x, &drop);
        let reduced = fit_nominal_logistic(&x, &d.y, &d.levels, 0, &PolytomousOptions::default()).unwrap();
        let want = red.get("llf").unwrap().as_f64().unwrap();
        assert!(close(reduced.statistics.log_likelihood, want, 1e-10), "without {drop:?}");
    }
}
