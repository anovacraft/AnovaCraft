use super::*;
use crate::capability::{CapabilityOptions, process_capability};

fn rings() -> Vec<f64> {
    vec![
        74.030, 74.002, 74.019, 73.992, 74.008, 73.995, 73.992, 74.001, 74.011, 74.004, 73.988, 74.024, 74.021, 74.005,
        74.002, 74.002, 73.996, 73.993, 74.015, 74.009,
    ]
}

/// `n` values at the midpoints `(i − ½)/n` of a distribution's quantile function.
fn quantile_sample(n: usize, q: impl Fn(f64) -> f64) -> Vec<f64> {
    (1..=n).map(|i| q((i as f64 - 0.5) / n as f64)).collect()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-300)
}

#[test]
fn normal_fit_reproduces_normal_theory_indices() {
    let x = rings();
    let limits = SpecLimits { lower: Some(73.95), upper: Some(74.05), target: None };
    let p = percentile_capability(&x, &limits, CapabilityModel::Distribution(FitFamily::Normal)).unwrap();
    let c = process_capability(&x, &limits, &CapabilityOptions::default()).unwrap();
    // Φ⁻¹(0.99865) = 2.99998 rather than 3, so the indices agree to about 1e-5.
    assert!(close(p.pp.unwrap(), c.overall.cp.unwrap(), 1e-4), "{} {}", p.pp.unwrap(), c.overall.cp.unwrap());
    assert!(close(p.ppk, c.overall.cpk, 1e-4));
    assert!(close(p.expected_ppm.0, c.overall.expected_ppm.0, 1e-9));
    assert!(close(p.expected_ppm.1, c.overall.expected_ppm.1, 1e-9));
    // The equivalent Ppk of a normal fit is its Ppk (up to the rounding of 3).
    assert!(close(p.ppk_equivalent.unwrap(), c.overall.cpk, 1e-3));
}

#[test]
fn exponential_closed_forms() {
    let x = quantile_sample(200, |p| -2.0 * (-p).ln_1p());
    let theta = x.iter().sum::<f64>() / x.len() as f64;
    let usl = 15.0;
    let limits = SpecLimits { lower: None, upper: Some(usl), target: None };
    let p = percentile_capability(&x, &limits, CapabilityModel::Distribution(FitFamily::Exponential)).unwrap();
    let q = |pr: f64| -theta * (-pr).ln_1p();
    for (got, pr) in p.percentiles.iter().zip(REFERENCE_PROBABILITIES) {
        assert!(close(*got, q(pr), 1e-9), "{got} {}", q(pr));
    }
    let ppu = (usl - q(0.5)) / (q(0.99865) - q(0.5));
    assert!(close(p.ppu.unwrap(), ppu, 1e-9));
    assert_eq!(p.ppk, p.ppu.unwrap());
    assert!(p.pp.is_none() && p.ppl.is_none());
    let above = (-usl / theta).exp();
    assert!(close(p.expected_ppm.1, 1e6 * above, 1e-9));
    assert_eq!(p.expected_ppm.0, 0.0);
    let z = Normal::standard().quantile(1.0 - above).unwrap();
    assert!(close(p.ppk_equivalent.unwrap(), z / 3.0, 1e-9));
}

