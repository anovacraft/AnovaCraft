//! Maximum-likelihood Weibull and lognormal fits to right-censored data.
//!
//! Both distributions are **log-location-scale** families: with `y = ln t`,
//! location `μ`, scale `σ > 0` and `z = (y − μ)/σ`, the survival function is
//! `S(t) = S₀(z)` for a standard distribution `S₀` (Lawless, *Statistical
//! Models and Methods for Lifetime Data*, 2nd ed., §1.3, §5.2; Meeker & Escobar,
//! *Statistical Methods for Reliability Data*, ch. 8):
//!
//! | Distribution | `S₀(z)` | log density `ln f₀(z)` | parameters |
//! |---|---|---|---|
//! | Weibull | `exp(−eᶻ)` (smallest extreme value) | `z − eᶻ` | scale `α = e^μ`, shape `β = 1/σ`; `S(t) = exp(−(t/α)^β)` |
//! | Lognormal | `1 − Φ(z)` | `−z²/2 − ½ ln 2π` | `μ`, `σ` of `ln T`; median `e^μ` |
//!
//! # Likelihood
//! For events `E` and right-censored observations `C`, the log-likelihood on
//! the time scale is
//!
//! ```text
//! ℓ(μ, σ) = Σ_E [ln f₀(zᵢ) − ln σ − ln tᵢ] + Σ_C ln S₀(zᵢ).
//! ```
//!
//! It is maximized by damped Newton iterations (Levenberg–Marquardt) on
//! `(μ, ln σ)` with analytic first and second derivatives, from moment-based
//! starting values. The covariance of `(μ̂, σ̂)` is the inverse of the observed
//! information `−∇²ℓ` at the maximum.
//!
//! # Intervals
//! Wald intervals are formed on scales where the estimates are closer to
//! normal: `μ̂ ± z·SE`, `σ̂·exp(±z·SE/σ̂)`. For a time quantile
//! `t_p = exp(μ + σ w_p)` (with `w_p = S₀⁻¹(1 − p)`) the delta method gives
//! `Var(ln t̂_p) = V_μμ + 2w_p V_μσ + w_p² V_σσ`; for the survival curve,
//! `Var(ẑ) = (V_μμ + 2z V_μσ + z² V_σσ)/σ̂²` and the limits are `S₀(ẑ ± z·SE)`.

use stat_core::special::erfc;
use stat_core::{ContinuousDistribution, Normal, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

const LN_SQRT_2PI: f64 = 0.918_938_533_204_672_8;

/// A two-parameter life distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifeDistribution {
    /// Weibull: `S(t) = exp(−(t/α)^β)`.
    Weibull,
    /// Lognormal: `ln T ~ N(μ, σ²)`.
    Lognormal,
}

impl LifeDistribution {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Weibull => "Weibull",
            Self::Lognormal => "Lognormal",
        }
    }

    /// `ln f₀(z)` and its first two derivatives.
    fn log_density(self, z: f64) -> (f64, f64, f64) {
        match self {
            Self::Weibull => {
                let e = z.exp();
                (z - e, 1.0 - e, -e)
            }
            Self::Lognormal => (-0.5 * z * z - LN_SQRT_2PI, -z, -1.0),
        }
    }

    /// `ln S₀(z)` and its first two derivatives.
    fn log_survival(self, z: f64) -> (f64, f64, f64) {
        match self {
            Self::Weibull => {
                let e = z.exp();
                (-e, -e, -e)
            }
            Self::Lognormal => {
                let ln_q = ln_normal_sf(z);
                // Inverse Mills ratio λ = φ(z)/Q(z), computed on the log scale.
                let lambda = (-0.5 * z * z - LN_SQRT_2PI - ln_q).exp();
                (ln_q, -lambda, -lambda * (lambda - z))
            }
        }
    }

    /// Standard survival function `S₀(z)`.
    fn standard_survival(self, z: f64) -> f64 {
        match self {
            Self::Weibull => (-z.exp()).exp(),
            Self::Lognormal => 0.5 * erfc(z * std::f64::consts::FRAC_1_SQRT_2),
        }
    }

    /// `w_p` with `S₀(w_p) = 1 − p`.
    fn standard_quantile(self, p: f64) -> Result<f64> {
        match self {
            Self::Weibull => Ok((-(-p).ln_1p()).ln()),
            Self::Lognormal => Normal::standard().quantile(p),
        }
    }
}

