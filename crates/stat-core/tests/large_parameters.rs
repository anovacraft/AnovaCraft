//! Accuracy of distribution functions at very large parameters (degrees of
//! freedom, shapes, counts and means from 10⁴ to 10¹⁰), where `ln Γ` values
//! of order 10⁷–10¹⁰ cancel in the normalizing constants.
//!
//! Reference values in `data/large_parameters.csv` come from direct numerical
//! quadrature of each density with mpmath at 40 significant digits
//! (`data/large_parameters.py`), independent of the series, continued
//! fractions and expansions under test.
//!
//! Every row must agree to 1e-12 relative error, or to `4 κ ε` where the code
//! itself rounds a transformed argument (`1 − p` for binomial cdfs, the beta
//! argument of an F tail) and the value's condition number `κ` with respect
//! to it (column 6) makes that unavoidable rounding cost more — the same
//! representation-floor principle as the StRD Filip exception in CLAUDE.md.

use stat_core::discrete::{binomial_cdf, binomial_pmf, poisson_cdf, poisson_pmf};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor, StudentsT};

/// Relative error every row must meet when no rounded argument limits it.
const TOLERANCE: f64 = 1e-12;

fn computed(kind: &str, a: f64, b: f64, c: f64) -> f64 {
    match kind {
        "chi2_sf" => ChiSquared::new(a).unwrap().sf(b).unwrap_or(f64::NAN),
        "chi2_cdf" => ChiSquared::new(a).unwrap().cdf(b).unwrap_or(f64::NAN),
        "chi2_pdf" => ChiSquared::new(a).unwrap().pdf(b),
        "t_p2" => StudentsT::new(a).unwrap().two_sided_p_value(b).unwrap_or(f64::NAN),
        "t_pdf" => StudentsT::new(a).unwrap().pdf(b),
        "f_sf" => FisherSnedecor::new(a, b).unwrap().sf(c).unwrap_or(f64::NAN),
        "f_pdf" => FisherSnedecor::new(a, b).unwrap().pdf(c),
        "pois_cdf" => poisson_cdf(a as u64, b).unwrap_or(f64::NAN),
        "pois_pmf" => poisson_pmf(a as u64, b).unwrap_or(f64::NAN),
        "binom_cdf" => binomial_cdf(a as u64, b as u64, c).unwrap_or(f64::NAN),
        "binom_pmf" => binomial_pmf(a as u64, b as u64, c).unwrap_or(f64::NAN),
        other => panic!("unknown reference kind {other}"),
    }
}

#[test]
fn distribution_functions_match_quadrature_at_large_parameters() {
    let mut failures = Vec::new();
    let mut worst = 0.0_f64;
    for line in include_str!("data/large_parameters.csv").lines() {
        let fields: Vec<&str> = line.split(',').collect();
        let [kind, a, b, c, expected, kappa] = fields[..] else { panic!("bad row {line}") };
        let parse = |s: &str| s.parse::<f64>().unwrap();
        let (a, b, c, expected, kappa) = (parse(a), parse(b), parse(c), parse(expected), parse(kappa));
        assert!(expected > 0.0, "reference {line} must be positive");
        let got = computed(kind, a, b, c);
        let rel = ((got - expected) / expected).abs();
        let tolerance = TOLERANCE.max(4.0 * kappa * f64::EPSILON);
        if rel / tolerance > 0.3 {
            eprintln!("{kind}({a}, {b}, {c}): rel {rel:.1e}, allowed {tolerance:.1e}");
        }
        worst = worst.max(rel / tolerance);
        if rel.is_nan() || rel > tolerance {
            failures.push(format!(
                "{kind}({a}, {b}, {c}) = {got:e}, expected {expected:e} (rel {rel:.1e}, allowed {tolerance:.1e})"
            ));
        }
    }
    assert!(failures.is_empty(), "{} rows outside tolerance:\n{}", failures.len(), failures.join("\n"));
    eprintln!("worst error {worst:.2} of its tolerance");
}
