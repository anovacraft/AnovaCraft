//! Reliability test planning: demonstration, estimation and accelerated
//! life test plans.
//!
//! # Demonstration plans (Weibull shape known)
//! To demonstrate reliability `R` at time `t` with confidence `C`, `n` units
//! are each tested for time `T` and the test passes with at most `c`
//! failures. With a Weibull life of known shape `β`, a unit meeting the
//! target fails during the test with probability
//! `p = 1 − R^((T/t)^β)`, and the plan requires
//! `P(X ≤ c | n, p) ≤ 1 − C` (binomial `X`), so a unit population worse
//! than the target passes with probability at most `1 − C`. Equivalently
//! `p ≥ p*`, the upper `C` confidence bound for a proportion after `c`
//! failures in `n`: `p* = B⁻¹(C; c + 1, n − c)` (beta quantile). With
//! `c = 0` this is the familiar `n = ln(1 − C) / ((T/t)^β ln R)` (Meeker &
//! Escobar 1998, *Statistical Methods for Reliability Data*, §10.7; Kececioglu
//! 1994, *Reliability & Life Testing Handbook* vol. 2).
//!
//! # Estimation plans (large-sample)
//! For a log-location-scale life distribution (Weibull: smallest extreme
//! value of log life; lognormal: normal) with scale `σ`, type I censored at
//! standardized time `ζ_c` (so a fraction `p_c = Φ(ζ_c)` fails), the
//! per-unit Fisher information of `(μ, σ)` is `I(ζ_c)/σ²` with
//!
//! ```text
//! I = ∫_{−∞}^{ζ_c} [ψ², ψ(1 + zψ); ·, (1 + zψ)²] φ(z) dz
//!     + (1 − Φ(ζ_c)) [h², z h²; ·, z² h²]_{z = ζ_c}
//! ```
//!
//! where `ψ = (ln φ)'` and `h = φ/(1 − Φ)` (Escobar & Meeker 1994,
//! "Algorithm AS 292", *Applied Statistics* 43; Meeker & Escobar §10.5). The
//! estimate of the `p` quantile `ln t_p = μ + w_p σ` has
//! `Var(ln t̂_p) ≈ σ² V/n` with `V = cᵀ I⁻¹ c`, `c = (1, w_p)`, so the
//! interval `[t̂_p/R, t̂_p·R]` has precision factor
//! `R = exp(z_{(1+C)/2} σ √(V/n))` and `n = z² σ² V / (ln R)²`.
//!
//! # Accelerated life test plans
//! With log life location `μ(ξ) = γ₀ + γ₁ξ` on standardized stress
//! `ξ = (s − s_H)/(s_U − s_H)` (`ξ = 0` at the highest test stress, 1 at
//! use; `s` the transformed stress, e.g. `11 604.5/(°C + 273.15)` for
//! Arrhenius), constant `σ` and censoring time `t_c`, the standardized
//! censoring time is linear in `ξ`: `ζ(ξ) = ζ_H + ξ(ζ_U − ζ_H)` with
//! `ζ_H, ζ_U` from the planning probabilities of failing by `t_c` at the
//! highest stress and at use. A plan puts fraction `π_i` of the units at
//! `ξ_i`; the information of `(γ₀, γ₁, σ)` is
//! `Σ π_i Gᵢᵀ I(ζ(ξ_i)) Gᵢ` with `Gᵢ = [1 ξ_i 0; 0 0 1]`, and
//! `V = cᵀ F⁻¹ c` with `c = (1, 1, w_p)` (the quantile at use). The
//! **optimum** two-level plan minimizes `V` over the low level and its
//! allocation with the high level at `ξ = 0` (Nelson & Kielpinski 1976,
//! *Technometrics* 18); the **compromise** plan uses three equally spaced
//! levels `ξ_L, ξ_L/2, 0` with allocation 4:2:1 and the best `ξ_L` (Meeker &
//! Hahn 1985, *How to Plan an Accelerated Life Test*, ASQC; Meeker &
//! Escobar ch. 20).