/// `ln Q(z)` for the standard normal upper tail, accurate far into the tail.
fn ln_normal_sf(z: f64) -> f64 {
    let q = 0.5 * erfc(z * std::f64::consts::FRAC_1_SQRT_2);
    if q > 1e-300 {
        q.ln()
    } else {
        // Asymptotic expansion (A&S 26.2.12).
        let z2 = z * z;
        -0.5 * z2 - z.ln() - LN_SQRT_2PI + (1.0 - 1.0 / z2 + 3.0 / (z2 * z2)).ln()
    }
}

/// One estimated parameter with its standard error and confidence limits.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterEstimate {
    /// Parameter name.
    pub name: &'static str,
    /// Point estimate.
    pub estimate: f64,
    /// Standard error (delta method where transformed).
    pub std_error: f64,
    /// Lower confidence limit.
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
}

/// A maximum-likelihood life-distribution fit.
#[derive(Debug, Clone, PartialEq)]
pub struct ParametricFit {
    /// Which distribution.
    pub distribution: LifeDistribution,
    /// Location `μ̂` of `ln T`.
    pub location: f64,
    /// Scale `σ̂` of `ln T`.
    pub scale: f64,
    /// Covariance of `(μ̂, σ̂)`.
    pub covariance: [[f64; 2]; 2],
    /// Maximized log-likelihood (time scale).
    pub log_likelihood: f64,
    /// Observations.
    pub n: usize,
    /// Events.
    pub n_events: usize,
    /// Newton iterations used.
    pub iterations: usize,
}

impl ParametricFit {
    /// Akaike information criterion `−2ℓ + 4`.
    #[must_use]
    pub fn aic(&self) -> f64 {
        -2.0 * self.log_likelihood + 4.0
    }

    /// Bayesian information criterion `−2ℓ + 2 ln n`.
    #[must_use]
    pub fn bic(&self) -> f64 {
        -2.0 * self.log_likelihood + 2.0 * (self.n as f64).ln()
    }

    /// Weibull scale (characteristic life) `α = e^μ`, the time by which
    /// `1 − e⁻¹ ≈ 63.2%` have failed. Meaningful for Weibull fits.
    #[must_use]
    pub fn weibull_scale(&self) -> f64 {
        self.location.exp()
    }

    /// Weibull shape `β = 1/σ` (`β > 1`: hazard increases with age).
    #[must_use]
    pub fn weibull_shape(&self) -> f64 {
        1.0 / self.scale
    }

    fn z(&self, t: f64) -> f64 {
        if t <= 0.0 { f64::NEG_INFINITY } else { (t.ln() - self.location) / self.scale }
    }

    /// Fitted survival `S(t)`.
    #[must_use]
    pub fn survival(&self, t: f64) -> f64 {
        if t <= 0.0 { 1.0 } else { self.distribution.standard_survival(self.z(t)) }
    }

    /// Fitted cumulative failure probability `F(t) = 1 − S(t)`.
    #[must_use]
    pub fn cdf(&self, t: f64) -> f64 {
        1.0 - self.survival(t)
    }

    /// Fitted density `f(t)`.
    #[must_use]
    pub fn pdf(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        let (lf, _, _) = self.distribution.log_density(self.z(t));
        (lf - self.scale.ln() - t.ln()).exp()
    }

