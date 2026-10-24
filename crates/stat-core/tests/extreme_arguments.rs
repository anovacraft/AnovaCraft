//! Distribution functions, random draws and decompositions at extreme
//! arguments: values near the overflow and underflow limits of binary64, tiny
//! shapes and degrees of freedom, and far tails (code review 2026-10-19 and
//! the Antigravity review items F-03 to F-06 and F-26 to F-33).
//!
//! Reference values were computed with mpmath 1.4 at 60 significant digits
//! from the defining integrals or closed forms (incomplete beta and gamma
//! functions, the noncentral t as `E_V[Φ̄(t√(V/ν) − δ)]` by quadrature over
//! the χ² density, gamma functions for moments), and SciPy 1.18
//! `integrate.quad` for the studentized range; independent of the series,
//! continued fractions and quadrature under test. Scripts:
//! `data/extreme_arguments.py`, `data/studentized_range_large_df.py`.

use stat_core::box_cox::box_cox_fit;
use stat_core::catalog::{Distribution, Family};
use stat_core::correlation::pearson;
use stat_core::eigen::{cholesky, symmetric_eigen};
use stat_core::optimize::minimize_in_box;
use stat_core::rng::Rng;
use stat_core::special::erfc;
use stat_core::{
    ChiSquared, ContinuousDistribution, FisherSnedecor, Matrix, NoncentralT, Normal, StudentizedRange, StudentsT, svd,
};

fn rel_close(computed: f64, expected: f64, tol: f64) {
    assert!(
        (computed - expected).abs() <= tol * expected.abs(),
        "computed {computed:e}, expected {expected:e} (relative tolerance {tol:e})"
    );
}

#[test]
fn normal_tails_beyond_the_square_overflow_are_exact_limits() {
    let n = Normal::standard();
    assert_eq!(n.cdf(1e200).unwrap(), 1.0);
    assert_eq!(n.sf(1e200).unwrap(), 0.0);
    assert_eq!(n.cdf(-1e200).unwrap(), 0.0);
    assert_eq!(erfc(1e155), 0.0);
    assert_eq!(erfc(-1e155), 2.0);
    // The cut-off at |x| = 40 leaves ordinary tails alone: Φ̄(10) (mpmath).
    rel_close(n.sf(10.0).unwrap(), 7.619_853_024_160_526e-24, 1e-12);
    let catalog = Distribution::new(Family::Normal, &[0.0, 1.0]).unwrap();
    assert_eq!(catalog.cdf(1e160), 1.0);
}

#[test]
fn t_and_f_tails_where_the_squared_statistic_overflows() {
    // F-27: these returned an error (NaN in the calculator).
    let t5 = StudentsT::new(5.0).unwrap();
    assert_eq!(t5.cdf(1e200).unwrap(), 1.0);
    assert_eq!(t5.cdf(-1e200).unwrap(), 0.0);
    assert_eq!(t5.sf(1e200).unwrap(), 0.0); // the true 9.5e-1000 underflows
    // Where t² overflows but the tail is representable it is kept:
    // P(T₁ > t) = atan(1/t)/π = 1/(π·2·10¹⁵⁴) to working precision.
    rel_close(StudentsT::new(1.0).unwrap().sf(2e154).unwrap(), 1.591_549_430_918_953_5e-155, 1e-10);
    let f = FisherSnedecor::new(10.0, 0.5).unwrap();
    rel_close(f.sf(1e308).unwrap(), 7.654_134_359_974_433e-78, 1e-10);
    assert_eq!(f.cdf(1e308).unwrap(), 1.0);
    let nct = NoncentralT::new(5.0, 1.0).unwrap();
    assert_eq!(nct.cdf(1e200).unwrap(), 1.0);
    assert_eq!(nct.sf(-1e200).unwrap(), 1.0);
    let ncf = stat_core::NoncentralF::new(3.0, 7.0, 2.0).unwrap();
    // The Poisson weights sum to 1 only to rounding, as at any large f.
    assert!((ncf.cdf(1e308).unwrap() - 1.0).abs() < 1e-15);
    assert!(ncf.sf(1e308).unwrap() < 1e-300);
}