use crate::LifeDistribution;
use stat_core::discrete::{beta_quantile, binomial_cdf};
use stat_core::optimize::minimize_in_box;
use stat_core::quadrature::CompositeRule;
use stat_core::special::erfc;
use stat_core::{ContinuousDistribution, Normal, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn check_open_unit(name: &str, v: f64) -> Result<()> {
    if v > 0.0 && v < 1.0 { Ok(()) } else { Err(invalid(format!("{name} must be between 0 and 1 (got {v})"))) }
}

fn z_two_sided(confidence: f64) -> Result<f64> {
    Normal::standard().quantile(0.5 + confidence / 2.0)
}

// ---------------------------------------------------------------------------
// Demonstration plans
// ---------------------------------------------------------------------------

/// The failure probability during the test that a unit exactly meeting the
/// target has: `1 − R^((T/t)^β)`.
fn unit_failure_probability(reliability: f64, time_ratio: f64, shape: f64) -> f64 {
    -(time_ratio.powf(shape) * reliability.ln()).exp_m1()
}

fn check_demonstration(reliability: f64, confidence: f64, shape: f64) -> Result<()> {
    check_open_unit("the reliability target", reliability)?;
    check_open_unit("the confidence level", confidence)?;
    if !(shape > 0.0 && shape.is_finite()) {
        return Err(invalid("the Weibull shape must be positive"));
    }
    Ok(())
}

/// Smallest number of units, each tested for `time_ratio = T/t` times the
/// target time, that demonstrates `reliability` at the target time with
/// `confidence` when at most `failures` may fail.
///
/// # Errors
/// [`StatError::InvalidArgument`] for values out of range, or when more than
/// a million units would be needed.
pub fn demonstration_units(
    reliability: f64,
    confidence: f64,
    shape: f64,
    time_ratio: f64,
    failures: usize,
) -> Result<usize> {
    check_demonstration(reliability, confidence, shape)?;
    if !(time_ratio > 0.0 && time_ratio.is_finite()) {
        return Err(invalid("the test time must be positive"));
    }
    let p = unit_failure_probability(reliability, time_ratio, shape);
    if p >= 1.0 {
        return Ok(failures + 1);
    }
    // P(X ≤ c | n, p) falls as n grows: double to bracket, then bisect.
    let c = failures as u64;
    let passes = |n: u64| binomial_cdf(c, n, p).map(|v| v <= 1.0 - confidence);
    let (mut lo, mut hi) = (c, c + 1);
    while !passes(hi)? {
        lo = hi;
        hi *= 2;
        if hi > 1_000_000 {
            return Err(invalid("more than a million units would be needed; allow a longer test or more failures"));
        }
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if passes(mid)? { hi = mid } else { lo = mid }
    }
    Ok(hi as usize)
}

/// Test time per unit, as a multiple of the target time, that demonstrates
/// `reliability` with `confidence` testing `units` units and allowing
/// `failures` failures.
///
/// # Errors
/// [`StatError::InvalidArgument`] for values out of range or
/// `units ≤ failures`.
pub fn demonstration_time_ratio(
    reliability: f64,
    confidence: f64,
    shape: f64,
    units: usize,
    failures: usize,
) -> Result<f64> {
    check_demonstration(reliability, confidence, shape)?;
    if units <= failures {
        return Err(invalid("there must be more units than allowed failures"));
    }
    let p = beta_quantile(failures as f64 + 1.0, (units - failures) as f64, confidence)?;
    Ok(((-p).ln_1p() / reliability.ln()).powf(1.0 / shape))
}

/// Probability that a plan passes when the true reliability at the target
/// time is `true_reliability` (its operating characteristic).
///
/// # Errors
/// Propagates numerical failures of the binomial distribution.
pub fn demonstration_pass_probability(
    true_reliability: f64,
    shape: f64,
    time_ratio: f64,
    units: usize,
    failures: usize,
) -> Result<f64> {
    let p = unit_failure_probability(true_reliability.clamp(1e-300, 1.0), time_ratio, shape).clamp(0.0, 1.0);
    binomial_cdf(failures as u64, units as u64, p)
}

// ---------------------------------------------------------------------------
// Location-scale information
// ---------------------------------------------------------------------------

/// Standardized `p` quantile `w_p` of the log-life distribution: smallest
/// extreme value `ln(−ln(1 − p))` (Weibull) or `Φ⁻¹(p)` (lognormal).
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `0 < p < 1`.
pub fn standardized_quantile(distribution: LifeDistribution, p: f64) -> Result<f64> {
    check_open_unit("the probability", p)?;
    Ok(match distribution {
        LifeDistribution::Weibull => (-(-p).ln_1p()).ln(),
        LifeDistribution::Lognormal => Normal::standard().quantile(p)?,
    })
}

/// `(φ(z), ψ(z) = (ln φ)'(z), 1 − Φ(z))` of the standardized distribution.
fn standard_parts(distribution: LifeDistribution, z: f64) -> (f64, f64, f64) {
    match distribution {
        LifeDistribution::Weibull => {
            let e = z.exp();
            ((z - e).exp(), 1.0 - e, (-e).exp())
        }
        LifeDistribution::Lognormal => {
            let phi = (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt();
            (phi, -z, 0.5 * erfc(z / std::f64::consts::SQRT_2))
        }
    }
}

/// Per-unit Fisher information `[I_μμ, I_μσ, I_σσ]` (times `σ²`) of a
/// log-location-scale distribution under type I censoring at standardized
/// time `zeta` (`f64::INFINITY` for none). See the module notes.
#[must_use]
pub fn location_scale_information(distribution: LifeDistribution, zeta: f64) -> [f64; 3] {
    let (low, high) = match distribution {
        LifeDistribution::Weibull => (-40.0, 4.0),
        LifeDistribution::Lognormal => (-12.0, 12.0),
    };
    let upper = zeta.min(high);
    let mut out = [0.0; 3];
    if upper > low {
        let rule = CompositeRule::new(10, 400);
        for (k, slot) in out.iter_mut().enumerate() {
            *slot = rule.integrate(low, upper, |z| {
                let (phi, psi, _) = standard_parts(distribution, z);
                let (a, b) = (psi, 1.0 + z * psi);
                phi * match k {
                    0 => a * a,
                    1 => a * b,
                    _ => b * b,
                }
            });
        }
    }
    if zeta < high {
        let (phi, _, surv) = standard_parts(distribution, zeta);
        if surv > 0.0 {
            let h = phi / surv;
            let w = surv * h * h;
            out[0] += w;
            out[1] += w * zeta;
            out[2] += w * zeta * zeta;
        }
    }
    out
}

/// `V` with `Var(ln t̂_p) ≈ σ²V/n` for a sample of `n` units censored when a
/// fraction `censored_fraction_failing` of them is expected to have failed
/// (1 for no censoring).
///
/// # Errors
/// [`StatError::InvalidArgument`] for probabilities out of range.
pub fn percentile_variance_factor(distribution: LifeDistribution, fraction_failing: f64, p: f64) -> Result<f64> {
    let zeta =
        if fraction_failing >= 1.0 { f64::INFINITY } else { standardized_quantile(distribution, fraction_failing)? };
    let w = standardized_quantile(distribution, p)?;
    let [a, b, c] = location_scale_information(distribution, zeta);
    let det = a * c - b * b;
    if det.is_nan() || det <= 0.0 {
        return Err(invalid("too few failures are expected for the parameters to be estimable"));
    }
    // c' I⁻¹ c with c = (1, w).
    Ok((c - 2.0 * w * b + w * w * a) / det)
}

/// Inputs of an estimation plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EstimationSetup {
    /// Life distribution.
    pub distribution: LifeDistribution,
    /// Planning value of `σ` (1/shape for Weibull, σ of log life for
    /// lognormal).
    pub sigma: f64,
    /// Fraction expected to fail by the end of the test (1 for none
    /// censored).
    pub fraction_failing: f64,
    /// The quantile to estimate (e.g. 0.1 for the B10 life).
    pub percentile: f64,
    /// Confidence level of the interval.
    pub confidence: f64,
}

/// An estimation plan's answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EstimationPlan {
    /// Units needed.
    pub units: usize,
    /// `V` (see [`percentile_variance_factor`]).
    pub variance_factor: f64,
    /// Precision factor reached with `units`.
    pub precision_factor: f64,
    /// Expected failures, `units × fraction_failing`.
    pub expected_failures: f64,
}