#[test]
fn weibull_data_best_fit_and_johnson() {
    // Weibull with shape 2 and scale 10, by its quantile function.
    let weibull = |p: f64| 10.0 * (-(-p).ln_1p()).sqrt();
    let x = quantile_sample(500, weibull);
    let limits = SpecLimits { lower: Some(0.5), upper: Some(28.0), target: None };
    let w = percentile_capability(&x, &limits, CapabilityModel::Distribution(FitFamily::Weibull)).unwrap();
    // The fit recovers the distribution, so the percentiles are close to the true ones.
    for (got, pr) in w.percentiles.iter().zip(REFERENCE_PROBABILITIES) {
        assert!(close(*got, weibull(pr), 0.03), "{pr}: {got} vs {}", weibull(pr));
    }
    let true_ppu = (28.0 - weibull(0.5)) / (weibull(0.99865) - weibull(0.5));
    assert!(close(w.ppu.unwrap(), true_ppu, 0.03));
    // Expected below LSL ≈ 1 − exp(−(0.05)²) = 2498 ppm.
    assert!(close(w.expected_ppm.0, 1e6 * (-(-(0.05f64).powi(2)).exp_m1()), 0.1), "{:?}", w.expected_ppm);
    // A normal fit to the same right-skewed data badly misjudges both tails:
    // far too much below LSL, far too little above USL (Weibull: 394 ppm).
    let n = percentile_capability(&x, &limits, CapabilityModel::Distribution(FitFamily::Normal)).unwrap();
    assert!(n.expected_ppm.0 > 10.0 * w.expected_ppm.0, "{:?} {:?}", n.expected_ppm, w.expected_ppm);
    assert!(n.expected_ppm.1 < 0.1 * w.expected_ppm.1, "{:?} {:?}", n.expected_ppm, w.expected_ppm);

    // Best fit: every family that fits is ranked by AIC; the winner is a Weibull form.
    let b = percentile_capability(&x, &limits, CapabilityModel::BestFit).unwrap();
    assert!(b.candidates.len() >= 10);
    assert!(b.candidates.windows(2).all(|c| c[0].1 <= c[1].1));
    let PercentileSource::Fitted(best) = &b.source else { panic!() };
    assert_eq!(best.family, b.candidates[0].0);
    assert!(matches!(best.family, FitFamily::Weibull | FitFamily::Weibull3 | FitFamily::Gamma3), "{:?}", best.family);

    // Johnson: its percentiles invert its CDF and sit near the true ones.
    let j = percentile_capability(&x, &limits, CapabilityModel::Johnson).unwrap();
    for (got, pr) in j.percentiles.iter().zip(REFERENCE_PROBABILITIES) {
        assert!((j.cdf(*got).unwrap() - pr).abs() < 1e-9);
        // Within 15% of the scale: the Johnson tails are extrapolated from four percentiles.
        assert!((got - weibull(pr)).abs() < 1.5, "{pr}: {got} vs {}", weibull(pr));
    }
    // The density integrates to about one over the data range.
    let pdf_area: f64 = (0..4000).map(|i| j.pdf(0.01 * (f64::from(i) + 0.5)).unwrap() * 0.01).sum();
    assert!((pdf_area - 1.0).abs() < 0.01, "{pdf_area}");
}

#[test]
fn nonparametric_percentiles_and_observed_rates() {
    let x: Vec<f64> = (0..1000).map(|i| f64::from(i) / 999.0).collect();
    let limits = SpecLimits { lower: Some(0.01), upper: Some(0.995), target: None };
    let p = percentile_capability(&x, &limits, CapabilityModel::Nonparametric).unwrap();
    // Type 6: position p(n + 1), so the median is the mean of the 500th and 501st values.
    assert!(close(p.percentiles[1], 0.5, 1e-12));
    assert!(close(p.percentiles[0], (0.00135 * 1001.0 - 1.0) / 999.0, 1e-9));
    // 10 values below 0.01 (0 … 9/999), 5 above 0.995.
    assert!(close(p.expected_ppm.0, 10_000.0, 1e-12));
    assert!(close(p.expected_ppm.1, 5_000.0, 1e-12));
    assert!(p.cdf(0.5).is_none() && p.pdf(0.5).is_none());
}

