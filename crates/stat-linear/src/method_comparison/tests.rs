use super::*;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

/// Forty paired measurements (comparison method x, test method y) with one
/// gross outlier (row 18); the sample dataset "Method Comparison". Reference
/// values were computed independently with a standard-library Python script
/// (`ref_mc.py` in the PR notes) that follows the published formulas:
/// Deming (Cornbleet & Gochman 1979) with jackknife standard errors (Linnet
/// 1990), Passing & Bablok (1983) with their CUSUM linearity test.
pub(crate) const X: [f64; 40] = [
    73.4, 76.0, 71.3, 85.0, 97.2, 118.7, 128.8, 133.3, 140.5, 143.9, 184.7, 183.2, 180.3, 201.3, 195.5, 211.9, 218.6,
    220.0, 224.3, 231.7, 255.4, 267.5, 260.8, 270.8, 268.7, 272.1, 312.4, 312.7, 321.2, 327.6, 318.8, 333.7, 332.3,
    350.7, 352.9, 347.9, 350.4, 370.1, 386.7, 372.2,
];
pub(crate) const Y: [f64; 40] = [
    87.5, 81.7, 83.1, 86.7, 99.4, 129.2, 134.5, 145.5, 151.0, 158.6, 181.0, 196.7, 199.5, 205.0, 209.0, 225.3, 234.8,
    264.9, 242.6, 249.6, 266.9, 279.7, 282.7, 280.8, 289.3, 281.2, 323.1, 328.2, 336.7, 342.2, 334.8, 353.0, 340.7,
    356.9, 376.8, 375.4, 379.9, 390.6, 398.7, 387.4,
];

#[test]
fn deming_matches_reference() {
    let fit = deming_regression(&X, &Y, 1.0, 0.95).unwrap();
    assert!(close(fit.intercept.estimate, 5.511_193_428_356_194, 1e-10));
    assert!(close(fit.slope.estimate, 1.036_367_222_143_800_5, 1e-12));
    assert!(close(fit.intercept.std_error.unwrap(), 2.704_090_988_050_727, 1e-9));
    assert!(close(fit.slope.std_error.unwrap(), 0.010_702_471_079_400_76, 1e-9));
    // t(0.975, 38) = 2.024394163911901 (Python bisection of the t cdf).
    assert!(close(fit.slope.lower, 1.014_701_202_151_225_6, 1e-8));
    assert!(close(fit.slope.upper, 1.058_033_242_136_375_4, 1e-8));
    assert!(close(fit.intercept.lower, 0.037_047_413_459_536_216, 1e-7));
    assert!(close(fit.intercept.upper, 10.985_339_443_252_851, 1e-8));
    let fit = deming_regression(&X, &Y, 1.5625, 0.95).unwrap();
    assert!(close(fit.slope.estimate, 1.035_655_934_114_847_7, 1e-12));
    assert!(close(fit.slope.std_error.unwrap(), 0.010_685_238_288_828_431, 1e-9));
    assert!(close(fit.intercept.estimate, 5.680_204_355_135_743, 1e-10));
    assert_eq!(fit.jackknife_slopes.len(), 40);
}

