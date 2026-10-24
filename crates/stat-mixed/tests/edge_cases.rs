//! Degenerate data, invalid input and larger models: errors where the model
//! cannot be fitted, finite results and warnings where it can.

use stat_core::{Matrix, StatError};
use stat_mixed::{MixedFamily, MixedModel, MixedOptions, RandomTerm, fit_mixed};

/// A small deterministic pseudo-random sequence in [0, 1).
fn lcg(seed: u64) -> impl FnMut() -> f64 {
    let mut s = seed;
    move || {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((s >> 11) as f64) / (1u64 << 53) as f64
    }
}

fn normal(r: &mut impl FnMut() -> f64) -> f64 {
    // Box–Muller.
    let (a, b) = (r().max(1e-300), r());
    (-2.0 * a.ln()).sqrt() * (2.0 * std::f64::consts::PI * b).cos()
}

fn term(group: &str, codes: Vec<usize>, g: usize) -> RandomTerm {
    let n = codes.len();
    RandomTerm {
        group: group.into(),
        levels: (0..g).map(|j| j.to_string()).collect(),
        codes,
        effects: vec!["Intercept".into()],
        z: Matrix::from_columns(&[vec![1.0; n]]).unwrap(),
        correlated: true,
    }
}

fn simple(y: Vec<f64>, codes: Vec<usize>, g: usize, family: MixedFamily) -> MixedModel {
    let n = y.len();
    MixedModel {
        y,
        x: Matrix::from_columns(&[vec![1.0; n]]).unwrap(),
        x_names: vec!["Intercept".into()],
        weights: Vec::new(),
        offset: Vec::new(),
        random: vec![term("G", codes, g)],
        family,
        residual_groups: None,
        categories: Vec::new(),
    }
}

fn assert_finite(fit: &stat_mixed::MixedFit) {
    assert!(fit.minus_two_log_likelihood.is_finite());
    for f in &fit.fixed {
        assert!(f.estimate.is_finite() && f.std_error.is_finite() && f.p_value.is_finite(), "{f:?}");
    }
    assert!(fit.variance_components.iter().all(|c| c.variance.is_finite() && c.variance >= 0.0));
    assert!(fit.fitted.iter().all(|v| v.is_finite()));
    assert!(fit.modes.iter().all(|m| m.values.iter().flatten().all(|v| v.is_finite())));
}

#[test]
fn invalid_inputs_are_errors() {
    let codes: Vec<usize> = (0..12).map(|i| i % 3).collect();
    let y: Vec<f64> = (0..12).map(|i| f64::from(i % 2)).collect();
    let ok = simple(y.clone(), codes.clone(), 3, MixedFamily::Binomial);
    assert!(fit_mixed(&ok, &MixedOptions::default()).is_ok());
    let mut bad = ok.clone();
    bad.y[0] = 2.0;
    assert!(matches!(fit_mixed(&bad, &MixedOptions::default()), Err(StatError::InvalidArgument { .. })));
    let mut bad = simple(y.iter().map(|v| v + 1.0).collect(), codes.clone(), 3, MixedFamily::Gamma);
    bad.y[3] = 0.0;
    assert!(fit_mixed(&bad, &MixedOptions::default()).is_err());
    let mut bad = ok.clone();
    bad.random.clear();
    assert!(fit_mixed(&bad, &MixedOptions::default()).is_err());
    let mut bad = ok.clone();
    bad.random[0].codes[0] = 7;
    assert!(fit_mixed(&bad, &MixedOptions::default()).is_err());
    let mut bad = ok.clone();
    bad.x = Matrix::from_columns(&[vec![1.0; 12], vec![2.0; 12]]).unwrap();
    bad.x_names = vec!["a".into(), "b".into()];
    assert!(matches!(fit_mixed(&bad, &MixedOptions::default()), Err(StatError::RankDeficient { .. })));
    let mut bad = ok.clone();
    bad.weights = vec![1.0; 5];
    assert!(matches!(fit_mixed(&bad, &MixedOptions::default()), Err(StatError::DimensionMismatch { .. })));
    // Quadrature needs one term with one effect, and 1–25 points.
    let mut two = ok.clone();
    two.random.push(term("H", (0..12).map(|i| i % 4).collect(), 4));
    let agq = MixedOptions { quadrature_points: 9, ..MixedOptions::default() };
    assert!(fit_mixed(&two, &agq).is_err());
    assert!(fit_mixed(&ok, &MixedOptions { quadrature_points: 26, ..MixedOptions::default() }).is_err());
    assert!(fit_mixed(&ok, &MixedOptions { quadrature_points: 0, ..MixedOptions::default() }).is_err());
    assert!(fit_mixed(&ok, &MixedOptions { confidence_level: 1.0, ..MixedOptions::default() }).is_err());
    let mut bad = ok;
    bad.x[(0, 0)] = f64::NAN;
    assert!(fit_mixed(&bad, &MixedOptions::default()).is_err());
}