#[test]
fn t_tail_at_huge_degrees_of_freedom_takes_the_right_branch() {
    // F-03: ν = 10¹⁹ rounds x = ν/(ν + t²) to 1; the tail is then Φ̄(10).
    let t = StudentsT::new(1e19).unwrap();
    rel_close(t.sf(10.0).unwrap(), 7.619_853_024_160_528e-24, 1e-6);
    assert!(t.two_sided_p_value(10.0).unwrap() <= 1.0);
}

#[test]
fn cauchy_quantiles_keep_relative_precision_in_the_tails() {
    // F-26: tan(π(p − ½)) lost every digit at p = 10⁻²⁰.
    let c = StudentsT::new(1.0).unwrap();
    rel_close(c.quantile(1e-20).unwrap(), -3.183_098_861_837_906_7e19, 1e-10);
    rel_close(c.quantile(1e-10).unwrap(), -3_183_098_861.837_907, 1e-10);
    rel_close(c.quantile(1.0 - 1e-10).unwrap(), 3_183_098_861.837_907, 1e-6);
    rel_close(c.quantile(0.975).unwrap(), 12.706_204_736_174_698, 1e-12);
}

#[test]
fn quantiles_for_tiny_degrees_of_freedom() {
    // F-28: these failed or returned an unconverged bisection midpoint.
    rel_close(ChiSquared::new(0.002).unwrap().quantile(0.6).unwrap(), 1.592_046_763_365_413_8e-222, 1e-8);
    let ncx = Distribution::new(Family::NoncentralChiSquared, &[0.02, 1.0]).unwrap();
    rel_close(ncx.quantile(0.05).unwrap(), 4.630_496_820_128_732e-109, 1e-8);
    let t = StudentsT::new(0.01).unwrap();
    for (p, expected) in [(0.6, 246_458_794.805_543_7), (0.9, 3.960_440_137_152_482e68)] {
        let q = t.quantile(p).unwrap();
        rel_close(q, expected, 1e-8);
        rel_close(t.cdf(q).unwrap(), p, 1e-8);
        rel_close(t.quantile(1.0 - p).unwrap(), -expected, 1e-8);
    }
}

#[test]
fn noncentral_t_far_upper_tail_is_summed_directly() {
    // F-33: 1 − cdf left 6.7e-16 of rounding where the tail is 1.9e-18.
    for (t, df, delta, expected) in [
        (1e9, 2.0, 1.0, 1.924_660_216_656_229_2e-18),
        (1e5, 5.0, 2.0, 4.222_692_877_386_607_7e-23),
        (50.0, 10.0, 1.5, 9.307_274_120_250_486e-12),
    ] {
        let d = NoncentralT::new(df, delta).unwrap();
        rel_close(d.sf(t).unwrap(), expected, 1e-8);
        // Mirror: P(T ≤ −t; −δ) is the same tail.
        rel_close(NoncentralT::new(df, -delta).unwrap().cdf(-t).unwrap(), expected, 1e-8);
    }
}

#[test]
fn noncentral_mixture_stops_when_every_weight_underflows() {
    // F-33: an underflowed total kept the Poisson sum running 10⁶ terms.
    let ncx = stat_core::NoncentralChiSquared::new(4.0, 2.0).unwrap();
    let start = std::time::Instant::now();
    for _ in 0..20 {
        assert_eq!(ncx.cdf(1e-300).unwrap(), 0.0);
    }
    assert!(start.elapsed().as_secs_f64() < 1.0, "took {:?}", start.elapsed());
}

#[test]
fn studentized_range_upper_tail_has_no_quadrature_floor() {
    // F-05: at ν = 10⁶ the cdf topped out at 1 − 2.3·10⁻¹⁰. Reference: SciPy
    // 1.18 `integrate.quad` of f_S(s)·P(R > qs) with P(R > w) from the range
    // integral (`tests/data/studentized_range_large_df.py`); SciPy's own
    // `studentized_range` treats ν this large as infinite.
    let d = StudentizedRange::new(3, 1e6).unwrap();
    rel_close(d.quantile(0.95).unwrap(), 3.314_498_100_842_761, 1e-9);
    rel_close(d.sf(7.0).unwrap(), 2.222_625_505_401_430_3e-6, 1e-8);
    let q = d.quantile(1.0 - 1e-10).unwrap();
    rel_close(q, 9.377_789_865_439_349, 1e-7);
    // The round trip carries the quadrature's rounding in the far tail,
    // which differs slightly between platforms' maths libraries (1.05e-6
    // relative on Windows); a floor would show as an error of order 1.
    rel_close(d.sf(q).unwrap(), 1e-10, 2e-6);
}