#[test]
fn deming_limits_and_exact_lines() {
    // Points on y = 1 + 2x: every ratio recovers the line, with zero spread.
    let x = [1.0, 2.0, 3.0, 4.0, 5.0];
    let y: Vec<f64> = x.iter().map(|v| 1.0 + 2.0 * v).collect();
    for lambda in [0.01, 1.0, 100.0] {
        let f = deming_regression(&x, &y, lambda, 0.95).unwrap();
        assert!(close(f.slope.estimate, 2.0, 1e-12) && close(f.intercept.estimate, 1.0, 1e-12));
        assert!(f.slope.std_error.unwrap() < 1e-12);
        assert!(close(f.predict(10.0), 21.0, 1e-12));
    }
    // A large ratio approaches least squares of y on x; a small one, of x on y.
    // S_xx = 10, S_xy = 19.9, S_yy = 39.708 (hand computation).
    let y2 = [2.1, 3.9, 6.2, 7.8, 10.1];
    assert!(close(deming_regression(&x, &y2, 1e9, 0.95).unwrap().slope.estimate, 19.9 / 10.0, 1e-6));
    assert!(close(deming_regression(&x, &y2, 1e-9, 0.95).unwrap().slope.estimate, 39.708 / 19.9, 1e-6));
    assert!(deming_regression(&x, &y2, 0.0, 0.95).is_err());
    assert!(deming_regression(&x[..2], &y2[..2], 1.0, 0.95).is_err());
    assert!(deming_regression(&[1.0, 2.0, 3.0], &[5.0, 5.0, 5.0], 1.0, 0.95).is_err());
}

/// Hand computation: slopes of (1,1), (2,3), (3,2), (4,5) are 2, ½, 4/3, −1
/// (dropped), 1 and 3; N = 5, K = 0, so b = 4/3 and a = median(−⅓, ⅓, −2, −⅓)
/// = −⅓.
#[test]
fn passing_bablok_hand_example() {
    let f = passing_bablok(&[1.0, 2.0, 3.0, 4.0], &[1.0, 3.0, 2.0, 5.0], 0.95).unwrap();
    assert_eq!((f.n_slopes, f.offset), (5, 0));
    assert!(close(f.slope.estimate, 4.0 / 3.0, 1e-14));
    assert!(close(f.intercept.estimate, -1.0 / 3.0, 1e-14));
    // With n = 4 the limits are the extreme slopes.
    assert!(f.slope.lower <= 0.5 && f.slope.upper >= 3.0);
}

#[test]
fn passing_bablok_matches_reference() {
    let f = passing_bablok(&X, &Y, 0.95).unwrap();
    assert_eq!((f.n_slopes, f.offset), (780, 11));
    assert!(close(f.slope.estimate, 1.034_595_274_861_371_2, 1e-12));
    assert!(close(f.slope.lower, 1.011_424_731_182_795_5, 1e-12));
    assert!(close(f.slope.upper, 1.060_790_273_556_231_3, 1e-12));
    assert!(close(f.intercept.estimate, 5.854_312_569_426_398, 1e-10));
    assert!(close(f.intercept.lower, -0.233_586_626_139_896_23, 1e-9));
    assert!(close(f.intercept.upper, 10.982_258_064_516_16, 1e-10));
    let lin = f.linearity.as_ref().unwrap();
    assert_eq!((lin.n_above, lin.n_below), (20, 20));
    assert!(close(lin.max_cusum, 6.0, 1e-12));
    // 6/√40 and its Kolmogorov tail (scipy.special.kolmogorov).
    assert!(close(lin.statistic, 0.948_683_298_050_513_8, 1e-12));
    assert!(close(lin.p_value, 0.329_104_789_097_815_1, 1e-9));
    // The outlier (row 18) barely moves Passing–Bablok but pulls Deming.
    let mut y = Y;
    y[17] -= 45.0;
    let clean = passing_bablok(&X, &y, 0.95).unwrap();
    assert!((clean.slope.estimate - f.slope.estimate).abs() < 0.005);
}

