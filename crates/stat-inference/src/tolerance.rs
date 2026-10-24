//! Tolerance intervals: limits that contain at least a proportion `p` of the
//! population with confidence `γ` (Krishnamoorthy & Mathew, *Statistical
//! Tolerance Regions*, 2009, chs. 2 and 8; NIST/SEMATECH e-Handbook §7.2.6).
//!
//! **Normal population**, from the sample mean `x̄` and standard deviation `s`
//! of `n` values, limits `x̄ ± k·s`:
//!
//! * **One-sided** (a lower bound `x̄ − k₁s` or an upper bound `x̄ + k₁s`): the
//!   exact factor is `k₁ = t′_γ(n − 1, δ)/√n`, the `γ` quantile of the
//!   noncentral t with `n − 1` df and noncentrality `δ = z_p√n` (Owen 1963;
//!   K&M §2.2).
//! * **Two-sided** (`x̄ ± k₂s` covering a central-or-not proportion `p`): the
//!   exact factor solves (Wald & Wolfowitz 1946; K&M eq. 2.3.5)
//!
//!   ```text
//!   √(2n/π) ∫₀^∞ P(χ²_{n−1} > (n−1)·r(z)²/k²) e^{−nz²/2} dz = γ,
//!   ```
//!
//!   where `r(z)` solves `Φ(z + r) − Φ(z − r) = p`. The integral is evaluated
//!   by composite Gauss–Legendre quadrature over `[0, 12/√n]` and the factor
//!   by root finding.
//!
//! **Distribution-free** (Wilks 1941; K&M ch. 8): for continuous data the
//! content of `[x₍ᵣ₎, x₍ₙ₊₁₋ₛ₎]` has a Beta(`n − m + 1`, `m`) distribution,
//! `m = r + s`, so the interval covers at least `p` with confidence
//! `P(Binomial(n, p) ≤ n − m)`. The interval uses the largest `m` reaching
//! `γ` (split as evenly as possible between the two ends, or entirely on the
//! bounded end of a one-sided interval). When even the sample extremes do not
//! reach `γ`, the extremes are returned with the confidence they do achieve.

use crate::common::{check_finite, increasing_root, invalid, mean_var};
use stat_core::discrete::binomial_cdf;
use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::quadrature::gauss_legendre;
use stat_core::{NoncentralT, Result, StatError};

/// Which limits a tolerance interval has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ToleranceSides {
    /// Lower and upper limits.
    #[default]
    TwoSided,
    /// A lower limit only: at least `p` of the population lies above it.
    Lower,
    /// An upper limit only: at least `p` of the population lies below it.
    Upper,
}

impl ToleranceSides {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::TwoSided => "two-sided",
            Self::Lower => "lower limit",
            Self::Upper => "upper limit",
        }
    }
}

/// A tolerance interval.
#[derive(Debug, Clone, PartialEq)]
pub struct ToleranceInterval {
    /// How it was computed.
    pub method: &'static str,
    /// Limits present.
    pub sides: ToleranceSides,
    /// Number of values.
    pub n: usize,
    /// Proportion of the population the interval is to contain.
    pub proportion: f64,
    /// Confidence requested.
    pub confidence: f64,
    /// Confidence the interval achieves: equal to `confidence` for the normal
    /// intervals; at least it (or below it, when the sample is too small) for
    /// the distribution-free interval.
    pub achieved_confidence: f64,
    /// Lower limit (−∞ for an upper limit only).
    pub lower: f64,
    /// Upper limit (+∞ for a lower limit only).
    pub upper: f64,
    /// Sample mean (normal intervals; NaN otherwise).
    pub mean: f64,
    /// Sample standard deviation (normal intervals; NaN otherwise).
    pub std_dev: f64,
    /// Factor `k` in `x̄ ± k·s` (normal intervals; NaN otherwise).
    pub factor: f64,
    /// Ranks `(r, n + 1 − s)` of the order statistics used (distribution-free
    /// interval; `0` or `n + 1` marks an unbounded side). `None` for normal intervals.
    pub order_statistics: Option<(usize, usize)>,
}