impl EstimationSetup {
    fn check(&self) -> Result<()> {
        if !(self.sigma > 0.0 && self.sigma.is_finite()) {
            return Err(invalid("σ must be positive"));
        }
        if !(self.fraction_failing > 0.0 && self.fraction_failing <= 1.0) {
            return Err(invalid("the fraction failing must be above 0 and at most 1"));
        }
        check_open_unit("the percentile", self.percentile)?;
        check_open_unit("the confidence level", self.confidence)
    }

    /// Precision factor `R` reached with `units` units.
    ///
    /// # Errors
    /// As [`percentile_variance_factor`], or invalid inputs.
    pub fn precision_factor(&self, units: usize) -> Result<f64> {
        self.check()?;
        let v = percentile_variance_factor(self.distribution, self.fraction_failing, self.percentile)?;
        Ok((z_two_sided(self.confidence)? * self.sigma * (v / units.max(1) as f64).sqrt()).exp())
    }

    /// Smallest sample size reaching precision factor `target` (> 1).
    ///
    /// # Errors
    /// As [`Self::precision_factor`], or `target ≤ 1`.
    pub fn plan(&self, target: f64) -> Result<EstimationPlan> {
        self.check()?;
        if !(target > 1.0 && target.is_finite()) {
            return Err(invalid("the precision factor must be greater than 1"));
        }
        let v = percentile_variance_factor(self.distribution, self.fraction_failing, self.percentile)?;
        let z = z_two_sided(self.confidence)?;
        let units = units_for_precision(self.sigma, v, z, target);
        Ok(EstimationPlan {
            units,
            variance_factor: v,
            precision_factor: (z * self.sigma * (v / units as f64).sqrt()).exp(),
            expected_failures: units as f64 * self.fraction_failing,
        })
    }
}