    /// Hazard rate `h(t) = f(t)/S(t)`.
    #[must_use]
    pub fn hazard(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return if self.distribution == LifeDistribution::Weibull && self.scale > 1.0 {
                f64::INFINITY
            } else {
                0.0
            };
        }
        let z = self.z(t);
        let (lf, _, _) = self.distribution.log_density(z);
        let (ls, _, _) = self.distribution.log_survival(z);
        (lf - ls - self.scale.ln() - t.ln()).exp()
    }

    /// Time by which a fraction `p` has failed, `exp(μ̂ + σ̂ w_p)`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < p < 1`.
    pub fn quantile(&self, p: f64) -> Result<f64> {
        if !(p > 0.0 && p < 1.0) {
            return Err(invalid(format!("quantile probability must lie strictly between 0 and 1 (got {p})")));
        }
        Ok((self.location + self.scale * self.distribution.standard_quantile(p)?).exp())
    }

    /// Median life.
    #[must_use]
    pub fn median(&self) -> f64 {
        self.quantile(0.5).unwrap_or(f64::NAN)
    }

    fn variance_of(&self, w: f64) -> f64 {
        let v = &self.covariance;
        (v[0][0] + 2.0 * w * v[0][1] + w * w * v[1][1]).max(0.0)
    }

    /// Delta-method `level` confidence interval for the `p`-quantile, formed on
    /// the log-time scale.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for `p` or `level` outside `(0, 1)`.
    pub fn quantile_confidence_interval(&self, p: f64, level: f64) -> Result<(f64, f64)> {
        if !(p > 0.0 && p < 1.0) {
            return Err(invalid(format!("quantile probability must lie strictly between 0 and 1 (got {p})")));
        }
        let zc = critical(level)?;
        let w = self.distribution.standard_quantile(p)?;
        let y = self.location + self.scale * w;
        let se = self.variance_of(w).sqrt();
        Ok(((y - zc * se).exp(), (y + zc * se).exp()))
    }

    /// Pointwise `level` confidence limits for `S(t)`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn survival_confidence_limits(&self, t: f64, level: f64) -> Result<(f64, f64)> {
        let zc = critical(level)?;
        if t <= 0.0 {
            return Ok((1.0, 1.0));
        }
        let z = self.z(t);
        let se = self.variance_of(z).sqrt() / self.scale;
        let d = self.distribution;
        Ok((d.standard_survival(z + zc * se), d.standard_survival(z - zc * se)))
    }

    /// Parameter table with `level` Wald limits: for Weibull the scale `α` and
    /// shape `β`; for lognormal the log-scale location `μ` and scale `σ`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] unless `0 < level < 1`.
    pub fn parameters(&self, level: f64) -> Result<Vec<ParameterEstimate>> {
        let zc = critical(level)?;
        let (mu, sigma) = (self.location, self.scale);
        let se_mu = self.covariance[0][0].max(0.0).sqrt();
        let se_sigma = self.covariance[1][1].max(0.0).sqrt();
        let (sig_lo, sig_hi) = (sigma * (-zc * se_sigma / sigma).exp(), sigma * (zc * se_sigma / sigma).exp());
        Ok(match self.distribution {
            LifeDistribution::Weibull => vec![
                ParameterEstimate {
                    name: "Scale α",
                    estimate: mu.exp(),
                    std_error: mu.exp() * se_mu,
                    lower: (mu - zc * se_mu).exp(),
                    upper: (mu + zc * se_mu).exp(),
                },
                ParameterEstimate {
                    name: "Shape β",
                    estimate: 1.0 / sigma,
                    std_error: se_sigma / (sigma * sigma),
                    lower: 1.0 / sig_hi,
                    upper: 1.0 / sig_lo,
                },
            ],
            LifeDistribution::Lognormal => vec![
                ParameterEstimate {
                    name: "Location μ (log scale)",
                    estimate: mu,
                    std_error: se_mu,
                    lower: mu - zc * se_mu,
                    upper: mu + zc * se_mu,
                },
                ParameterEstimate {
                    name: "Scale σ (log scale)",
                    estimate: sigma,
                    std_error: se_sigma,
                    lower: sig_lo,
                    upper: sig_hi,
                },
            ],
        })
    }
}

