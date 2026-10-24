//! Spline bases and spline terms in linear models against R 4.3.3
//! (`splines::bs`, `splines::ns`, `lm`, `drop1`); the script is
//! `tests/data/spline_reference.R`.
#![allow(clippy::excessive_precision)]

use stat_linear::{
    ModelDesign, OlsOptions, Predictor, PredictorValues, SplineBasis, SplineKind, SplineSpec, TermValue, effect_tests,
    fit_linear_model,
};

const X: [f64; 12] = [1.2, 2.5, 3.1, 4.8, 5.0, 6.7, 7.3, 8.9, 9.4, 10.6, 11.2, 12.8];
const AT: [f64; 6] = [-1.0, 1.2, 3.0, 6.0, 12.8, 15.0];

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{a} vs {b}");
}

fn check(basis: &SplineBasis, expected: &[f64]) {
    let k = basis.n_columns();
    assert_eq!(expected.len(), AT.len() * k);
    for (i, &x) in AT.iter().enumerate() {
        for (j, v) in basis.evaluate(x).into_iter().enumerate() {
            close(v, expected[i * k + j], 1e-12);
        }
    }
}

#[test]
fn cubic_bspline_with_quantile_knots_matches_bs() {
    let b = SplineSpec::bspline(3, 3).basis(&X).unwrap();
    for (k, r) in b.interior().iter().zip([4.375, 7.0, 9.7000000000000011]) {
        close(*k, r, 1e-14);
    }
    check(
        &b,
        &[
            -4.9221141894491991,
            1.1383257528493098,
            -0.06802638513999136,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.54529749626128787,
            0.33622127167315119,
            0.037258628675472354,
            0.0,
            0.0,
            0.0,
            0.011324387067549969,
            0.37882847511342421,
            0.57341025075314245,
            0.036436887065883591,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            -0.070292411944692851,
            1.1709987956289489,
            -5.0980881432760761,
            4.9973817595918222,
        ],
    );
}

#[test]
fn quadratic_bspline_with_given_knots_matches_bs() {
    let b = SplineBasis::new(SplineKind::BSpline { degree: 2 }, (1.2, 12.8), vec![4.0, 9.0]).unwrap();
    check(
        &b,
        &[
            -2.4103872318158035,
            0.22161172161172168,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.72409733124018849,
            0.14835164835164838,
            0.0,
            0.0,
            0.23076923076923084,
            0.67832167832167845,
            0.090909090909090912,
            0.0,
            0.0,
            0.0,
            0.0,
            0.99999999999999989,
            0.0,
            0.144736842105263,
            -1.637811634349029,
            2.4930747922437662,
        ],
    );
}

#[test]
fn natural_splines_match_ns() {
    let b = SplineSpec::natural(3).basis(&X).unwrap();
    for (k, r) in b.interior().iter().zip([4.375, 7.0, 9.7000000000000011]) {
        close(*k, r, 1e-14);
    }
    check(
        &b,
        &[
            0.0,
            0.1942090737149571,
            -0.55756798582681233,
            0.36335891211185528,
            0.0,
            0.0,
            0.0,
            0.0,
            0.037258628675472354,
            -0.13973947443987331,
            0.4011875233918944,
            -0.26144804895202112,
            0.57341025075314245,
            -0.06752715298878427,
            0.29847740531823996,
            -0.19451336526357219,
            0.0,
            -0.14682345255593668,
            0.42152539604768918,
            0.72529805650824741,
            0.0,
            -0.93908214927323197,
            0.56704229952637641,
            1.3720398497468553,
        ],
    );
    let b = SplineBasis::new(SplineKind::Natural, (1.2, 12.8), vec![7.0]).unwrap();
    check(
        &b,
        &[
            -0.28955415185626604,
            0.19303610123751069,
            0.0,
            0.0,
            0.23101630507275969,
            -0.14902911597904892,
            0.52003161218508154,
            -0.25221892354700765,
            0.34409691742892717,
            0.77060205504738188,
            0.18506228597673746,
            1.255935487509761,
        ],
    );
}

#[test]
fn linear_model_with_a_spline_term_matches_lm() {
    let y = [3.1, 4.9, 5.2, 7.9, 8.4, 9.1, 8.8, 9.9, 11.3, 10.2, 9.8, 12.5];
    let g: Vec<String> =
        ["a", "b", "a", "b", "c", "a", "c", "b", "c", "a", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
    let preds = [
        Predictor { name: "x", values: PredictorValues::Continuous(&X) },
        Predictor { name: "g", values: PredictorValues::Categorical(&g) },
    ];
    let rows: Vec<usize> = (0..12).collect();
    let design =
        ModelDesign::with_splines(&preds, &rows, &[vec![(0, 1)], vec![(1, 1)]], &[Some(SplineSpec::natural(2))])
            .unwrap();
    assert_eq!(design.effects()[0].name, "x (spline)");
    assert_eq!(design.column_names().len(), 5);
    let x = design.matrix(&preds, &rows).unwrap();
    let options = OlsOptions { predictor_names: design.column_names().to_vec(), ..OlsOptions::default() };
    let fit = fit_linear_model(&x, &y, &options).unwrap();
    for (b, r) in fit.coefficients().iter().zip([
        3.0195644916512987,
        4.9496997746409912,
        12.253354378369043,
        5.2537763608550954,
        0.025884911891719622,
        0.82157980415709253,
    ]) {
        close(*b, r, 1e-9);
    }
    for (f, r) in fit.fitted_values().iter().zip([
        3.0195644916512956,
        4.8337042420207252,
        5.5860977527684055,
        7.4740412074931477,
        8.443210777039555,
        8.7072339038734903,
        9.7941351405230357,
        9.5986670026300551,
        10.589216568893345,
        10.287103851706808,
        10.593587547856073,
        12.173437513544066,
    ]) {
        close(*f, r, 1e-10);
    }
    let tests = effect_tests(&design, &x, &y, &fit, &options).unwrap();
    assert_eq!(tests[0].df, 3);
    for (t, (f, p)) in tests
        .iter()
        .zip([41.28446831488386, 1.5046964319624467].into_iter().zip([0.0002120208336572652, 0.2953705390399729]))
    {
        close(t.f_statistic.unwrap(), f, 1e-9);
        close(t.p_value.unwrap(), p, 1e-8);
    }
    let new: Vec<Vec<f64>> = [(0.0, 0), (6.0, 1), (14.0, 2)]
        .iter()
        .map(|&(x, l)| design.encode(&[TermValue::Number(x), TermValue::Level(l)]).unwrap())
        .collect();
    let pred = fit.predict(&stat_core::Matrix::from_rows(&new).unwrap()).unwrap();
    for (p, r) in pred.iter().zip([1.3427005335429241, 8.358686856746802, 12.769796876693903]) {
        close(p.fitted, r, 1e-9);
    }
}

#[test]
fn splines_stay_out_of_interactions() {
    let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let z = [2.0, 1.0, 4.0, 3.0, 6.0, 5.0];
    let preds = [
        Predictor { name: "x", values: PredictorValues::Continuous(&x) },
        Predictor { name: "z", values: PredictorValues::Continuous(&z) },
    ];
    let rows: Vec<usize> = (0..6).collect();
    let e = [vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]];
    assert!(ModelDesign::with_splines(&preds, &rows, &e, &[Some(SplineSpec::natural(1))]).is_err());
}