fn check_inputs(proportion: f64, confidence: f64) -> Result<()> {
    if !(proportion > 0.0 && proportion < 1.0) {
        return Err(invalid(format!("the proportion covered must lie strictly between 0 and 1 (got {proportion})")));
    }
    if !(confidence > 0.0 && confidence < 1.0) {
        return Err(invalid(format!("the confidence must lie strictly between 0 and 1 (got {confidence})")));
    }
    Ok(())
}

/// Exact one-sided normal tolerance factor `k₁` for `n` values: a bound
/// `x̄ − k₁s` (or `x̄ + k₁s`) exceeded by (or exceeding) at least `proportion`
/// of the population with confidence `confidence`.
///
/// # Errors
/// [`StatError::InsufficientData`] for `n < 2`; [`StatError::InvalidArgument`]
/// for a proportion or confidence outside (0, 1).
pub fn one_sided_tolerance_factor(n: usize, proportion: f64, confidence: f64) -> Result<f64> {
    check_inputs(proportion, confidence)?;
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    let nf = n as f64;
    let delta = Normal::standard().quantile(proportion)? * nf.sqrt();
    let dist = NoncentralT::new(nf - 1.0, delta)?;
    // The quantile lies above δ for γ ≥ ½ only roughly; bracket from below.
    let start = delta.min(0.0) - 10.0;
    let t = increasing_root(|t| Ok(dist.cdf(t)? - confidence), start, delta.abs() + 10.0, "noncentral t quantile")?;
    Ok(t / nf.sqrt())
}

/// Exact two-sided normal tolerance factor `k₂` for `n` values: `x̄ ± k₂s`
/// contains at least `proportion` of the population with confidence
/// `confidence`.
///
/// # Errors
/// As [`one_sided_tolerance_factor`].
pub fn two_sided_tolerance_factor(n: usize, proportion: f64, confidence: f64) -> Result<f64> {
    check_inputs(proportion, confidence)?;
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    let coverage = two_sided_content(n, proportion)?;
    increasing_root(|k| Ok(coverage(k)? - confidence), 0.01, 10.0, "two-sided tolerance factor")
}

/// The probability, over samples of `n` normal values, that the limits
/// `x̄ ± k·s` (two-sided) or the one limit `x̄ − k·s` (or `x̄ + k·s`) contain
/// at least `proportion` of the population — the confidence of a tolerance
/// interval with factor `k` (inverting it gives the factors above). Used to
/// choose sample sizes (Odeh & Owen 1980; Faulkenberry & Weeks 1968,
/// *Technometrics* 10): the probability that the content exceeds a
/// `proportion` larger than the one required should be small.
///
/// # Errors
/// [`StatError::InsufficientData`] for `n < 2`; [`StatError::InvalidArgument`]
/// for a proportion outside (0, 1) or `k ≤ 0`.
pub fn tolerance_content_probability(n: usize, proportion: f64, factor: f64, two_sided: bool) -> Result<f64> {
    check_inputs(proportion, 0.5)?;
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    if !(factor > 0.0 && factor.is_finite()) {
        return Err(invalid(format!("the tolerance factor must be positive (got {factor})")));
    }
    if two_sided {
        two_sided_content(n, proportion)?(factor)
    } else {
        // P(x̄ − ks ≤ μ − z_p σ) = P(T′(n − 1, z_p√n) ≤ k√n).
        let nf = n as f64;
        let delta = Normal::standard().quantile(proportion)? * nf.sqrt();
        NoncentralT::new(nf - 1.0, delta)?.cdf(factor * nf.sqrt())
    }
}