fn units_for_precision(sigma: f64, v: f64, z: f64, target: f64) -> usize {
    let n = (z * sigma / target.ln()).powi(2) * v;
    (n - 1e-9).ceil().max(2.0) as usize
}

// ---------------------------------------------------------------------------
// Accelerated life test plans
// ---------------------------------------------------------------------------

/// Planning values of an accelerated life test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceleratedSetup {
    /// Life distribution.
    pub distribution: LifeDistribution,
    /// Planning value of `σ`.
    pub sigma: f64,
    /// Probability of failing by the censoring time at use conditions.
    pub p_use: f64,
    /// Probability of failing by the censoring time at the highest test
    /// stress.
    pub p_high: f64,
    /// The quantile to estimate at use conditions.
    pub percentile: f64,
}

/// One test level of an accelerated plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StressLevel {
    /// Standardized stress `ξ` (0 highest, 1 use).
    pub xi: f64,
    /// Fraction of units tested there.
    pub fraction: f64,
}

impl AcceleratedSetup {
    fn check(&self) -> Result<()> {
        if !(self.sigma > 0.0 && self.sigma.is_finite()) {
            return Err(invalid("σ must be positive"));
        }
        check_open_unit("the probability of failing at use", self.p_use)?;
        check_open_unit("the probability of failing at the highest stress", self.p_high)?;
        check_open_unit("the percentile", self.percentile)?;
        if self.p_high <= self.p_use {
            return Err(invalid("failures must be more likely at the highest stress than at use"));
        }
        Ok(())
    }