fn critical(level: f64) -> Result<f64> {
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid(format!("confidence level must lie strictly between 0 and 1 (got {level})")));
    }
    Normal::standard().quantile(0.5 + level / 2.0)
}

/// Log-likelihood on the log-time scale with gradient and Hessian in `(μ, σ)`.
struct Evaluation {
    value: f64,
    gradient: [f64; 2],
    hessian: [[f64; 2]; 2],
}

fn evaluate(dist: LifeDistribution, y: &[f64], events: &[bool], mu: f64, sigma: f64) -> Evaluation {
    let (mut value, mut g_mu, mut g_s) = (0.0, 0.0, 0.0);
    let (mut h_mm, mut h_ms, mut h_ss) = (0.0, 0.0, 0.0);
    let s2 = sigma * sigma;
    for (&yi, &event) in y.iter().zip(events) {
        let z = (yi - mu) / sigma;
        let (l, w, v) = if event { dist.log_density(z) } else { dist.log_survival(z) };
        value += l;
        g_mu -= w / sigma;
        g_s -= z * w / sigma;
        h_mm += v / s2;
        h_ms += (w + z * v) / s2;
        h_ss += (2.0 * z * w + z * z * v) / s2;
        if event {
            value -= sigma.ln();
            g_s -= 1.0 / sigma;
            h_ss += 1.0 / s2;
        }
    }
    Evaluation { value, gradient: [g_mu, g_s], hessian: [[h_mm, h_ms], [h_ms, h_ss]] }
}

