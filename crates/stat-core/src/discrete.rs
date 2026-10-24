//! Binomial, Poisson and hypergeometric probabilities, and the beta quantile
//! used for exact binomial confidence limits.
//!
//! Cumulative probabilities use the incomplete beta and gamma identities
//! (Abramowitz & Stegun 26.5.24, 6.5.29):
//!
//! * Binomial(`n`, `p`): `P(X ≤ k) = I_{1−p}(n − k, k + 1)`.
//! * Poisson(`μ`): `P(X ≤ k) = Q(k + 1, μ)`.
//!
//! Binomial and Poisson point probabilities use Loader's (2000) saddle-point
//! form ([`crate::special::beta_prefactor`], [`poisson_probability`]), which
//! stays accurate for counts in the billions; hypergeometric ones are
//! evaluated in log space through `ln Γ`.

use crate::error::{Result, invalid};
use crate::special::{
    beta_prefactor, ln_gamma, poisson_probability, regularized_gamma_p, regularized_gamma_q,
    regularized_incomplete_beta_pair,
};

fn ln_choose(n: f64, k: f64) -> f64 {
    ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0)
}

fn check_binomial(n: u64, p: f64) -> Result<()> {
    if (0.0..=1.0).contains(&p) && n > 0 {
        Ok(())
    } else {
        Err(invalid(format!("binomial needs n ≥ 1 and 0 ≤ p ≤ 1 (got n = {n}, p = {p})")))
    }
}

/// Binomial `P(X = k)`.
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] unless `n ≥ 1` and `0 ≤ p ≤ 1`.
pub fn binomial_pmf(k: u64, n: u64, p: f64) -> Result<f64> {
    check_binomial(n, p)?;
    if k > n {
        return Ok(0.0);
    }
    if p == 0.0 {
        return Ok(if k == 0 { 1.0 } else { 0.0 });
    }
    if p == 1.0 {
        return Ok(if k == n { 1.0 } else { 0.0 });
    }
    let (n, k) = (n as f64, k as f64);
    if k == 0.0 {
        return Ok((n * (-p).ln_1p()).exp());
    }
    if k == n {
        return Ok((n * p.ln()).exp());
    }
    // C(n, k) pᵏ qⁿ⁻ᵏ = [pᵏ qⁿ⁻ᵏ / B(k, n − k)] · n / (k (n − k)), with the
    // prefactor in Loader's (2000) saddle-point form for large counts.
    Ok(beta_prefactor(k, n - k, p, 1.0 - p) * n / (k * (n - k)))
}

/// Binomial `P(X ≤ k)`.
///
/// # Errors
/// As [`binomial_pmf`].
pub fn binomial_cdf(k: u64, n: u64, p: f64) -> Result<f64> {
    check_binomial(n, p)?;
    if k >= n {
        return Ok(1.0);
    }
    if p == 0.0 {
        return Ok(1.0);
    }
    if p == 1.0 {
        return Ok(0.0);
    }
    regularized_incomplete_beta_pair((n - k) as f64, k as f64 + 1.0, 1.0 - p, p)
}

/// Binomial `P(X ≥ k)`.
///
/// # Errors
/// As [`binomial_pmf`].
pub fn binomial_sf_inclusive(k: u64, n: u64, p: f64) -> Result<f64> {
    check_binomial(n, p)?;
    if k == 0 {
        return Ok(1.0);
    }
    if k > n {
        return Ok(0.0);
    }
    if p == 0.0 {
        return Ok(0.0);
    }
    if p == 1.0 {
        return Ok(1.0);
    }
    regularized_incomplete_beta_pair(k as f64, (n - k) as f64 + 1.0, p, 1.0 - p)
}

/// Poisson `P(X ≤ k)` for mean `μ ≥ 0`.
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for negative or non-finite `μ`.
pub fn poisson_cdf(k: u64, mean: f64) -> Result<f64> {
    if !(mean >= 0.0 && mean.is_finite()) {
        return Err(invalid(format!("Poisson mean must be non-negative (got {mean})")));
    }
    if mean == 0.0 {
        return Ok(1.0);
    }
    regularized_gamma_q(k as f64 + 1.0, mean)
}