    /// Standardized censoring time at standardized stress `xi`.
    ///
    /// # Errors
    /// Invalid planning values.
    pub fn zeta(&self, xi: f64) -> Result<f64> {
        let zh = standardized_quantile(self.distribution, self.p_high)?;
        let zu = standardized_quantile(self.distribution, self.p_use)?;
        Ok(zh + xi * (zu - zh))
    }

    /// Probability of failing by the censoring time at `xi`.
    ///
    /// # Errors
    /// Invalid planning values.
    pub fn failure_probability(&self, xi: f64) -> Result<f64> {
        let z = self.zeta(xi)?;
        Ok(1.0 - standard_parts(self.distribution, z).2)
    }

    /// `V` with `Var(ln t̂_p at use) ≈ σ²V/n` for a plan.
    ///
    /// # Errors
    /// Invalid planning values or levels (fractions must be positive; at
    /// least two distinct levels).
    pub fn variance_factor(&self, levels: &[StressLevel]) -> Result<f64> {
        self.check()?;
        let total: f64 = levels.iter().map(|l| l.fraction).sum();
        if levels.iter().any(|l| l.fraction.is_nan() || l.fraction <= 0.0 || !l.xi.is_finite())
            || total.is_nan()
            || total <= 0.0
        {
            return Err(invalid("every test level needs a positive share of the units"));
        }
        let mut f = [[0.0f64; 3]; 3];
        for l in levels {
            let [a, b, c] = location_scale_information(self.distribution, self.zeta(l.xi)?);
            let w = l.fraction / total;
            let g = [1.0, l.xi];
            for i in 0..2 {
                for j in 0..2 {
                    f[i][j] += w * a * g[i] * g[j];
                }
                f[i][2] += w * b * g[i];
                f[2][i] += w * b * g[i];
            }
            f[2][2] += w * c;
        }
        let w = standardized_quantile(self.distribution, self.percentile)?;
        quadratic_form_inverse(&f, &[1.0, 1.0, w])
            .ok_or_else(|| invalid("the plan cannot estimate the model: use at least two distinct stress levels"))
    }

    /// The optimum two-level plan: high level at `ξ = 0`, the low level and
    /// its share chosen to minimize `V`. Returns the levels (low first) and
    /// `V`.
    ///
    /// # Errors
    /// Invalid planning values.
    pub fn optimum_plan(&self) -> Result<(Vec<StressLevel>, f64)> {
        self.check()?;
        let levels =
            |x: &[f64]| vec![StressLevel { xi: x[0], fraction: x[1] }, StressLevel { xi: 0.0, fraction: 1.0 - x[1] }];
        let mut best: Option<(Vec<f64>, f64)> = None;
        for start in [[0.5, 0.6], [0.8, 0.75], [0.3, 0.5], [0.95, 0.9]] {
            let mut f = |x: &[f64]| self.variance_factor(&levels(x)).unwrap_or(f64::INFINITY);
            let m = minimize_in_box(&mut f, &start, &[0.01, 0.01], &[1.0, 0.99], 3000);
            if best.as_ref().is_none_or(|b| m.value < b.1) {
                best = Some((m.x, m.value));
            }
        }
        let (x, v) = best.ok_or_else(|| invalid("no plan found"))?;
        if !v.is_finite() {
            return Err(invalid("no plan can estimate the model with these planning values"));
        }
        Ok((levels(&x), v))
    }