/// The statistic equals the two-sample Kolmogorov–Smirnov statistic
/// `√(lL/(l + L))·D` between the projections of the points above and below
/// the line, computed here directly from the two empirical distributions.
#[test]
fn cusum_statistic_is_two_sample_kolmogorov_smirnov() {
    let x: Vec<f64> = (1..=25).map(f64::from).collect();
    let y: Vec<f64> = x.iter().map(|v| 1.0 + 0.9 * v + 1.7 * (v * 1.3).sin()).collect();
    let f = passing_bablok(&x, &y, 0.95).unwrap();
    let (a, b) = (f.intercept.estimate, f.slope.estimate);
    let lin = f.linearity.unwrap();
    let scale = (1.0 + 1.0 / (b * b)).sqrt();
    let mut above = Vec::new();
    let mut below = Vec::new();
    for (xi, yi) in x.iter().zip(&y) {
        let d = (yi + xi / b - a / b) / scale;
        let r = yi - (a + b * xi);
        if r > 0.0 {
            above.push(d);
        } else if r < 0.0 {
            below.push(d);
        }
    }
    let ecdf = |v: &[f64], t: f64| v.iter().filter(|&&s| s <= t).count() as f64 / v.len() as f64;
    let d = above.iter().chain(&below).map(|&t| (ecdf(&above, t) - ecdf(&below, t)).abs()).fold(0.0, f64::max);
    let (l, big_l) = (above.len() as f64, below.len() as f64);
    assert_eq!((lin.n_above, lin.n_below), (above.len(), below.len()));
    assert!(close(lin.statistic, (l * big_l / (l + big_l)).sqrt() * d, 1e-12), "{}", lin.statistic);
}

#[test]
fn kolmogorov_tail_and_curved_data() {
    // Known values: P(K > 1.36) ≈ 0.0494, P(K > 1.0) ≈ 0.2700.
    assert!((kolmogorov_sf(1.36) - 0.049_44).abs() < 1e-4);
    assert!((kolmogorov_sf(1.0) - 0.269_99).abs() < 1e-4);
    assert_eq!(kolmogorov_sf(0.1), 1.0);
    // Strongly curved data: runs of residuals of one sign give a large CUSUM.
    let x: Vec<f64> = (1..=50).map(f64::from).collect();
    let y: Vec<f64> = x.iter().map(|v| v * v / 10.0).collect();
    let f = passing_bablok(&x, &y, 0.95).unwrap();
    let lin = f.linearity.unwrap();
    assert!(lin.p_value < 0.05, "{}", lin.p_value);
    assert!(passing_bablok(&[1.0, 1.0, 1.0], &[2.0, 2.0, 2.0], 0.95).is_err());
}

/// A huge error-variance ratio: the slope must not cancel (the textbook root
/// lost every digit by λ = 1e18). References from the closed form evaluated in
/// 80-digit decimal arithmetic on the same binary64 data (Python `decimal`).
#[test]
fn deming_huge_variance_ratio_keeps_its_digits() {
    let x = [1.0, 2.0, 3.0, 4.0, 5.0];
    let y = [2.1, 3.9, 6.2, 7.8, 10.1];
    for (lambda, slope) in [(1e8, 1.990_000_000_212_929_9), (1e12, 1.990_000_000_000_021_2), (1e18, 1.99)] {
        let f = deming_regression(&x, &y, lambda, 0.95).unwrap();
        assert!(close(f.slope.estimate, slope, 1e-12), "λ = {lambda}: {}", f.slope.estimate);
    }
}

/// Negatively related methods (y = −3x + noise): every pairwise slope is
/// below −1, so K = N and the upper limit's rank M₂ + K exceeds N. Passing &
/// Bablok (1983) give no finite limit; it used to be clamped to the largest
/// slope (≈ −3) without a word.
#[test]
fn passing_bablok_negative_relation_has_no_finite_upper_limit() {
    let x: Vec<f64> = (1..=20).map(f64::from).collect();
    let y: Vec<f64> = x.iter().map(|v| -3.0 * v + 0.01 * (v * 1.7).sin()).collect();
    let f = passing_bablok(&x, &y, 0.95).unwrap();
    assert_eq!(f.offset, f.n_slopes);
    assert!(f.slope.upper.is_infinite() && f.slope.upper > 0.0);
    assert!(f.note.as_deref().is_some_and(|n| n.contains("not positively related")), "{:?}", f.note);
    // A positive relation keeps finite limits and no note.
    let g = passing_bablok(&X, &Y, 0.95).unwrap();
    assert!(g.note.is_none() && g.slope.upper.is_finite());
}