#[test]
fn gamma_draws_with_tiny_shapes_are_formed_in_log_space() {
    let mut rng = Rng::new(20_261_019);
    let k = 0.005;
    let gamma = Distribution::new(Family::Gamma, &[k, 1.0]).unwrap();
    let n = 1_000_000;
    let (mut zeros, mut sum) = (0usize, 0.0);
    for _ in 0..n {
        let x = gamma.sample(&mut rng);
        assert!(x.is_finite() && x >= 0.0, "draw {x}");
        zeros += usize::from(x == 0.0);
        sum += x;
    }
    // A draw is 0 only when its true value is below the smallest subnormal,
    // P(G < 2⁻¹⁰⁷⁵) = 0.024166 (mpmath); allow five standard errors.
    let p0 = 0.024_166_194_86;
    let se0 = (p0 * (1.0 - p0) / n as f64).sqrt();
    assert!((zeros as f64 / n as f64 - p0).abs() < 5.0 * se0, "{zeros} zeros");
    // Mean within three standard errors of k (variance k).
    assert!((sum / n as f64 - k).abs() < 3.0 * (k / n as f64).sqrt(), "mean {}", sum / n as f64);
}

#[test]
fn beta_draws_with_tiny_shapes_are_never_nan() {
    // Both gamma draws underflowed together in ≈6·10⁻⁴ of Beta(0.005, 0.005)
    // draws, giving 0/0.
    let mut rng = Rng::new(7);
    let (a, b) = (0.005, 0.005);
    let beta = Distribution::new(Family::Beta, &[a, b]).unwrap();
    let n = 200_000;
    let mut sum = 0.0;
    for _ in 0..n {
        let x = beta.sample(&mut rng);
        assert!((0.0..=1.0).contains(&x), "draw {x}");
        sum += x;
    }
    let mean = a / (a + b);
    let var = a * b / ((a + b) * (a + b) * (a + b + 1.0));
    assert!((sum / n as f64 - mean).abs() < 3.0 * (var / n as f64).sqrt(), "mean {}", sum / n as f64);
    // A shape that keeps every draw strictly inside (0, 1).
    let beta = Distribution::new(Family::Beta, &[0.05, 2.0]).unwrap();
    for _ in 0..100_000 {
        let x = beta.sample(&mut rng);
        assert!(x > 0.0 && x < 1.0, "draw {x}");
    }
}

#[test]
fn rng_gamma_with_tiny_shape_matches_its_mean() {
    let mut rng = Rng::new(11);
    let k = 0.01;
    let n = 400_000;
    let draws: Vec<f64> = (0..n).map(|_| rng.gamma(k)).collect();
    assert!(draws.iter().all(|x| x.is_finite() && *x >= 0.0));
    let mean = draws.iter().sum::<f64>() / n as f64;
    assert!((mean - k).abs() < 3.0 * (k / n as f64).sqrt(), "mean {mean}");
}

#[test]
fn pearson_r_survives_underflowing_cross_products() {
    // F-04: sxx·syy underflowed for data scaled by 10⁻⁹⁰.
    let x = [1.0, 2.0, 3.0, 4.0, 6.0];
    let y = [2.0, 1.0, 4.0, 3.0, 7.0];
    let r = pearson(&x, &y, 0.95).unwrap().correlation.estimate;
    let xs: Vec<f64> = x.iter().map(|v| v * 1e-90).collect();
    let ys: Vec<f64> = y.iter().map(|v| v * 1e-90).collect();
    let rs = pearson(&xs, &ys, 0.95).unwrap().correlation.estimate;
    assert!((rs - r).abs() <= 1e-12, "{rs} vs {r}");
    // Exactly proportional data stay at r = 1 (the single root).
    assert_eq!(pearson(&x, &x.map(|v| 3.0 * v), 0.95).unwrap().correlation.estimate, 1.0);
}