    /// The compromise plan: levels `ξ_L, ξ_L/2, 0` with shares 4/7, 2/7,
    /// 1/7, the low level chosen to minimize `V`. Returns the levels (low
    /// first) and `V`.
    ///
    /// # Errors
    /// Invalid planning values.
    pub fn compromise_plan(&self) -> Result<(Vec<StressLevel>, f64)> {
        self.check()?;
        let levels = |xl: f64| {
            vec![
                StressLevel { xi: xl, fraction: 4.0 / 7.0 },
                StressLevel { xi: xl / 2.0, fraction: 2.0 / 7.0 },
                StressLevel { xi: 0.0, fraction: 1.0 / 7.0 },
            ]
        };
        // V is smooth in ξ_L; a fine scan then golden-section refinement.
        let v = |xl: f64| self.variance_factor(&levels(xl)).unwrap_or(f64::INFINITY);
        let grid: Vec<f64> = (1..=100).map(|i| i as f64 / 100.0).collect();
        let (mut k, mut best) = (0, f64::INFINITY);
        for (i, &x) in grid.iter().enumerate() {
            let value = v(x);
            if value < best {
                (k, best) = (i, value);
            }
        }
        if !best.is_finite() {
            return Err(invalid("no plan can estimate the model with these planning values"));
        }
        let (mut a, mut b) = (grid[k.saturating_sub(1)], grid[(k + 1).min(grid.len() - 1)]);
        let g = 0.5 * (5f64.sqrt() - 1.0);
        for _ in 0..80 {
            let (c, d) = (b - g * (b - a), a + g * (b - a));
            if v(c) < v(d) {
                b = d;
            } else {
                a = c;
            }
        }
        let xl = 0.5 * (a + b);
        Ok((levels(xl), v(xl)))
    }
}

/// Sample size for precision factor `target` with `V`, `σ` and confidence
/// level `confidence`.
///
/// # Errors
/// `target ≤ 1` or a confidence level outside (0, 1).
pub fn units_for_precision_factor(sigma: f64, variance_factor: f64, confidence: f64, target: f64) -> Result<usize> {
    check_open_unit("the confidence level", confidence)?;
    if !(target > 1.0 && target.is_finite()) {
        return Err(invalid("the precision factor must be greater than 1"));
    }
    Ok(units_for_precision(sigma, variance_factor, z_two_sided(confidence)?, target))
}