/// `k ↦ P(the content of x̄ ± k·s is at least proportion)` for `n` values
/// (Wald & Wolfowitz's integral; see the module notes).
fn two_sided_content(n: usize, proportion: f64) -> Result<impl Fn(f64) -> Result<f64>> {
    let nf = n as f64;
    let nu = nf - 1.0;
    let chi = ChiSquared::new(nu)?;
    let std = Normal::standard();
    // Composite Gauss–Legendre nodes over [0, 12/√n], with r(z)² at each node.
    let upper = 12.0 / nf.sqrt();
    let (base_nodes, base_weights) = gauss_legendre(16);
    let panels = 12;
    let half = 0.5 * upper / panels as f64;
    let mut terms = Vec::with_capacity(16 * panels);
    for p in 0..panels {
        let mid = half * (2 * p + 1) as f64;
        for (x, w) in base_nodes.iter().zip(&base_weights) {
            let z = mid + half * x;
            let content = |r: f64| -> Result<f64> { Ok(std.cdf(z + r)? - std.cdf(z - r)? - proportion) };
            let r = increasing_root(content, 0.0, z + 10.0, "tolerance content radius")?;
            terms.push((half * w * (-0.5 * nf * z * z).exp(), nu * r * r));
        }
    }
    let scale = (2.0 * nf / std::f64::consts::PI).sqrt();
    Ok(move |k: f64| -> Result<f64> {
        let mut total = 0.0;
        for &(w, a) in &terms {
            total += w * chi.sf(a / (k * k))?;
        }
        Ok(scale * total)
    })
}

/// Normal-theory tolerance interval for the population sampled by `x`.
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than two values;
/// [`StatError::NonFiniteInput`] for NaN/±∞; [`StatError::InvalidArgument`]
/// for a proportion or confidence outside (0, 1) or values without variation.
pub fn normal_tolerance_interval(
    x: &[f64],
    proportion: f64,
    confidence: f64,
    sides: ToleranceSides,
) -> Result<ToleranceInterval> {
    check_finite(x, "sample values")?;
    if x.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: x.len() });
    }
    let (mean, var) = mean_var(x);
    if var <= 0.0 {
        return Err(invalid("the values have no variation"));
    }
    let s = var.sqrt();
    let n = x.len();
    let factor = match sides {
        ToleranceSides::TwoSided => two_sided_tolerance_factor(n, proportion, confidence)?,
        _ => one_sided_tolerance_factor(n, proportion, confidence)?,
    };
    let (lower, upper) = match sides {
        ToleranceSides::TwoSided => (mean - factor * s, mean + factor * s),
        ToleranceSides::Lower => (mean - factor * s, f64::INFINITY),
        ToleranceSides::Upper => (f64::NEG_INFINITY, mean + factor * s),
    };
    Ok(ToleranceInterval {
        method: "Normal (exact factor)",
        sides,
        n,
        proportion,
        confidence,
        achieved_confidence: confidence,
        lower,
        upper,
        mean,
        std_dev: s,
        factor,
        order_statistics: None,
    })
}