/// Fit `distribution` by maximum likelihood to observed `times` (all `> 0`)
/// with event indicators (`false` = right-censored).
///
/// # Errors
/// [`StatError::DimensionMismatch`] for slices of different lengths,
/// [`StatError::InvalidArgument`] for non-positive or non-finite times, no
/// events, or fewer than two distinct times; [`StatError::ConvergenceFailure`]
/// if the likelihood has no interior maximum (for example when the scale
/// collapses towards zero).
pub fn fit_life_distribution(times: &[f64], events: &[bool], distribution: LifeDistribution) -> Result<ParametricFit> {
    if times.len() != events.len() {
        return Err(StatError::DimensionMismatch {
            context: "survival times and event indicators",
            expected: times.len(),
            found: events.len(),
        });
    }
    if times.iter().any(|t| !t.is_finite() || *t <= 0.0) {
        return Err(invalid("life-distribution fits need finite, positive times"));
    }
    let n_events = events.iter().filter(|&&e| e).count();
    if n_events == 0 {
        return Err(invalid("a life-distribution fit needs at least one event"));
    }
    let y: Vec<f64> = times.iter().map(|t| t.ln()).collect();
    let (lo, hi) = y.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    if hi - lo <= 1e-12 * hi.abs().max(1.0) {
        return Err(invalid("a life-distribution fit needs at least two distinct times"));
    }
    // Starting values from the moments of the log times (events weighted fully).
    let n = y.len() as f64;
    let mean = y.iter().sum::<f64>() / n;
    let sd = (y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt().max(1e-3 * (hi - lo));
    let (mut mu, mut sigma) = match distribution {
        LifeDistribution::Weibull => {
            let s = sd * 6f64.sqrt() / std::f64::consts::PI;
            (mean + 0.577_215_664_901_532_9 * s, s)
        }
        LifeDistribution::Lognormal => (mean, sd),
    };
    let mut current = evaluate(distribution, &y, events, mu, sigma);
    let mut damping = 1e-3;
    const MAX_ITER: usize = 500;
    for iter in 1..=MAX_ITER {
        // Gradient and Hessian with respect to (μ, τ = ln σ).
        let [g_mu, g_s] = current.gradient;
        let [[h_mm, h_ms], [_, h_ss]] = current.hessian;
        let g = [g_mu, sigma * g_s];
        let h = [[h_mm, sigma * h_ms], [sigma * h_ms, sigma * sigma * h_ss + sigma * g_s]];
        let grad_norm = (g[0] * g[0] + g[1] * g[1]).sqrt();
        if grad_norm < 1e-11 * (1.0 + current.value.abs()) {
            return finish(distribution, &y, events, mu, sigma, n_events, iter, &current);
        }
        let mut accepted = false;
        for _ in 0..60 {
            // Solve (−H + λ·diag) Δ = g.
            let a = [
                [-h[0][0] + damping * (1.0 + h[0][0].abs()), -h[0][1]],
                [-h[1][0], -h[1][1] + damping * (1.0 + h[1][1].abs())],
            ];
            let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
            if !(a[0][0] > 0.0 && det > 0.0) {
                damping *= 10.0;
                continue;
            }
            let d_mu = (a[1][1] * g[0] - a[0][1] * g[1]) / det;
            let d_tau = (a[0][0] * g[1] - a[1][0] * g[0]) / det;
            let (new_mu, new_sigma) = (mu + d_mu, sigma * d_tau.clamp(-5.0, 5.0).exp());
            let trial = evaluate(distribution, &y, events, new_mu, new_sigma);
            if trial.value.is_finite() && trial.value >= current.value - 1e-12 * current.value.abs() {
                let small = d_mu.abs() < 1e-13 * (1.0 + mu.abs()) && d_tau.abs() < 1e-13;
                mu = new_mu;
                sigma = new_sigma;
                current = trial;
                damping = (damping * 0.1).max(1e-12);
                accepted = true;
                if small {
                    return finish(distribution, &y, events, mu, sigma, n_events, iter, &current);
                }
                break;
            }
            damping *= 10.0;
        }
        if !accepted || !(sigma > 1e-8 && sigma < 1e8) || !mu.is_finite() {
            break;
        }
    }
    Err(StatError::ConvergenceFailure { routine: "censored life-distribution likelihood", iterations: MAX_ITER })
}

#[allow(clippy::too_many_arguments)]
fn finish(
    distribution: LifeDistribution,
    y: &[f64],
    events: &[bool],
    mu: f64,
    sigma: f64,
    n_events: usize,
    iterations: usize,
    eval: &Evaluation,
) -> Result<ParametricFit> {
    let [[a, b], [_, d]] = eval.hessian;
    // Observed information −H and its inverse.
    let (ia, ib, id) = (-a, -b, -d);
    let det = ia * id - ib * ib;
    if !(ia > 0.0 && det > 0.0) {
        return Err(StatError::ConvergenceFailure { routine: "censored life-distribution likelihood", iterations });
    }
    let covariance = [[id / det, -ib / det], [-ib / det, ia / det]];
    let log_t_events: f64 = y.iter().zip(events).filter(|e| *e.1).map(|e| *e.0).sum();
    Ok(ParametricFit {
        distribution,
        location: mu,
        scale: sigma,
        covariance,
        log_likelihood: eval.value - log_t_events,
        n: y.len(),
        n_events,
        iterations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kaplan_meier::tests::{placebo, six_mp};

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    /// Reference values from an independent solution of the likelihood
    /// equations (SciPy 1.18 `fsolve` on a fourth-order finite-difference
    /// gradient of the log-likelihood written directly from its definition;
    /// stable to ~1e-11 under halving the step). `scipy.stats.weibull_min.fit`
    /// on `CensoredData` agrees to its optimizer tolerance (shape 1.353731,
    /// scale 33.76518).
    #[test]
    fn weibull_six_mp() {
        let (t, e) = six_mp();
        let fit = fit_life_distribution(&t, &e, LifeDistribution::Weibull).unwrap();
        close(fit.location, 3.519_429_234_144_641, 1e-9);
        close(fit.scale, 0.738_697_272_174_849_8, 1e-9);
        close(fit.log_likelihood, -41.658_678_476_881_53, 1e-11);
        close(fit.weibull_scale(), 33.765_150_967_631_23, 1e-9);
        close(fit.weibull_shape(), 1.353_734_523_827_102_8, 1e-9);
        close(fit.median(), 25.756_503_014_909_217, 1e-9);
        close(fit.survival(10.0), 0.824_834_730_160_062_3, 1e-9);
        // Standard errors from a finite-difference Hessian of the reference.
        close(fit.covariance[0][0].sqrt(), 0.273_368_913_736_879_9, 1e-5);
        close(fit.covariance[1][1].sqrt(), 0.205_651_747_836_522_9, 1e-5);
        close(fit.covariance[0][1], 0.024_419_928_684_666_43, 1e-5);
        close(fit.aic(), 2.0 * 41.658_678_476_881_53 + 4.0, 1e-11);
        // Consistency of the parametrisations.
        let s = fit.survival(fit.median());
        close(s, 0.5, 1e-12);
        close(fit.survival(fit.weibull_scale()), (-1.0f64).exp(), 1e-12);
        let p = fit.parameters(0.95).unwrap();
        assert_eq!(p[0].name, "Scale α");
        assert!(p[0].lower < p[0].estimate && p[0].estimate < p[0].upper);
        assert!(p[1].lower < p[1].estimate && p[1].estimate < p[1].upper);
        let (lo, hi) = fit.quantile_confidence_interval(0.5, 0.95).unwrap();
        assert!(lo < fit.median() && fit.median() < hi);
        let (slo, shi) = fit.survival_confidence_limits(20.0, 0.95).unwrap();
        assert!(slo < fit.survival(20.0) && fit.survival(20.0) < shi);
    }

    #[test]
    fn lognormal_six_mp() {
        let (t, e) = six_mp();
        let fit = fit_life_distribution(&t, &e, LifeDistribution::Lognormal).unwrap();
        close(fit.location, 3.203_067_652_470_761_3, 1e-9);
        close(fit.scale, 0.978_724_792_995_037_7, 1e-9);
        close(fit.log_likelihood, -40.680_155_869_907_864, 1e-11);
        close(fit.median(), 24.607_903_023_693_45, 1e-9);
        close(fit.survival(10.0), 0.821_228_503_332_445_2, 1e-9);
        close(fit.covariance[0][0].sqrt(), 0.286_132_016_600_771_64, 1e-5);
        close(fit.covariance[1][1].sqrt(), 0.250_591_360_070_402_3, 1e-5);
    }

    #[test]
    fn weibull_placebo_uncensored() {
        let t = placebo();
        let fit = fit_life_distribution(&t, &vec![true; t.len()], LifeDistribution::Weibull).unwrap();
        close(fit.location, 2.249_410_161_600_512, 1e-9);
        close(fit.scale, 0.729_660_782_688_073_6, 1e-9);
        close(fit.log_likelihood, -64.920_107_589_012_37, 1e-11);
        close(fit.median(), 7.257_098_237_277_685_5, 1e-9);
        close(fit.survival(10.0), 0.341_093_922_036_507_74, 1e-9);
    }

    /// Without censoring the lognormal MLE is closed-form: μ̂ is the mean of
    /// the log times and σ̂² their mean squared deviation (divisor n), with
    /// `Var(μ̂) = σ̂²/n`, `Var(σ̂) = σ̂²/(2n)`, `Cov = 0`.
    #[test]
    fn lognormal_uncensored_closed_form() {
        let t = placebo();
        let y: Vec<f64> = t.iter().map(|v| v.ln()).collect();
        let n = y.len() as f64;
        let mean = y.iter().sum::<f64>() / n;
        let sigma = (y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
        let fit = fit_life_distribution(&t, &vec![true; t.len()], LifeDistribution::Lognormal).unwrap();
        close(fit.location, mean, 1e-12);
        close(fit.scale, sigma, 1e-12);
        close(fit.covariance[0][0], sigma * sigma / n, 1e-10);
        close(fit.covariance[1][1], sigma * sigma / (2.0 * n), 1e-10);
        assert!(fit.covariance[0][1].abs() < 1e-12);
        close(fit.median(), 6.203_545_727_740_177_5, 1e-11);
        close(fit.survival(10.0), 0.298_532_287_641_005_3, 1e-10);
        // Log-likelihood in closed form: −Σ ln t − n ln σ − n/2 ln 2π − n/2.
        let ll = -y.iter().sum::<f64>() - n * sigma.ln() - n * LN_SQRT_2PI - n / 2.0;
        close(fit.log_likelihood, ll, 1e-12);
    }

    #[test]
    fn exponential_special_case_is_recovered() {
        // Uncensored data with shape near 1: hazard is nearly constant.
        let t: Vec<f64> = (1..=200).map(|i| -(1.0 - (f64::from(i) - 0.5) / 200.0).ln() * 50.0).collect();
        let fit = fit_life_distribution(&t, &vec![true; t.len()], LifeDistribution::Weibull).unwrap();
        assert!((fit.weibull_shape() - 1.0).abs() < 0.05, "{}", fit.weibull_shape());
        assert!((fit.weibull_scale() - 50.0).abs() < 2.0);
        let h1 = fit.hazard(10.0);
        let h2 = fit.hazard(100.0);
        assert!((h1 / h2 - 1.0).abs() < 0.15);
        close(fit.pdf(30.0) / fit.survival(30.0), fit.hazard(30.0), 1e-12);
    }

    #[test]
    fn heavy_censoring_far_tail_is_stable() {
        // One early failure, many units still running much later.
        let mut t = vec![1.0];
        let mut e = vec![true];
        t.extend((0..30).map(|i| 100.0 + f64::from(i)));
        e.extend(std::iter::repeat_n(false, 30));
        for d in [LifeDistribution::Weibull, LifeDistribution::Lognormal] {
            let fit = fit_life_distribution(&t, &e, d).unwrap();
            assert!(fit.log_likelihood.is_finite());
            assert!(fit.median() > 100.0, "{d:?}: {}", fit.median());
        }
    }

    #[test]
    fn rejects_bad_input() {
        let d = LifeDistribution::Weibull;
        assert!(fit_life_distribution(&[1.0, 2.0], &[false, false], d).is_err(), "no events");
        assert!(fit_life_distribution(&[0.0, 2.0], &[true, true], d).is_err(), "zero time");
        assert!(fit_life_distribution(&[3.0, 3.0], &[true, true], d).is_err(), "one distinct time");
        assert!(fit_life_distribution(&[3.0], &[true, true], d).is_err());
    }

    /// The documented error for a probability outside (0, 1); before the
    /// check, p = 0 gave (0, NaN) and p = 1.5 gave (NaN, NaN) for a Weibull fit.
    #[test]
    fn quantile_interval_rejects_bad_probability() {
        let fit = fit_life_distribution(&[1.0, 2.0, 4.0], &[true; 3], LifeDistribution::Weibull).unwrap();
        for p in [0.0, 1.0, 1.5, -0.1, f64::NAN] {
            assert!(fit.quantile_confidence_interval(p, 0.95).is_err(), "p = {p}");
        }
        assert!(fit.quantile_confidence_interval(0.5, 0.95).is_ok());
    }

    #[test]
    fn normal_tail_is_accurate() {
        // ln Q(30) from erfc agrees with the asymptotic series used beyond underflow.
        let direct = ln_normal_sf(30.0);
        let expected = -0.5 * 900.0 - 30f64.ln() - LN_SQRT_2PI + (1.0f64 - 1.0 / 900.0 + 3.0 / 810_000.0).ln();
        assert!((direct - expected).abs() < 1e-6);
        assert!(ln_normal_sf(50.0).is_finite());
    }
}