/// `cᵀ F⁻¹ c` for a symmetric positive-definite 3 × 3 `F` (cofactors), or
/// `None` when `F` is singular.
fn quadratic_form_inverse(f: &[[f64; 3]; 3], c: &[f64; 3]) -> Option<f64> {
    let cof = |i: usize, j: usize| {
        let r: Vec<usize> = (0..3).filter(|&k| k != i).collect();
        let s: Vec<usize> = (0..3).filter(|&k| k != j).collect();
        let m = f[r[0]][s[0]] * f[r[1]][s[1]] - f[r[0]][s[1]] * f[r[1]][s[0]];
        if (i + j) % 2 == 0 { m } else { -m }
    };
    let det = f[0][0] * cof(0, 0) + f[0][1] * cof(0, 1) + f[0][2] * cof(0, 2);
    let scale = f[0][0] * f[1][1] * f[2][2];
    if det.is_nan() || det <= 1e-12 * scale.abs() {
        return None;
    }
    let mut q = 0.0;
    for i in 0..3 {
        for j in 0..3 {
            q += c[i] * cof(j, i) * c[j];
        }
    }
    Some(q / det)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_failure_plan_matches_closed_form() {
        // n = ln(1 − C)/((T/t)^β ln R): R = 0.9, C = 0.9, β = 2, T/t = 1.5:
        // ln 0.1 / (2.25 ln 0.9) = 9.7106… → 10 units.
        let n = demonstration_units(0.9, 0.9, 2.0, 1.5, 0).unwrap();
        let exact = 0.1f64.ln() / (2.25 * 0.9f64.ln());
        assert_eq!(n, exact.ceil() as usize);
        // The time for 10 units reproduces the closed form: (ln(1−C)/(n ln R))^(1/β).
        let ratio = demonstration_time_ratio(0.9, 0.9, 2.0, 10, 0).unwrap();
        assert!((ratio - (0.1f64.ln() / (10.0 * 0.9f64.ln())).sqrt()).abs() < 1e-9);
        // Classic success-run plan (β = 1, T = t): R = 0.95, C = 0.9 → 45 units.
        assert_eq!(demonstration_units(0.95, 0.9, 1.0, 1.0, 0).unwrap(), 45);
    }

    #[test]
    fn few_failure_plan_meets_the_binomial_condition() {
        // R = 0.9 at t, C = 0.9, β = 1, T = t, one failure allowed: n = 38
        // (by hand: P(X ≤ 1 | 38, 0.1) = 4.7·0.9³⁷ = 0.0953; at 37: 0.1037).
        let n = demonstration_units(0.9, 0.9, 1.0, 1.0, 1).unwrap();
        assert_eq!(n, 38);
        let pass = demonstration_pass_probability(0.9, 1.0, 1.0, n, 1).unwrap();
        assert!(pass <= 0.1 && pass > 0.09, "{pass}");
        assert!(demonstration_pass_probability(0.9, 1.0, 1.0, 37, 1).unwrap() > 0.1);
        // The time ratio for n = 38 is at most 1 and gives exactly 1 − C.
        let ratio = demonstration_time_ratio(0.9, 0.9, 1.0, 38, 1).unwrap();
        assert!(ratio <= 1.0);
        let p = demonstration_pass_probability(0.9, 1.0, ratio, 38, 1).unwrap();
        assert!((p - 0.1).abs() < 1e-9);
        assert!(demonstration_time_ratio(0.9, 0.9, 1.0, 1, 1).is_err());
    }

    #[test]
    fn information_without_censoring_matches_known_values() {
        // Normal: diag(1, 2). Smallest extreme value: I_μμ = 1,
        // I_μσ = 1 − γ = 0.4227843, I_σσ = π²/6 + (1 − γ)² = 1.8236806.
        let n = location_scale_information(LifeDistribution::Lognormal, f64::INFINITY);
        assert!((n[0] - 1.0).abs() < 1e-9 && n[1].abs() < 1e-9 && (n[2] - 2.0).abs() < 1e-9);
        let s = location_scale_information(LifeDistribution::Weibull, f64::INFINITY);
        let g = 0.577_215_664_901_532_9;
        assert!((s[0] - 1.0).abs() < 1e-9);
        assert!((s[1] - (1.0 - g)).abs() < 1e-9);
        assert!((s[2] - (std::f64::consts::PI.powi(2) / 6.0 + (1.0 - g).powi(2))).abs() < 1e-9);
        // Uncensored normal: Var(ln t̂_p) = σ²(1 + w²/2)/n.
        let w = standardized_quantile(LifeDistribution::Lognormal, 0.1).unwrap();
        let v = percentile_variance_factor(LifeDistribution::Lognormal, 1.0, 0.1).unwrap();
        assert!((v - (1.0 + w * w / 2.0)).abs() < 1e-8);
    }

    #[test]
    fn censored_information_matches_independent_quadrature() {
        // Independent Python (stdlib, Simpson's rule with 200 000 panels on
        // the same integrals):
        // Weibull censored at p_c = 0.3 → [0.3, −0.33651124, 0.70367547]
        // (I_μμ = p_c exactly for the smallest extreme value); lognormal
        // censored at the median → [½ + 1/π, −φ(0), 1].
        let zw = standardized_quantile(LifeDistribution::Weibull, 0.3).unwrap();
        let w = location_scale_information(LifeDistribution::Weibull, zw);
        for (a, b) in w.iter().zip([0.3, -0.336_511_239, 0.703_675_466]) {
            assert!((a - b).abs() < 1e-8, "{w:?}");
        }
        let l = location_scale_information(LifeDistribution::Lognormal, 0.0);
        let pi = std::f64::consts::PI;
        for (a, b) in l.iter().zip([0.5 + 1.0 / pi, -1.0 / (2.0 * pi).sqrt(), 1.0]) {
            assert!((a - b).abs() < 1e-9, "{l:?}");
        }
        // Python: V = 7.2384340 for the B10 life, Weibull, 30% failing.
        let v = percentile_variance_factor(LifeDistribution::Weibull, 0.3, 0.1).unwrap();
        assert!((v - 7.238_434_0).abs() < 1e-6, "{v}");
    }

    #[test]
    fn estimation_plan_sample_size() {
        // Uncensored lognormal median, σ = 0.5, 95%, R = 1.2:
        // n = (1.959964 · 0.5 / ln 1.2)² · 1 = 28.9 → 29.
        let setup = EstimationSetup {
            distribution: LifeDistribution::Lognormal,
            sigma: 0.5,
            fraction_failing: 1.0,
            percentile: 0.5,
            confidence: 0.95,
        };
        let plan = setup.plan(1.2).unwrap();
        let exact = (1.959_963_984_540_054 * 0.5 / 1.2f64.ln()).powi(2);
        assert_eq!(plan.units, exact.ceil() as usize);
        assert!(plan.precision_factor <= 1.2);
        assert!(setup.precision_factor(plan.units - 1).unwrap() > 1.2);
        // Censoring only adds units.
        let censored = EstimationSetup { fraction_failing: 0.3, ..setup }.plan(1.2).unwrap();
        assert!(censored.units > plan.units);
    }

    #[test]
    fn accelerated_plans() {
        // Uncensored normal with planning values making every level fail
        // (p ≈ 1) reduces to linear regression: ξ = 0, 1 half each →
        // Var(γ₀ + γ₁) = 2σ²/n, plus the w_p² Var(σ̂) term (w = 0 at the median).
        let setup = AcceleratedSetup {
            distribution: LifeDistribution::Lognormal,
            sigma: 0.5,
            p_use: 1.0 - 1e-15,
            p_high: 1.0 - 1e-16,
            percentile: 0.5,
        };
        let v = setup
            .variance_factor(&[StressLevel { xi: 1.0, fraction: 0.5 }, StressLevel { xi: 0.0, fraction: 0.5 }])
            .unwrap();
        assert!((v - 2.0).abs() < 1e-6, "{v}");
        // Optimum and compromise plans for a censored test: the optimum is no
        // worse than the compromise, both put their low level inside (0, 1].
        let s = AcceleratedSetup {
            distribution: LifeDistribution::Weibull,
            sigma: 0.6,
            p_use: 0.001,
            p_high: 0.9,
            percentile: 0.1,
        };
        let (opt, vo) = s.optimum_plan().unwrap();
        let (comp, vc) = s.compromise_plan().unwrap();
        // Independent Python grid searches over the same model (Simpson
        // quadrature): optimum V ≈ 119.962 at ξ_L = 0.32, π_L = 0.71 (grid
        // step 0.01); compromise V = 146.125 at ξ_L = 0.391 (step 0.001).
        assert!(vo <= 119.97 && vo > 119.5, "{vo}");
        assert!((opt[0].xi - 0.32).abs() < 0.02 && (opt[0].fraction - 0.71).abs() < 0.02, "{opt:?}");
        assert!((vc - 146.125).abs() < 0.05, "{vc}");
        assert!((comp[0].xi - 0.391).abs() < 0.003, "{comp:?}");
        assert!(vo <= vc);
        assert_eq!(comp.len(), 3);
        assert!((comp[1].xi - comp[0].xi / 2.0).abs() < 1e-15);
        // One level only cannot estimate the slope.
        assert!(s.variance_factor(&[StressLevel { xi: 0.0, fraction: 1.0 }]).is_err());
        assert!(units_for_precision_factor(0.6, vo, 0.95, 1.5).unwrap() >= 2);
    }
}