/// Poisson `P(X ≥ k)`.
///
/// # Errors
/// As [`poisson_cdf`].
pub fn poisson_sf_inclusive(k: u64, mean: f64) -> Result<f64> {
    if !(mean >= 0.0 && mean.is_finite()) {
        return Err(invalid(format!("Poisson mean must be non-negative (got {mean})")));
    }
    if k == 0 {
        return Ok(1.0);
    }
    if mean == 0.0 {
        return Ok(0.0);
    }
    regularized_gamma_p(k as f64, mean)
}

/// Poisson `P(X = k)`.
///
/// # Errors
/// As [`poisson_cdf`].
pub fn poisson_pmf(k: u64, mean: f64) -> Result<f64> {
    if !(mean >= 0.0 && mean.is_finite()) {
        return Err(invalid(format!("Poisson mean must be non-negative (got {mean})")));
    }
    if mean == 0.0 {
        return Ok(if k == 0 { 1.0 } else { 0.0 });
    }
    Ok(poisson_probability(k as f64, mean))
}

/// Hypergeometric `P(X = k)`: `k` successes in `draws` draws without
/// replacement from `total` items of which `successes` are successes.
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] for inconsistent counts.
pub fn hypergeometric_pmf(k: u64, total: u64, successes: u64, draws: u64) -> Result<f64> {
    if successes > total || draws > total {
        return Err(invalid("hypergeometric counts exceed the population size"));
    }
    if k > successes || k > draws || draws - k > total - successes {
        return Ok(0.0);
    }
    let (n, m, d, k) = (total as f64, successes as f64, draws as f64, k as f64);
    Ok((ln_choose(m, k) + ln_choose(n - m, d - k) - ln_choose(n, d)).exp())
}

/// The `x` with `I_x(a, b) = p` (beta distribution quantile), by bisection
/// to full double precision.
///
/// # Errors
/// [`crate::StatError::InvalidArgument`] unless `a, b > 0` and `0 ≤ p ≤ 1`.
pub fn beta_quantile(a: f64, b: f64, p: f64) -> Result<f64> {
    if !(a > 0.0 && b > 0.0 && (0.0..=1.0).contains(&p)) {
        return Err(invalid(format!("beta quantile needs a, b > 0 and 0 ≤ p ≤ 1 (got {a}, {b}, {p})")));
    }
    if p == 0.0 {
        return Ok(0.0);
    }
    if p == 1.0 {
        return Ok(1.0);
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..1100 {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi {
            break;
        }
        if regularized_incomplete_beta_pair(a, b, mid, 1.0 - mid)? < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(0.5 * (lo + hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs(), "{a} vs {b}");
    }

    /// Reference values: SciPy 1.18 `binom`, `poisson`, `hypergeom`, `beta.ppf`.
    #[test]
    fn matches_reference() {
        close(binomial_cdf(7, 20, 0.3).unwrap(), 0.772_271_797_418_160_8, 1e-13);
        close(binomial_sf_inclusive(8, 20, 0.3).unwrap(), 0.227_728_202_581_839_26, 1e-13);
        close(binomial_pmf(7, 20, 0.3).unwrap(), 0.164_261_985_217_236_6, 1e-13);
        close(poisson_cdf(4, 6.5).unwrap(), 0.223_671_816_811_499_45, 1e-13);
        close(poisson_sf_inclusive(5, 6.5).unwrap(), 0.776_328_183_188_500_5, 1e-13);
        close(hypergeometric_pmf(3, 20, 7, 10).unwrap(), 0.325_077_399_380_805, 1e-13);
        close(beta_quantile(4.0, 17.0, 0.025).unwrap(), 0.057_333_997_050_032_77, 1e-12);
        close(beta_quantile(5.0, 16.0, 0.975).unwrap(), 0.436_614_002_996_668_3, 1e-12);
        // Probabilities sum to one.
        let total: f64 = (0..=20).map(|k| binomial_pmf(k, 20, 0.3).unwrap()).sum();
        close(total, 1.0, 1e-14);
        let total: f64 = (0..=7).map(|k| hypergeometric_pmf(k, 20, 7, 10).unwrap()).sum();
        close(total, 1.0, 1e-14);
        assert_eq!(binomial_cdf(20, 20, 0.3).unwrap(), 1.0);
        assert!(binomial_pmf(1, 0, 0.5).is_err());
    }
}