#[test]
fn degenerate_data_gives_finite_fits_and_warnings() {
    let mut r = lcg(7);
    // No group effect at all: a singular fit.
    let codes: Vec<usize> = (0..40).map(|i| i % 8).collect();
    let y: Vec<f64> = (0..40).map(|i| 10.0 + if (i / 8) % 2 == 0 { 0.5 } else { -0.5 }).collect();
    let fit = fit_mixed(&simple(y, codes.clone(), 8, MixedFamily::Normal), &MixedOptions::default()).unwrap();
    assert_finite(&fit);
    assert!(fit.variance_components[0].variance < 1e-8);
    assert!(fit.warnings.iter().any(|w| w.starts_with("Singular")), "{:?}", fit.warnings);
    // One level only.
    let y1: Vec<f64> = (0..10).map(|_| normal(&mut r)).collect();
    let fit = fit_mixed(&simple(y1, vec![0; 10], 1, MixedFamily::Normal), &MixedOptions::default()).unwrap();
    assert_finite(&fit);
    // Groups that are all 0 or all 1 (quasi-separation by group).
    let yb: Vec<f64> = (0..40)
        .map(|i| {
            if i % 8 < 2 {
                1.0
            } else if i % 8 < 4 {
                0.0
            } else {
                f64::from(u8::from(r() < 0.5))
            }
        })
        .collect();
    let fit =
        fit_mixed(&simple(yb.clone(), codes.clone(), 8, MixedFamily::Binomial), &MixedOptions::default()).unwrap();
    assert_finite(&fit);
    let agq = fit_mixed(
        &simple(yb, codes.clone(), 8, MixedFamily::Binomial),
        &MixedOptions { quadrature_points: 15, ..MixedOptions::default() },
    )
    .unwrap();
    assert_finite(&agq);
    // All-zero counts in some groups; NB with no overdispersion (shape → large).
    let yc: Vec<f64> = (0..40).map(|i| if i % 8 == 0 { 0.0 } else { f64::from((i % 5) as u8) }).collect();
    for fam in [MixedFamily::Poisson, MixedFamily::NegativeBinomial] {
        let fit = fit_mixed(&simple(yc.clone(), codes.clone(), 8, fam), &MixedOptions::default()).unwrap();
        assert_finite(&fit);
    }
    // Constant responses: no variance to estimate.
    assert!(fit_mixed(&simple(vec![3.0; 40], codes.clone(), 8, MixedFamily::Gamma), &MixedOptions::default()).is_err());
    assert!(fit_mixed(&simple(vec![5.0; 40], codes, 8, MixedFamily::Normal), &MixedOptions::default()).is_err());
}