#[test]
fn box_minimiser_declines_an_invalid_box() {
    // F-06: inverted or NaN bounds asserted (an app crash).
    let mut f = |x: &[f64]| x[0] * x[0];
    let m = minimize_in_box(&mut f, &[0.5], &[1.0], &[0.0], 100);
    assert_eq!(m.value, f64::INFINITY);
    assert_eq!(m.evaluations, 0);
    let m = minimize_in_box(&mut f, &[0.5], &[f64::NAN], &[1.0], 100);
    assert_eq!(m.value, f64::INFINITY);
    let m = minimize_in_box(&mut f, &[0.5], &[-1.0], &[1.0], 500);
    assert!(m.value < 1e-10);
}

#[test]
fn box_cox_never_selects_an_undefined_likelihood() {
    // F-29: ∞ − ∞ in the variance gave a NaN that ranked highest on Apple silicon.
    let fit = box_cox_fit(&[1.0, 2.0, 1e100], 0.95).unwrap();
    assert!(fit.lambda.is_finite() && fit.log_likelihood.is_finite(), "{fit:?}");
}

#[test]
fn decompositions_at_extreme_scales() {
    // F-31: Cholesky panicked on a ragged matrix; eigen and SVD overflowed.
    assert!(cholesky(&[vec![1.0], vec![]]).is_none());
    let base = [vec![2.0, 1.0], vec![1.0, 3.0]];
    // Eigenvalues of [[2, 1], [1, 3]]: (5 ± √5)/2.
    let expected = [(5.0 + 5f64.sqrt()) / 2.0, (5.0 - 5f64.sqrt()) / 2.0];
    for scale in [1e200, 1e-200, 1e300, 1e-300] {
        let a: Vec<Vec<f64>> = base.iter().map(|r| r.iter().map(|v| v * scale).collect()).collect();
        let (values, vectors) = symmetric_eigen(&a).unwrap();
        for (v, e) in values.iter().zip(expected) {
            rel_close(*v, e * scale, 1e-10);
        }
        assert!(vectors.iter().flatten().all(|x| x.is_finite()));
        let m = Matrix::from_rows(&a).unwrap();
        let s = svd(&m).unwrap();
        for (v, e) in s.singular_values.iter().zip(expected) {
            rel_close(*v, e * scale, 1e-10);
        }
    }
    let ones = vec![vec![1e200; 2]; 2];
    let (values, _) = symmetric_eigen(&ones).unwrap();
    rel_close(values[0], 2e200, 1e-12);
    assert!(values[1].abs() <= 1e-12 * 2e200);
}

#[test]
fn catalog_variances_at_tiny_shapes() {
    // F-32: Weibull shape 0.01 gave NaN (Γ overflow); here the variance is
    // finite, λ²[Γ(201) − Γ(101)²] at λ = 10⁻¹⁰⁰ (mpmath).
    let w = Distribution::new(Family::Weibull, &[0.01, 1e-100]).unwrap();
    rel_close(w.variance().unwrap(), 7.886_578_673_647_905e174, 1e-10);
    let w = Distribution::new(Family::Weibull, &[0.005, 1.0]).unwrap();
    assert_eq!(w.variance().unwrap(), f64::INFINITY);
    let w = Distribution::new(Family::Weibull, &[2.0, 3.0]).unwrap();
    rel_close(w.variance().unwrap(), 9.0 * (1.0 - std::f64::consts::PI / 4.0), 1e-14);
    // Log-logistic: e^{2μ}[2a/sin 2a − (a/sin a)²], a = πσ, cancelled to 0 for tiny σ.
    for (sigma, expected) in
        [(1e-9, 2.430_902_019_796_737e-17), (0.02, 0.009_751_813_680_454_534), (0.1, 0.261_579_539_099_219_6)]
    {
        let d = Distribution::new(Family::LogLogistic, &[1.0, sigma]).unwrap();
        rel_close(d.variance().unwrap(), expected, 1e-12);
    }
}