/// Distribution-free tolerance interval from order statistics (Wilks 1941).
///
/// # Errors
/// [`StatError::InsufficientData`] for an empty sample (two values for a
/// two-sided interval); [`StatError::NonFiniteInput`] for NaN/±∞;
/// [`StatError::InvalidArgument`] for a proportion or confidence outside (0, 1).
pub fn nonparametric_tolerance_interval(
    x: &[f64],
    proportion: f64,
    confidence: f64,
    sides: ToleranceSides,
) -> Result<ToleranceInterval> {
    check_inputs(proportion, confidence)?;
    check_finite(x, "sample values")?;
    let n = x.len();
    let min_m = if sides == ToleranceSides::TwoSided { 2 } else { 1 };
    if n < min_m {
        return Err(StatError::InsufficientData { required: min_m, found: n });
    }
    let conf = |m: usize| binomial_cdf((n - m) as u64, n as u64, proportion);
    // Largest m ∈ [min_m, n] with confidence ≥ γ (confidence falls as m grows).
    let mut m = min_m;
    while m < n && conf(m + 1)? >= confidence {
        m += 1;
    }
    let achieved = conf(m)?;
    let mut sorted = x.to_vec();
    sorted.sort_by(f64::total_cmp);
    let (r, s) = match sides {
        ToleranceSides::TwoSided => (m / 2, m - m / 2),
        ToleranceSides::Lower => (m, 0),
        ToleranceSides::Upper => (0, m),
    };
    let lower = if r == 0 { f64::NEG_INFINITY } else { sorted[r - 1] };
    let upper = if s == 0 { f64::INFINITY } else { sorted[n - s] };
    Ok(ToleranceInterval {
        method: "Distribution-free (order statistics)",
        sides,
        n,
        proportion,
        confidence,
        achieved_confidence: achieved,
        lower,
        upper,
        mean: f64::NAN,
        std_dev: f64::NAN,
        factor: f64::NAN,
        order_statistics: Some((r, n + 1 - s)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    /// NIST/SEMATECH e-Handbook §7.2.6.3: n = 43, p = 0.90, γ = 0.99 gives
    /// k₁ = 1.8740 by the noncentral t; n = 6 gives 4.4111. Further values from
    /// SciPy 1.18 (`nct.ppf(γ, n−1, z_p√n)/√n`).
    #[test]
    fn one_sided_factors_match_nist_and_scipy() {
        let cases = [
            (43, 0.90, 0.99, 1.8739536058511987),
            (6, 0.90, 0.99, 4.411080572397619),
            (10, 0.90, 0.95, 2.354640131829059),
            (20, 0.95, 0.95, 2.3960016837521687),
            (5, 0.99, 0.99, 8.939024938893024),
        ];
        for (n, p, g, k) in cases {
            let got = one_sided_tolerance_factor(n, p, g).unwrap();
            assert!(close(got, k, 1e-8), "n={n} p={p} γ={g}: {got} vs {k}");
        }
        assert!(close(one_sided_tolerance_factor(43, 0.90, 0.99).unwrap(), 1.8740, 5e-5));
        assert!(close(one_sided_tolerance_factor(6, 0.90, 0.99).unwrap(), 4.4111, 5e-5));
    }

    /// Exact two-sided factors against an independent SciPy computation of
    /// the same integral (adaptive `quad`, `ncx2.ppf` for r(z), `brentq`).
    /// NIST's approximate k₂ for n = 43, p = 0.90, γ = 0.99 is 2.217 (Howe),
    /// close to the exact 2.2228.
    #[test]
    fn two_sided_factors_match_independent_integration() {
        let cases = [
            (43, 0.90, 0.99, 2.222825173791122),
            (10, 0.90, 0.95, 2.8563108485789286),
            (20, 0.95, 0.95, 2.7603461784456984),
            (2, 0.90, 0.90, 15.512325981126796),
            (100, 0.99, 0.95, 2.935549241147594),
        ];
        for (n, p, g, k) in cases {
            let got = two_sided_tolerance_factor(n, p, g).unwrap();
            assert!(close(got, k, 1e-6), "n={n} p={p} γ={g}: {got} vs {k}");
        }
        assert!((two_sided_tolerance_factor(43, 0.90, 0.99).unwrap() - 2.217).abs() < 0.01);
    }

    #[test]
    fn content_probability_inverts_the_factors() {
        for (n, p, g) in [(10, 0.9, 0.95), (30, 0.99, 0.9), (5, 0.75, 0.99)] {
            let k2 = two_sided_tolerance_factor(n, p, g).unwrap();
            assert!((tolerance_content_probability(n, p, k2, true).unwrap() - g).abs() < 1e-9);
            let k1 = one_sided_tolerance_factor(n, p, g).unwrap();
            assert!((tolerance_content_probability(n, p, k1, false).unwrap() - g).abs() < 1e-9);
        }
        assert!(tolerance_content_probability(1, 0.9, 2.0, true).is_err());
        assert!(tolerance_content_probability(5, 0.9, 0.0, true).is_err());
    }

    #[test]
    fn normal_interval_uses_mean_and_sd() {
        let x = [9.8, 10.2, 10.1, 9.7, 10.4, 10.0, 9.9, 10.3, 10.0, 9.6];
        let (m, v) = mean_var(&x);
        let two = normal_tolerance_interval(&x, 0.90, 0.95, ToleranceSides::TwoSided).unwrap();
        assert!(close(two.lower, m - 2.8563108485789286 * v.sqrt(), 1e-6));
        assert!(close(two.upper, m + 2.8563108485789286 * v.sqrt(), 1e-6));
        let up = normal_tolerance_interval(&x, 0.90, 0.95, ToleranceSides::Upper).unwrap();
        assert_eq!(up.lower, f64::NEG_INFINITY);
        assert!(close(up.upper, m + 2.354640131829059 * v.sqrt(), 1e-8));
        assert!(normal_tolerance_interval(&[1.0], 0.9, 0.95, ToleranceSides::TwoSided).is_err());
        assert!(normal_tolerance_interval(&[1.0, 1.0], 0.9, 0.95, ToleranceSides::TwoSided).is_err());
        assert!(normal_tolerance_interval(&x, 1.0, 0.95, ToleranceSides::TwoSided).is_err());
    }

    /// Wilks: with the sample extremes (m = 2) the confidence is
    /// `1 − n pⁿ⁻¹ + (n − 1)pⁿ`; 93 values give ≥ 95% confidence for 95%
    /// content (the classic "n = 93" result), 92 do not. One-sided with the
    /// minimum: `1 − pⁿ`, so 59 values for 95%/95%.
    #[test]
    fn distribution_free_intervals_match_wilks() {
        let wilks = |n: usize, p: f64| 1.0 - n as f64 * p.powi(n as i32 - 1) + (n as f64 - 1.0) * p.powi(n as i32);
        let x: Vec<f64> = (0..93).map(|i| ((i * 37) % 93) as f64).collect();
        let t = nonparametric_tolerance_interval(&x, 0.95, 0.95, ToleranceSides::TwoSided).unwrap();
        assert_eq!(t.order_statistics, Some((1, 93)));
        assert_eq!((t.lower, t.upper), (0.0, 92.0));
        assert!(close(t.achieved_confidence, wilks(93, 0.95), 1e-12));
        assert!(t.achieved_confidence >= 0.95);
        let short = nonparametric_tolerance_interval(&x[..92], 0.95, 0.95, ToleranceSides::TwoSided).unwrap();
        assert!(short.achieved_confidence < 0.95);
        let one = nonparametric_tolerance_interval(&x[..59], 0.95, 0.95, ToleranceSides::Lower).unwrap();
        assert_eq!(one.order_statistics, Some((1, 60)));
        assert!(close(one.achieved_confidence, 1.0 - 0.95f64.powi(59), 1e-12));
        assert!(
            nonparametric_tolerance_interval(&x[..58], 0.95, 0.95, ToleranceSides::Lower).unwrap().achieved_confidence
                < 0.95
        );
        // A large sample moves inward from the extremes.
        let big: Vec<f64> = (0..1000).map(f64::from).collect();
        let t = nonparametric_tolerance_interval(&big, 0.90, 0.95, ToleranceSides::TwoSided).unwrap();
        let (r, upper_rank) = t.order_statistics.unwrap();
        let m = r + (1001 - upper_rank);
        assert!(binomial_cdf((1000 - m) as u64, 1000, 0.90).unwrap() >= 0.95);
        assert!(binomial_cdf((1000 - m - 1) as u64, 1000, 0.90).unwrap() < 0.95);
    }
}