#[test]
fn large_models_fit_quickly() {
    let mut r = lcg(11);
    // 1500 subjects × 6 observations with a random slope, crossed with 25 sites.
    let (g, per) = (1500, 6);
    let n = g * per;
    let subject: Vec<usize> = (0..n).map(|i| i / per).collect();
    let site: Vec<usize> = (0..n).map(|i| (i * 7) % 25).collect();
    let t: Vec<f64> = (0..n).map(|i| (i % per) as f64).collect();
    let b0: Vec<f64> = (0..g).map(|_| 2.0 * normal(&mut r)).collect();
    let b1: Vec<f64> = (0..g).map(|_| 0.5 * normal(&mut r)).collect();
    let s: Vec<f64> = (0..25).map(|_| normal(&mut r)).collect();
    let y: Vec<f64> = (0..n)
        .map(|i| 3.0 + 0.8 * t[i] + b0[subject[i]] + b1[subject[i]] * t[i] + s[site[i]] + normal(&mut r))
        .collect();
    let model = MixedModel {
        y,
        x: Matrix::from_columns(&[vec![1.0; n], t.clone()]).unwrap(),
        x_names: vec!["Intercept".into(), "t".into()],
        weights: Vec::new(),
        offset: Vec::new(),
        random: vec![
            RandomTerm {
                group: "Subject".into(),
                levels: (0..g).map(|j| j.to_string()).collect(),
                codes: subject.clone(),
                effects: vec!["Intercept".into(), "t".into()],
                z: Matrix::from_columns(&[vec![1.0; n], t]).unwrap(),
                correlated: true,
            },
            term("Site", site, 25),
        ],
        family: MixedFamily::Normal,
        residual_groups: None,
        categories: Vec::new(),
    };
    let start = std::time::Instant::now();
    let fit = fit_mixed(&model, &MixedOptions::default()).unwrap();
    let secs = start.elapsed().as_secs_f64();
    eprintln!("normal, {n} rows, {} effects: {secs:.2}s, {} evaluations", 2 * g + 25, fit.evaluations);
    assert_finite(&fit);
    assert!((fit.fixed[1].estimate - 0.8).abs() < 0.1);
    assert!((fit.variance_components[0].variance - 4.0).abs() < 1.0);
    assert!(secs < 30.0, "{secs}s");
    // Binary outcomes, 1000 clusters of 6, Laplace.
    let (g, per) = (1000, 6);
    let n = g * per;
    let codes: Vec<usize> = (0..n).map(|i| i / per).collect();
    let b: Vec<f64> = (0..g).map(|_| normal(&mut r)).collect();
    let x: Vec<f64> = (0..n).map(|_| normal(&mut r)).collect();
    let y: Vec<f64> =
        (0..n).map(|i| f64::from(u8::from(r() < 1.0 / (1.0 + (-(0.5 * x[i] + b[codes[i]])).exp())))).collect();
    let model = MixedModel {
        y,
        x: Matrix::from_columns(&[vec![1.0; n], x]).unwrap(),
        x_names: vec!["Intercept".into(), "x".into()],
        weights: Vec::new(),
        offset: Vec::new(),
        random: vec![term("Cluster", codes, g)],
        family: MixedFamily::Binomial,
        residual_groups: None,
        categories: Vec::new(),
    };
    let start = std::time::Instant::now();
    let fit = fit_mixed(&model, &MixedOptions::default()).unwrap();
    let secs = start.elapsed().as_secs_f64();
    eprintln!("binomial Laplace, {n} rows: {secs:.2}s, {} evaluations", fit.evaluations);
    assert_finite(&fit);
    assert!(secs < 30.0, "{secs}s");
    let start = std::time::Instant::now();
    let fit = fit_mixed(&model, &MixedOptions { quadrature_points: 10, ..MixedOptions::default() }).unwrap();
    eprintln!("binomial AGQ(10), {n} rows: {:.2}s", start.elapsed().as_secs_f64());
    assert_finite(&fit);
}

/// Satterthwaite χ² limits of a variance from its estimate and standard
/// error. Reference values from SciPy 1.x: `ν = 2(v/se)²`, limits
/// `ν·v/chi2.ppf(0.975, ν)` and `ν·v/chi2.ppf(0.025, ν)`.
#[test]
fn satterthwaite_variance_limits() {
    for (v, se, lo, hi) in [
        (4.0, 1.5, 2.152_891_014_256_357, 9.862_206_696_928_622),
        (0.8, 1.1, 0.163_368_864_052_196, 566.441_274_690_715_3),
        (2.5, 0.4, 1.869_724_187_605_763, 3.514_649_205_472_299),
    ] {
        let (a, b) = stat_mixed::satterthwaite_limits(v, se, 0.95).unwrap();
        assert!((a - lo).abs() < 1e-8 * lo && (b - hi).abs() < 1e-8 * hi, "{v} {se}: {a} {b}");
        assert!(a > 0.0, "positive where the Wald limit {} is not", v - 1.96 * se);
    }
    assert!(stat_mixed::satterthwaite_limits(0.0, 1.0, 0.95).is_none());
}