#[test]
fn errors_and_keys() {
    let x = rings();
    let limits = SpecLimits { lower: Some(73.95), upper: Some(74.05), target: None };
    assert!(percentile_capability(&x, &limits, CapabilityModel::Normal).is_err());
    assert!(percentile_capability(&x, &SpecLimits::default(), CapabilityModel::Johnson).is_err());
    let backwards = SpecLimits { lower: Some(74.05), upper: Some(73.95), target: None };
    assert!(percentile_capability(&x, &backwards, CapabilityModel::BestFit).is_err());
    assert!(percentile_capability(&x[..8], &limits, CapabilityModel::Nonparametric).is_err());
    let negative = [-1.0, 2.0, 3.0, 4.0, 5.0];
    assert!(percentile_capability(&negative, &limits, CapabilityModel::Distribution(FitFamily::Weibull)).is_err());
    assert!(percentile_capability(&[1.0; 12], &limits, CapabilityModel::Nonparametric).is_err());
    for m in [
        CapabilityModel::Normal,
        CapabilityModel::BestFit,
        CapabilityModel::Johnson,
        CapabilityModel::Nonparametric,
        CapabilityModel::Distribution(FitFamily::Gamma3),
    ] {
        assert_eq!(CapabilityModel::from_key(m.key()), Some(m));
    }
    assert_eq!(CapabilityModel::from_key("nonsense"), None);
}

/// For a normal sample, bootstrap percentile intervals under a normal fit
/// should approximate the normal-theory intervals: the χ² interval for `Pp`
/// and Bissell's interval for `Ppk` (an independent analytic reference).
#[test]
fn bootstrap_intervals_match_normal_theory_for_normal_data() {
    let mut rng = stat_core::rng::Rng::new(20_261_015);
    let x: Vec<f64> = (0..200).map(|_| 10.0 + rng.normal()).collect();
    let limits = SpecLimits { lower: Some(6.5), upper: Some(14.0), target: None };
    let p = percentile_capability(&x, &limits, CapabilityModel::Distribution(FitFamily::Normal)).unwrap();
    let c = process_capability(&x, &limits, &CapabilityOptions::default()).unwrap();
    let b = bootstrap_percentile_intervals(&x, &limits, &p, 2000, 7, 0.95).unwrap();
    assert_eq!((b.resamples, b.used, b.seed), (2000, 2000, 7));
    let (lo, hi) = b.ppk;
    let (blo, bhi) = c.overall.cpk_interval;
    assert!(lo < p.ppk && p.ppk < hi);
    assert!((lo - blo).abs() < 0.04 && (hi - bhi).abs() < 0.04, "bootstrap ({lo}, {hi}) vs Bissell ({blo}, {bhi})");
    let (lo, hi) = b.pp.unwrap();
    let (clo, chi) = c.overall.cp_interval.unwrap();
    assert!((lo - clo).abs() < 0.04 && (hi - chi).abs() < 0.04, "bootstrap ({lo}, {hi}) vs χ² ({clo}, {chi})");
    assert!(b.ppl.is_some() && b.ppu.is_some());
    // The same seed reproduces the intervals; another seed moves them a little.
    let again = bootstrap_percentile_intervals(&x, &limits, &p, 2000, 7, 0.95).unwrap();
    assert_eq!(again, b);
    let other = bootstrap_percentile_intervals(&x, &limits, &p, 2000, 8, 0.95).unwrap();
    assert_ne!(other.ppk, b.ppk);
    assert!((other.ppk.0 - b.ppk.0).abs() < 0.03);
}

#[test]
fn bootstrap_with_one_limit_best_fit_and_errors() {
    let x = quantile_sample(80, |u| 4.0 * (-(-u).ln_1p()).powf(1.0 / 1.5));
    let limits = SpecLimits { lower: None, upper: Some(16.0), target: None };
    let best = percentile_capability(&x, &limits, CapabilityModel::BestFit).unwrap();
    let b = bootstrap_percentile_intervals(&x, &limits, &best, 200, 1, 0.90).unwrap();
    assert!(b.pp.is_none() && b.ppl.is_none() && b.ppu.is_some());
    assert!(b.ppk.0 < best.ppk && best.ppk < b.ppk.1, "{:?} {}", b.ppk, best.ppk);
    let np = percentile_capability(&x, &limits, CapabilityModel::Nonparametric).unwrap();
    assert!(bootstrap_percentile_intervals(&x, &limits, &np, 100, 1, 0.95).is_ok());
    assert!(bootstrap_percentile_intervals(&x, &limits, &best, 10, 1, 0.95).is_err(), "too few resamples");
    assert!(bootstrap_percentile_intervals(&x, &limits, &best, 100, 1, 1.0).is_err(), "bad level");
}
