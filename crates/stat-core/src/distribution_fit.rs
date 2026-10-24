//! Maximum-likelihood fits of continuous distributions to one sample, with
//! Anderson–Darling goodness of fit, for identifying which distribution
//! describes a set of measurements (and, later, for non-normal capability and
//! life-data analysis).
//!
//! # Families
//! Eight of the families are **location–scale** families on the data scale
//! or on the log scale: with standard density `f₀` and `z = (y − μ)/σ`,
//!
//! | Family | `y` | standard distribution `F₀(z)` |
//! |---|---|---|
//! | Normal | `x` | `Φ(z)` |
//! | Logistic | `x` | `1/(1 + e^{−z})` |
//! | Smallest extreme value | `x` | `1 − exp(−eᶻ)` |
//! | Largest extreme value | `x` | `exp(−e^{−z})` |
//! | Lognormal | `ln(x − γ)` | `Φ(z)` |
//! | Log-logistic | `ln(x − γ)` | `1/(1 + e^{−z})` |
//! | Weibull | `ln(x − γ)` | `1 − exp(−eᶻ)`; shape `β = 1/σ`, scale `α = e^μ` |
//!
//! The log families have a two-parameter form (threshold `γ = 0`) and a
//! three-parameter form with the threshold estimated. The **exponential**
//! (`F = 1 − e^{−(x−γ)/θ}`) and **gamma** (`F = P(k, (x−γ)/θ)`, the
//! regularized incomplete gamma function) families have the same two forms.
//! (Meeker & Escobar, *Statistical Methods for Reliability Data*, ch. 4–5 and
//! 11; Johnson, Kotz & Balakrishnan, *Continuous Univariate Distributions*.)
//!
//! # Estimation
//! * Normal: `μ̂ = x̄`; the reported `σ` is the sample standard deviation
//!   (divisor `n − 1`) so it agrees with the summary statistics; the
//!   log-likelihood is evaluated at the maximum-likelihood `σ̂` (divisor `n`).
//! * The other location–scale families: damped Newton iterations on
//!   `(μ, ln σ)` with analytic derivatives of
//!   `ℓ = Σ ln f₀(zᵢ) − n ln σ`, from moment starting values.
//! * Gamma: `θ̂ = ū/k̂` with `k̂` solving `ln k − ψ(k) = ln ū − mean(ln u)`
//!   (Newton from Minka's 2002 starting value).
//! * Exponential: `θ̂ = ū`. With a threshold the maximum-likelihood
//!   estimates are `γ̂ = x₍₁₎`, `θ̂ = x̄ − x₍₁₎`; because the fitted
//!   distribution then gives the smallest value probability zero, the reported
//!   parameters are the unbiased estimates `θ* = n(x̄ − x₍₁₎)/(n − 1)`,
//!   `γ* = x₍₁₎ − θ*/n` (Johnson, Kotz & Balakrishnan, vol. 1, §19.4), while
//!   the log-likelihood (for AIC and the threshold test) is the maximum.
//! * Three-parameter forms: the profile log-likelihood
//!   `ℓ_p(γ) = max_{μ,σ} ℓ(γ, μ, σ)` is searched over `γ = x₍₁₎ − d`,
//!   `d = r·e^t` (`r` the sample range) on a grid of `t`, then refined by
//!   golden-section search. The likelihood of these families can grow without
//!   bound as `γ → x₍₁₎` (Weibull or gamma shape below one; Hill 1963 for the
//!   lognormal); the estimate is the largest **interior** local maximum
//!   (Cohen & Whitten 1980), and a fit with none is reported as failing.
//!
//! # Goodness of fit
//! The Anderson–Darling statistic with estimated parameters is
//! `A² = −n − (1/n) Σ (2i − 1)[ln F(x₍ᵢ₎) + ln(1 − F(x₍ₙ₊₁₋ᵢ₎))]`
//! (D'Agostino & Stephens, *Goodness-of-Fit Techniques*, 1986, ch. 4).
//! Its null distribution depends on the family and on which parameters were
//! estimated, so p-values come from:
//! * the normal: the published formula for the modified statistic
//!   `A*  = A²(1 + 0.75/n + 2.25/n²)` ([`crate::normality::anderson_darling`]);
//! * every other family: Monte Carlo simulation (a parametric bootstrap).
//!   For the location–scale and exponential families the statistic's null
//!   distribution does not depend on the parameter values, so it is simulated
//!   from the standard distribution once per family and sample size and
//!   cached; for the gamma and three-parameter families it depends on the
//!   shape, and samples are drawn from the fitted distribution. Samples are
//!   simulated at size `min(n, 250)`; beyond that the percentage points change
//!   by `O(1/n)` (Stephens 1974, *JASA* 69) and the published modifications
//!   for the normal, exponential, logistic and extreme-value families are
//!   applied to both sides of the comparison. The p-value is
//!   `(1 + #{A²_sim ≥ A²})/(B + 1)`.
//!
//! The likelihood-ratio test of a threshold compares each three-parameter
//! form with its two-parameter form: `2(ℓ₃ − ℓ₂)` against χ²₁.
//!
//! [`johnson_transformation`] chooses and fits a Johnson system transformation
//! to normality by the percentile method (Slifker & Shapiro 1980,
//! *Technometrics* 22; selection over the percentile spacing as in Chou,
//! Polansky & Mason 1998, *J. Quality Technology* 30).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;

use crate::distributions::{ChiSquared, ContinuousDistribution, Normal};
use crate::error::{Result, StatError, invalid};
use crate::normality::{NormalityTest, anderson_darling};
use crate::rng::Rng;
use crate::special::{digamma, erfc, ln_gamma, regularized_gamma_p, regularized_gamma_q, trigamma};

const LN_SQRT_2PI: f64 = 0.918_938_533_204_672_8;
const EULER: f64 = 0.577_215_664_901_532_9;

/// Largest sample size simulated for Anderson–Darling p-values.
const MAX_SIMULATED_N: usize = 250;

/// A family of continuous distributions that can be fitted to a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FitFamily {
    /// Normal (Gaussian).
    Normal,
    /// Lognormal: `ln X` is normal.
    Lognormal,
    /// Lognormal with a threshold: `ln(X − γ)` is normal.
    Lognormal3,
    /// Exponential with mean `θ`.
    Exponential,
    /// Exponential with a threshold (two-parameter exponential).
    Exponential2,
    /// Weibull with shape `β` and scale `α`.
    Weibull,
    /// Weibull with a threshold.
    Weibull3,
    /// Gamma with shape `k` and scale `θ`.
    Gamma,
    /// Gamma with a threshold.
    Gamma3,
    /// Logistic.
    Logistic,
    /// Log-logistic: `ln X` is logistic.
    LogLogistic,
    /// Log-logistic with a threshold.
    LogLogistic3,
    /// Smallest extreme value (Gumbel for minima).
    SmallestExtremeValue,
    /// Largest extreme value (Gumbel for maxima).
    LargestExtremeValue,
}

impl FitFamily {
    /// Every family, in the order reports list them.
    pub const ALL: [Self; 14] = [
        Self::Normal,
        Self::Lognormal,
        Self::Lognormal3,
        Self::Exponential,
        Self::Exponential2,
        Self::Weibull,
        Self::Weibull3,
        Self::Gamma,
        Self::Gamma3,
        Self::Logistic,
        Self::LogLogistic,
        Self::LogLogistic3,
        Self::SmallestExtremeValue,
        Self::LargestExtremeValue,
    ];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Lognormal => "Lognormal",
            Self::Lognormal3 => "Lognormal with threshold",
            Self::Exponential => "Exponential",
            Self::Exponential2 => "Exponential with threshold",
            Self::Weibull => "Weibull",
            Self::Weibull3 => "Weibull with threshold",
            Self::Gamma => "Gamma",
            Self::Gamma3 => "Gamma with threshold",
            Self::Logistic => "Logistic",
            Self::LogLogistic => "Log-logistic",
            Self::LogLogistic3 => "Log-logistic with threshold",
            Self::SmallestExtremeValue => "Smallest extreme value",
            Self::LargestExtremeValue => "Largest extreme value",
        }
    }

    /// Stable key for saved files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Lognormal => "lognormal",
            Self::Lognormal3 => "lognormal-threshold",
            Self::Exponential => "exponential",
            Self::Exponential2 => "exponential-threshold",
            Self::Weibull => "weibull",
            Self::Weibull3 => "weibull-threshold",
            Self::Gamma => "gamma",
            Self::Gamma3 => "gamma-threshold",
            Self::Logistic => "logistic",
            Self::LogLogistic => "log-logistic",
            Self::LogLogistic3 => "log-logistic-threshold",
            Self::SmallestExtremeValue => "smallest-extreme-value",
            Self::LargestExtremeValue => "largest-extreme-value",
        }
    }

    /// The family with this [`key`](Self::key).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == key)
    }

    /// Number of estimated parameters.
    #[must_use]
    pub fn n_parameters(self) -> usize {
        match self {
            Self::Exponential => 1,
            Self::Lognormal3 | Self::Weibull3 | Self::Gamma3 | Self::LogLogistic3 => 3,
            _ => 2,
        }
    }

    /// `true` when the family estimates a threshold (lower bound).
    #[must_use]
    pub fn has_threshold(self) -> bool {
        matches!(self, Self::Lognormal3 | Self::Exponential2 | Self::Weibull3 | Self::Gamma3 | Self::LogLogistic3)
    }

    /// The same family without its threshold (for the likelihood-ratio test).
    #[must_use]
    pub fn without_threshold(self) -> Option<Self> {
        match self {
            Self::Lognormal3 => Some(Self::Lognormal),
            Self::Exponential2 => Some(Self::Exponential),
            Self::Weibull3 => Some(Self::Weibull),
            Self::Gamma3 => Some(Self::Gamma),
            Self::LogLogistic3 => Some(Self::LogLogistic),
            _ => None,
        }
    }

    /// `true` when the family needs every value to be positive (a threshold of zero).
    #[must_use]
    pub fn needs_positive(self) -> bool {
        matches!(self, Self::Lognormal | Self::Exponential | Self::Weibull | Self::Gamma | Self::LogLogistic)
    }

    /// The standard location–scale distribution and whether it applies to `ln(x − γ)`.
    fn standard(self) -> Option<(Standard, bool)> {
        match self {
            Self::Normal => Some((Standard::Normal, false)),
            Self::Logistic => Some((Standard::Logistic, false)),
            Self::SmallestExtremeValue => Some((Standard::Sev, false)),
            Self::LargestExtremeValue => Some((Standard::Lev, false)),
            Self::Lognormal | Self::Lognormal3 => Some((Standard::Normal, true)),
            Self::LogLogistic | Self::LogLogistic3 => Some((Standard::Logistic, true)),
            Self::Weibull | Self::Weibull3 => Some((Standard::Sev, true)),
            _ => None,
        }
    }
}

/// A standard location–scale distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Standard {
    Normal,
    Logistic,
    /// Smallest extreme value.
    Sev,
    /// Largest extreme value.
    Lev,
}

/// `ln(1 + eˣ)` without overflow.
fn softplus(x: f64) -> f64 {
    if x > 0.0 { x + (-x).exp().ln_1p() } else { x.exp().ln_1p() }
}

/// `ln Q(z)`, the standard normal upper tail, accurate far into the tail.
pub(crate) fn ln_normal_sf(z: f64) -> f64 {
    let q = 0.5 * erfc(z * std::f64::consts::FRAC_1_SQRT_2);
    if q > 1e-300 {
        q.ln()
    } else {
        // Asymptotic expansion (A&S 26.2.12).
        let z2 = z * z;
        -0.5 * z2 - z.ln() - LN_SQRT_2PI + (1.0 - 1.0 / z2 + 3.0 / (z2 * z2)).ln()
    }
}

impl Standard {
    /// `ln f₀(z)` and its first two derivatives.
    fn log_density(self, z: f64) -> (f64, f64, f64) {
        match self {
            Self::Normal => (-0.5 * z * z - LN_SQRT_2PI, -z, -1.0),
            Self::Logistic => {
                // F = 1/(1 + e^{−z}); ln f₀ = −z − 2 ln(1 + e^{−z}).
                let f = 1.0 / (1.0 + (-z).exp());
                (-z - 2.0 * softplus(-z), 1.0 - 2.0 * f, -2.0 * f * (1.0 - f))
            }
            Self::Sev => {
                let e = z.exp();
                (z - e, 1.0 - e, -e)
            }
            Self::Lev => {
                let e = (-z).exp();
                (-z - e, e - 1.0, -e)
            }
        }
    }

    /// `ln F₀(z)`.
    fn ln_cdf(self, z: f64) -> f64 {
        match self {
            Self::Normal => ln_normal_sf(-z),
            Self::Logistic => -softplus(-z),
            Self::Sev => {
                let e = z.exp();
                if e < 1e-8 { z + (-0.5 * e).ln_1p() } else { (-(-e).exp_m1()).ln() }
            }
            Self::Lev => -(-z).exp(),
        }
    }

    /// `ln(1 − F₀(z))`.
    fn ln_sf(self, z: f64) -> f64 {
        match self {
            Self::Normal => ln_normal_sf(z),
            Self::Logistic => -softplus(z),
            Self::Sev => -z.exp(),
            Self::Lev => {
                let e = (-z).exp();
                if e < 1e-8 { -z + (-0.5 * e).ln_1p() } else { (-(-e).exp_m1()).ln() }
            }
        }
    }

    /// `F₀⁻¹(p)` for `0 < p < 1`.
    fn quantile(self, p: f64) -> f64 {
        match self {
            Self::Normal => Normal::standard().quantile(p).unwrap_or(f64::NAN),
            Self::Logistic => (p / (1.0 - p)).ln(),
            Self::Sev => (-(-p).ln_1p()).ln(),
            Self::Lev => -(-p.ln()).ln(),
        }
    }

    /// Moment starting values `(μ, σ)` from the mean and standard deviation of `y`.
    fn start(self, mean: f64, sd: f64) -> (f64, f64) {
        let pi = std::f64::consts::PI;
        match self {
            Self::Normal => (mean, sd),
            Self::Logistic => (mean, sd * 3f64.sqrt() / pi),
            Self::Sev => {
                let s = sd * 6f64.sqrt() / pi;
                (mean + EULER * s, s)
            }
            Self::Lev => {
                let s = sd * 6f64.sqrt() / pi;
                (mean - EULER * s, s)
            }
        }
    }

    /// The modification of `A²` that makes its percentage points nearly
    /// independent of `n` when location and scale are estimated (D'Agostino &
    /// Stephens 1986, Tables 4.7, 4.17 and 4.22).
    fn ad_modifier(self, n: usize) -> f64 {
        let n = n as f64;
        match self {
            Self::Normal => 1.0 + 0.75 / n + 2.25 / (n * n),
            Self::Logistic => 1.0 + 0.25 / n,
            Self::Sev | Self::Lev => 1.0 + 0.2 / n.sqrt(),
        }
    }
}

/// A fitted distribution.
#[derive(Debug, Clone, PartialEq)]
pub struct FittedDistribution {
    /// The family.
    pub family: FitFamily,
    /// Threshold (lower bound) `γ`; `0` for the two-parameter log families,
    /// exponential and gamma, and unused by the families on the whole real line.
    pub threshold: f64,
    /// Location `μ` (on the log scale for the log families); unused by the
    /// exponential and gamma.
    pub location: f64,
    /// Scale: `σ` for the location–scale families (on the log scale for the
    /// log families), `θ` for the exponential and gamma.
    pub scale: f64,
    /// Gamma shape `k` (`1` for the exponential; unused otherwise).
    pub shape: f64,
    /// Maximized log-likelihood on the data scale.
    pub log_likelihood: f64,
    /// Sample size.
    pub n: usize,
}

/// One named parameter of a fitted distribution.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedParameter {
    /// Display name.
    pub name: &'static str,
    /// Estimate.
    pub value: f64,
}

impl FittedDistribution {
    /// Akaike's information criterion `−2ℓ + 2p`.
    #[must_use]
    pub fn aic(&self) -> f64 {
        -2.0 * self.log_likelihood + 2.0 * self.family.n_parameters() as f64
    }

    /// Small-sample corrected AIC `AIC + 2p(p + 1)/(n − p − 1)`
    /// (infinite when `n ≤ p + 1`).
    #[must_use]
    pub fn aicc(&self) -> f64 {
        let p = self.family.n_parameters() as f64;
        let n = self.n as f64;
        if n - p - 1.0 > 0.0 { self.aic() + 2.0 * p * (p + 1.0) / (n - p - 1.0) } else { f64::INFINITY }
    }

    /// Schwarz's Bayesian information criterion `−2ℓ + p ln n`.
    #[must_use]
    pub fn bic(&self) -> f64 {
        -2.0 * self.log_likelihood + self.family.n_parameters() as f64 * (self.n as f64).ln()
    }

    /// The parameters in the family's usual form, e.g. a Weibull's shape `β`
    /// and scale `α`.
    #[must_use]
    pub fn parameters(&self) -> Vec<NamedParameter> {
        let p = |name, value| NamedParameter { name, value };
        let mut out = match self.family {
            FitFamily::Normal
            | FitFamily::Logistic
            | FitFamily::SmallestExtremeValue
            | FitFamily::LargestExtremeValue => {
                vec![p("Location μ", self.location), p("Scale σ", self.scale)]
            }
            FitFamily::Lognormal | FitFamily::Lognormal3 | FitFamily::LogLogistic | FitFamily::LogLogistic3 => {
                vec![p("Location μ (log scale)", self.location), p("Scale σ (log scale)", self.scale)]
            }
            FitFamily::Weibull | FitFamily::Weibull3 => {
                vec![p("Shape β", 1.0 / self.scale), p("Scale α", self.location.exp())]
            }
            FitFamily::Exponential | FitFamily::Exponential2 => vec![p("Scale θ (mean)", self.scale)],
            FitFamily::Gamma | FitFamily::Gamma3 => vec![p("Shape k", self.shape), p("Scale θ", self.scale)],
        };
        if self.family.has_threshold() {
            out.push(p("Threshold γ", self.threshold));
        }
        out
    }

    /// `ln F(x)` (`−∞` at or below a threshold).
    #[must_use]
    pub fn ln_cdf(&self, x: f64) -> f64 {
        match self.family.standard() {
            Some((std, log)) => match self.standard_z(x, log) {
                Some(z) => std.ln_cdf(z),
                None => f64::NEG_INFINITY,
            },
            None => {
                let u = (x - self.threshold) / self.scale;
                if u <= 0.0 {
                    f64::NEG_INFINITY
                } else if self.is_exponential() {
                    let e = (-u).exp();
                    if e > 0.5 { (-(-u).exp_m1()).ln() } else { (-e).ln_1p() }
                } else {
                    regularized_gamma_p(self.shape, u).map_or(f64::NAN, f64::ln)
                }
            }
        }
    }

    /// `ln(1 − F(x))` (`0` at or below a threshold).
    #[must_use]
    pub fn ln_sf(&self, x: f64) -> f64 {
        match self.family.standard() {
            Some((std, log)) => match self.standard_z(x, log) {
                Some(z) => std.ln_sf(z),
                None => 0.0,
            },
            None => {
                let u = (x - self.threshold) / self.scale;
                if u <= 0.0 {
                    0.0
                } else if self.is_exponential() {
                    -u
                } else {
                    regularized_gamma_q(self.shape, u).map_or(f64::NAN, f64::ln)
                }
            }
        }
    }

    /// Distribution function `F(x)`.
    #[must_use]
    pub fn cdf(&self, x: f64) -> f64 {
        self.ln_cdf(x).exp()
    }

    /// Density `f(x)`.
    ///
    /// At a threshold `x = γ` this is the right-hand limit: for the Weibull
    /// and log-logistic (shape `β = 1/σ`) and the gamma and exponential (shape
    /// `k`), `+∞` for a shape below one, `1/α` (`1/θ`) for a shape of one and
    /// 0 above; 0 for the lognormal.
    #[must_use]
    pub fn pdf(&self, x: f64) -> f64 {
        let at_threshold = |shape: f64, value_at_one: f64| {
            if shape < 1.0 {
                f64::INFINITY
            } else if shape == 1.0 {
                value_at_one
            } else {
                0.0
            }
        };
        match self.family.standard() {
            Some((std, log)) => match self.standard_z(x, log) {
                Some(z) => {
                    let jac = if log { self.scale * (x - self.threshold) } else { self.scale };
                    std.log_density(z).0.exp() / jac
                }
                None if x == self.threshold && matches!(std, Standard::Sev | Standard::Logistic) => {
                    at_threshold(1.0 / self.scale, (-self.location).exp())
                }
                None => 0.0,
            },
            None => {
                let u = (x - self.threshold) / self.scale;
                if u < 0.0 || u.is_nan() {
                    0.0
                } else if u == 0.0 {
                    at_threshold(self.shape, 1.0 / self.scale)
                } else {
                    ((self.shape - 1.0) * u.ln() - u - ln_gamma(self.shape)).exp() / self.scale
                }
            }
        }
    }

    /// Log density `ln f(x)`, accurate far into the tails where [`Self::pdf`]
    /// underflows (`−∞` outside the support; at a threshold, the logarithm of
    /// the right-hand limit described at [`Self::pdf`]).
    #[must_use]
    pub fn ln_pdf(&self, x: f64) -> f64 {
        match self.family.standard() {
            Some((std, log)) => match self.standard_z(x, log) {
                Some(z) => {
                    let ln_jac = if log { self.scale.ln() + (x - self.threshold).ln() } else { self.scale.ln() };
                    std.log_density(z).0 - ln_jac
                }
                None => self.pdf(x).ln(),
            },
            None => {
                let u = (x - self.threshold) / self.scale;
                if u > 0.0 {
                    (self.shape - 1.0) * u.ln() - u - ln_gamma(self.shape) - self.scale.ln()
                } else {
                    self.pdf(x).ln()
                }
            }
        }
    }

    /// The lower end of the support: the threshold for the families on
    /// `(γ, ∞)` (log families, exponential and gamma, with `γ = 0` for the
    /// two-parameter forms), `−∞` for the families on the whole real line.
    #[must_use]
    pub fn lower_bound(&self) -> f64 {
        match self.family.standard() {
            Some((_, false)) => f64::NEG_INFINITY,
            _ => self.threshold,
        }
    }

    /// Quantile `F⁻¹(p)` for `0 < p < 1` (NaN otherwise).
    #[must_use]
    pub fn quantile(&self, p: f64) -> f64 {
        if !(p > 0.0 && p < 1.0) {
            return f64::NAN;
        }
        match self.family.standard() {
            Some((std, log)) => {
                let y = self.location + self.scale * std.quantile(p);
                if log { self.threshold + y.exp() } else { y }
            }
            None if self.is_exponential() => self.threshold - self.scale * (-p).ln_1p(),
            None => self.threshold + self.scale * gamma_quantile(self.shape, p),
        }
    }

    /// The quantile of the family's standard form at `p`: `F₀⁻¹(p)` for
    /// the location–scale families (on the log scale for the log families),
    /// and the quantile of `(X − γ)/θ` for the exponential and gamma. A
    /// probability plot puts each ordered value against the standard
    /// quantile of its plotting position; [`Self::standardize`] is the
    /// fitted relationship, a straight line on the family's natural scale.
    #[must_use]
    pub fn standard_quantile(&self, p: f64) -> f64 {
        if !(p > 0.0 && p < 1.0) {
            return f64::NAN;
        }
        match self.family.standard() {
            Some((std, _)) => std.quantile(p),
            None if self.is_exponential() => -(-p).ln_1p(),
            None => gamma_quantile(self.shape, p),
        }
    }

    /// The standard value of `x` under the fit: `(y − μ)/σ` for the
    /// location–scale families (`y = ln(x − γ)` for the log families) and
    /// `(x − γ)/θ` for the exponential and gamma, so that
    /// `standardize(quantile(p)) = standard_quantile(p)`. NaN at or below a
    /// log family's threshold.
    #[must_use]
    pub fn standardize(&self, x: f64) -> f64 {
        match self.family.standard() {
            Some((_, log)) => self.standard_z(x, log).unwrap_or(f64::NAN),
            None => (x - self.threshold) / self.scale,
        }
    }

    fn is_exponential(&self) -> bool {
        matches!(self.family, FitFamily::Exponential | FitFamily::Exponential2)
    }

    /// Standardized value on the location–scale scale; `None` at or below a
    /// log family's threshold.
    fn standard_z(&self, x: f64, log: bool) -> Option<f64> {
        if log {
            let u = x - self.threshold;
            (u > 0.0).then(|| (u.ln() - self.location) / self.scale)
        } else {
            Some((x - self.location) / self.scale)
        }
    }

    /// A random draw from the fitted distribution.
    fn sample(&self, rng: &mut Rng) -> f64 {
        if matches!(self.family, FitFamily::Gamma | FitFamily::Gamma3) {
            return self.threshold + self.scale * gamma_variate(self.shape, rng);
        }
        // Inversion; 1 − u lies in (0, 1].
        let mut p = 1.0 - rng.next_f64();
        if p >= 1.0 {
            p = 0.5;
        }
        self.quantile(p)
    }
}

/// Inverse of the regularized lower incomplete gamma function: the `u` with
/// `P(k, u) = p`. Newton steps on `P`, safeguarded by bisection, from the
/// Wilson–Hilferty approximation.
fn gamma_quantile(k: f64, p: f64) -> f64 {
    let z = Normal::standard().quantile(p).unwrap_or(0.0);
    let c = 1.0 / (9.0 * k);
    let mut u = (k * (1.0 - c + z * c.sqrt()).powi(3)).max(1e-300);
    if !u.is_finite() || u <= 0.0 {
        u = k;
    }
    let (mut lo, mut hi) = (0.0_f64, f64::INFINITY);
    for _ in 0..200 {
        let Ok(f) = regularized_gamma_p(k, u) else { return f64::NAN };
        let diff = f - p;
        if diff > 0.0 {
            hi = u;
        } else {
            lo = u;
        }
        let density = ((k - 1.0) * u.ln() - u - ln_gamma(k)).exp();
        let mut next = if density > 0.0 { u - diff / density } else { f64::NAN };
        if !(next > lo && next < hi) {
            next = if hi.is_finite() { 0.5 * (lo + hi) } else { 2.0 * u.max(1e-300) };
        }
        if (next - u).abs() <= 1e-14 * u {
            return next;
        }
        u = next;
    }
    u
}

/// A gamma(`k`, 1) variate (Marsaglia & Tsang 2000, *ACM TOMS* 26; for
/// `k < 1` the boost `G(k) = G(k + 1)·U^{1/k}`).
fn gamma_variate(k: f64, rng: &mut Rng) -> f64 {
    if k < 1.0 {
        // Boost in log space: `U^{1/k}` alone underflows for tiny k even when
        // the product is representable.
        let u = 1.0 - rng.next_f64();
        let g = gamma_variate(k + 1.0, rng);
        return (g.ln() + u.ln() / k).exp();
    }
    let d = k - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = rng.normal();
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 {
            continue;
        }
        let u = 1.0 - rng.next_f64();
        if u.ln() < 0.5 * x * x + d - d * v + d * v.ln() {
            return d * v;
        }
    }
}

// ----------------------------------------------------------------------------
// Two-parameter fits
// ----------------------------------------------------------------------------

/// Log-likelihood `Σ ln f₀(zᵢ) − n ln σ` with gradient and Hessian in `(μ, σ)`.
fn ls_evaluate(std: Standard, y: &[f64], mu: f64, sigma: f64) -> (f64, [f64; 2], [[f64; 2]; 2]) {
    let n = y.len() as f64;
    let (mut value, mut g_mu, mut g_s) = (0.0, 0.0, 0.0);
    let (mut h_mm, mut h_ms, mut h_ss) = (0.0, 0.0, 0.0);
    for &yi in y {
        let z = (yi - mu) / sigma;
        let (l, w, v) = std.log_density(z);
        value += l;
        g_mu -= w;
        g_s -= z * w;
        h_mm += v;
        h_ms += w + z * v;
        h_ss += 2.0 * z * w + z * z * v;
    }
    let s2 = sigma * sigma;
    (value - n * sigma.ln(), [g_mu / sigma, (g_s - n) / sigma], [[h_mm / s2, h_ms / s2], [h_ms / s2, (h_ss + n) / s2]])
}

/// Maximum-likelihood `(μ, σ, ℓ)` of a location–scale family for `y`, from
/// `start` (moment values when `None`).
fn fit_location_scale(std: Standard, y: &[f64], start: Option<(f64, f64)>) -> Result<(f64, f64, f64)> {
    let n = y.len() as f64;
    let mean = y.iter().sum::<f64>() / n;
    let var = y.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    if !(var > 0.0 && var.is_finite()) {
        return Err(invalid("a distribution fit needs at least two distinct values"));
    }
    if std == Standard::Normal {
        let sigma = var.sqrt();
        return Ok((mean, sigma, -n * (sigma.ln() + LN_SQRT_2PI + 0.5)));
    }
    let (mut mu, mut sigma) = start.unwrap_or_else(|| std.start(mean, var.sqrt()));
    let (mut value, mut g, mut h) = ls_evaluate(std, y, mu, sigma);
    let mut damping = 1e-3;
    for _ in 0..500 {
        // Gradient and Hessian in (μ, τ = ln σ).
        let gt = [g[0], sigma * g[1]];
        let ht = [[h[0][0], sigma * h[0][1]], [sigma * h[0][1], sigma * sigma * h[1][1] + sigma * g[1]]];
        if (gt[0] * gt[0] + gt[1] * gt[1]).sqrt() < 1e-10 * (1.0 + value.abs() / n) {
            return Ok((mu, sigma, value));
        }
        let mut accepted = false;
        for _ in 0..60 {
            let a = [
                [-ht[0][0] + damping * (1.0 + ht[0][0].abs()), -ht[0][1]],
                [-ht[1][0], -ht[1][1] + damping * (1.0 + ht[1][1].abs())],
            ];
            let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
            if !(a[0][0] > 0.0 && det > 0.0) {
                damping *= 10.0;
                continue;
            }
            let d_mu = (a[1][1] * gt[0] - a[0][1] * gt[1]) / det;
            let d_tau = (a[0][0] * gt[1] - a[1][0] * gt[0]) / det;
            let (new_mu, new_sigma) = (mu + d_mu, sigma * d_tau.clamp(-5.0, 5.0).exp());
            let trial = ls_evaluate(std, y, new_mu, new_sigma);
            if trial.0.is_finite() && trial.0 >= value - 1e-12 * value.abs() {
                let small = d_mu.abs() < 1e-12 * (sigma + mu.abs()) && d_tau.abs() < 1e-12;
                (mu, sigma) = (new_mu, new_sigma);
                (value, g, h) = trial;
                damping = (damping * 0.1).max(1e-12);
                accepted = true;
                if small {
                    return Ok((mu, sigma, value));
                }
                break;
            }
            damping *= 10.0;
        }
        if !accepted || !(sigma > 0.0 && sigma.is_finite() && mu.is_finite()) {
            break;
        }
    }
    Err(StatError::ConvergenceFailure { routine: "location-scale likelihood", iterations: 500 })
}

/// Maximum-likelihood gamma shape and scale for positive `u`, and `ℓ`.
fn fit_gamma(u: &[f64]) -> Result<(f64, f64, f64)> {
    let n = u.len() as f64;
    let mean = u.iter().sum::<f64>() / n;
    let mean_log = u.iter().map(|v| v.ln()).sum::<f64>() / n;
    let s = mean.ln() - mean_log;
    if !(s > 1e-14 && s.is_finite()) {
        return Err(invalid("a gamma fit needs at least two distinct positive values"));
    }
    // Minka (2002), "Estimating a gamma distribution", starting value.
    let mut k = (3.0 - s + ((s - 3.0).powi(2) + 24.0 * s).sqrt()) / (12.0 * s);
    for _ in 0..100 {
        let f = k.ln() - digamma(k) - s;
        let fp = 1.0 / k - trigamma(k);
        let mut next = k - f / fp;
        if !(next > 0.0 && next.is_finite()) {
            next = k / 2.0;
        }
        let done = (next - k).abs() <= 1e-13 * k;
        k = next;
        if done {
            break;
        }
    }
    let theta = mean / k;
    let ll = (k - 1.0) * n * mean_log - n * mean / theta - n * ln_gamma(k) - n * k * theta.ln();
    if !ll.is_finite() {
        return Err(StatError::ConvergenceFailure { routine: "gamma likelihood", iterations: 100 });
    }
    Ok((k, theta, ll))
}

/// The two-parameter fit of `family` (no threshold, or a fixed one) to `x − γ`.
fn fit_fixed_threshold(
    family: FitFamily,
    x: &[f64],
    gamma: f64,
    warm: Option<(f64, f64)>,
) -> Result<FittedDistribution> {
    let n = x.len();
    let mut fit = FittedDistribution {
        family,
        threshold: gamma,
        location: 0.0,
        scale: 1.0,
        shape: 1.0,
        log_likelihood: f64::NAN,
        n,
    };
    match family.standard() {
        Some((std, log)) => {
            if log {
                let y: Vec<f64> = x.iter().map(|v| (v - gamma).ln()).collect();
                let (mu, sigma, ll) = fit_location_scale(std, &y, warm)?;
                (fit.location, fit.scale) = (mu, sigma);
                fit.log_likelihood = ll - y.iter().sum::<f64>();
            } else {
                let (mu, sigma, ll) = fit_location_scale(std, x, warm)?;
                (fit.location, fit.scale, fit.log_likelihood) = (mu, sigma, ll);
            }
        }
        None if matches!(family, FitFamily::Gamma | FitFamily::Gamma3) => {
            let u: Vec<f64> = x.iter().map(|v| v - gamma).collect();
            let (k, theta, ll) = fit_gamma(&u)?;
            (fit.shape, fit.scale, fit.log_likelihood) = (k, theta, ll);
        }
        None => {
            let theta = x.iter().map(|v| v - gamma).sum::<f64>() / n as f64;
            if theta.is_nan() || theta <= 0.0 {
                return Err(invalid("an exponential fit needs a positive mean"));
            }
            fit.scale = theta;
            fit.log_likelihood = -(n as f64) * (theta.ln() + 1.0);
        }
    }
    Ok(fit)
}

// ----------------------------------------------------------------------------
// Threshold forms
// ----------------------------------------------------------------------------

/// Profile-likelihood fit of a three-parameter family (see the module docs).
fn fit_threshold(family: FitFamily, sorted: &[f64]) -> Result<FittedDistribution> {
    let x1 = sorted[0];
    let range = sorted[sorted.len() - 1] - x1;
    let profile = |t: f64, warm: Option<(f64, f64)>| -> Option<FittedDistribution> {
        let gamma = x1 - range * t.exp();
        if gamma.is_nan() || gamma >= x1 {
            return None;
        }
        fit_fixed_threshold(family, sorted, gamma, warm).ok().filter(|f| f.log_likelihood.is_finite())
    };
    let warm_of = |f: &FittedDistribution| Some((f.location, f.scale));
    // Grid over t = ln(d/r).
    let grid: Vec<f64> = (0..=32).map(|i| -11.0 + 0.5 * f64::from(i)).collect();
    let mut values: Vec<Option<FittedDistribution>> = Vec::with_capacity(grid.len());
    let mut warm = None;
    for &t in &grid {
        let f = profile(t, warm);
        if let Some(f) = &f {
            warm = warm_of(f);
        }
        values.push(f);
    }
    let ll = |i: usize| values[i].as_ref().map_or(f64::NEG_INFINITY, |f| f.log_likelihood);
    // Interior local maxima of the grid.
    let mut best: Option<usize> = None;
    for i in 1..grid.len() - 1 {
        if ll(i).is_finite() && ll(i) >= ll(i - 1) && ll(i) >= ll(i + 1) && best.is_none_or(|b| ll(i) > ll(b)) {
            best = Some(i);
        }
    }
    let Some(i) = best else {
        return Err(StatError::ConvergenceFailure { routine: "threshold profile likelihood", iterations: grid.len() });
    };
    // Golden-section refinement on [t_{i−1}, t_{i+1}].
    let (mut a, mut b) = (grid[i - 1], grid[i + 1]);
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let mut warm = values[i].as_ref().and_then(warm_of);
    let eval = |t: f64, warm: &mut Option<(f64, f64)>| -> f64 {
        match profile(t, *warm) {
            Some(f) => {
                *warm = warm_of(&f);
                f.log_likelihood
            }
            None => f64::NEG_INFINITY,
        }
    };
    let mut c = b - ratio * (b - a);
    let mut d = a + ratio * (b - a);
    let (mut fc, mut fd) = (eval(c, &mut warm), eval(d, &mut warm));
    for _ in 0..80 {
        if (b - a).abs() < 1e-9 {
            break;
        }
        if fc >= fd {
            b = d;
            d = c;
            fd = fc;
            c = b - ratio * (b - a);
            fc = eval(c, &mut warm);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + ratio * (b - a);
            fd = eval(d, &mut warm);
        }
    }
    let t = 0.5 * (a + b);
    let refined = profile(t, warm);
    let grid_best = values[i].clone();
    let fit = match (refined, grid_best) {
        (Some(r), Some(g)) => {
            if r.log_likelihood >= g.log_likelihood {
                r
            } else {
                g
            }
        }
        (Some(r), None) => r,
        (None, Some(g)) => g,
        (None, None) => {
            return Err(StatError::ConvergenceFailure { routine: "threshold profile likelihood", iterations: 80 });
        }
    };
    Ok(fit)
}

/// Fit `family` to `x` by maximum likelihood (see the module docs).
///
/// # Errors
/// [`StatError::NonFiniteInput`] for NaN/±∞; [`StatError::InsufficientData`]
/// for fewer than 3 values (4 for a threshold form); [`StatError::InvalidArgument`]
/// for constant data or, for a family that needs them, values that are not
/// all positive; [`StatError::ConvergenceFailure`] when the likelihood has no
/// interior maximum (a threshold form whose likelihood rises towards the
/// smallest value, or levels off as the threshold recedes).
pub fn fit_distribution(x: &[f64], family: FitFamily) -> Result<FittedDistribution> {
    let sorted = checked_sorted(x, if family.has_threshold() { 4 } else { 3 })?;
    fit_sorted(&sorted, family)
}

fn checked_sorted(x: &[f64], min_n: usize) -> Result<Vec<f64>> {
    if x.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "distribution fit input" });
    }
    if x.len() < min_n {
        return Err(StatError::InsufficientData { required: min_n, found: x.len() });
    }
    let mut s = x.to_vec();
    s.sort_by(f64::total_cmp);
    if s[0] == s[s.len() - 1] {
        return Err(invalid("all values are equal, so no distribution can be fitted"));
    }
    Ok(s)
}

fn fit_sorted(sorted: &[f64], family: FitFamily) -> Result<FittedDistribution> {
    if family.needs_positive() && sorted[0] <= 0.0 {
        return Err(invalid(format!("the {} distribution needs every value to be positive", family.label())));
    }
    match family {
        FitFamily::Exponential2 => {
            let n = sorted.len() as f64;
            let x1 = sorted[0];
            let mean = sorted.iter().sum::<f64>() / n;
            let theta_mle = mean - x1;
            let theta = n * theta_mle / (n - 1.0);
            Ok(FittedDistribution {
                family,
                threshold: x1 - theta / n,
                location: 0.0,
                scale: theta,
                shape: 1.0,
                log_likelihood: -n * (theta_mle.ln() + 1.0),
                n: sorted.len(),
            })
        }
        f if f.has_threshold() => fit_threshold(f, sorted),
        FitFamily::Normal => {
            let mut fit = fit_fixed_threshold(family, sorted, 0.0, None)?;
            let n = sorted.len() as f64;
            fit.scale *= (n / (n - 1.0)).sqrt();
            Ok(fit)
        }
        f => fit_fixed_threshold(f, sorted, 0.0, None),
    }
}

// ----------------------------------------------------------------------------
// Goodness of fit
// ----------------------------------------------------------------------------

/// The Anderson–Darling statistic `A²` of sorted data against `dist`
/// (`+∞` when a value lies outside the fitted support).
#[must_use]
pub fn anderson_darling_statistic(sorted: &[f64], dist: &FittedDistribution) -> f64 {
    let n = sorted.len();
    let nf = n as f64;
    let mut acc = 0.0;
    for i in 0..n {
        let term = dist.ln_cdf(sorted[i]) + dist.ln_sf(sorted[n - 1 - i]);
        acc += (2.0 * i as f64 + 1.0) * term;
    }
    let a2 = -nf - acc / nf;
    if a2.is_nan() { f64::INFINITY } else { a2 }
}

/// How an Anderson–Darling p-value was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PValueMethod {
    /// A published approximation formula.
    Formula,
    /// Monte Carlo simulation with this many replicate samples.
    Simulation {
        /// Replicates used.
        replicates: usize,
    },
}

/// An Anderson–Darling goodness-of-fit test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AndersonDarling {
    /// `A²` against the fitted distribution.
    pub statistic: f64,
    /// p-value (small values: the family does not fit).
    pub p_value: f64,
    /// How the p-value was obtained.
    pub method: PValueMethod,
}

/// A likelihood-ratio test of a threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThresholdTest {
    /// `2(ℓ₃ − ℓ₂)`.
    pub statistic: f64,
    /// Upper χ²₁ tail.
    pub p_value: f64,
}

/// Options for [`identify_distribution`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IdentifyOptions {
    /// Monte Carlo replicates for families whose null distribution does not
    /// depend on the parameters (cached per family and sample size).
    pub replicates: usize,
    /// Replicates for the gamma family (parametric bootstrap).
    pub gamma_replicates: usize,
    /// Replicates for the three-parameter forms (parametric bootstrap; each
    /// replicate repeats the profile-likelihood search).
    pub threshold_replicates: usize,
    /// Seed of the simulations.
    pub seed: u64,
}

impl Default for IdentifyOptions {
    fn default() -> Self {
        Self { replicates: 2000, gamma_replicates: 1000, threshold_replicates: 200, seed: 20_261_014 }
    }
}

/// One family's fit and goodness of fit.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyFit {
    /// The fitted distribution.
    pub fit: FittedDistribution,
    /// Anderson–Darling test.
    pub anderson_darling: AndersonDarling,
    /// For a three-parameter form, the test that its threshold improves on
    /// the two-parameter form.
    pub threshold_test: Option<ThresholdTest>,
}

/// Seed of replicate `i` of a simulation.
fn replicate_seed(seed: u64, key: u64, i: usize) -> u64 {
    let mut r =
        Rng::new(seed ^ key.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (i as u64).wrapping_mul(0xD1B5_4A32_D192_ED03));
    r.next_u64()
}

/// Simulated null values of `A²` (sorted) for a family whose null
/// distribution depends only on `n`.
fn pivotal_null(family: FitFamily, n: usize, replicates: usize, seed: u64) -> Arc<Vec<f64>> {
    type Cache = Mutex<HashMap<(FitFamily, usize, usize, u64), Arc<Vec<f64>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (family, n, replicates, seed);
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return v;
    }
    let standard =
        FittedDistribution { family, threshold: 0.0, location: 0.0, scale: 1.0, shape: 1.0, log_likelihood: 0.0, n };
    let values = simulate(&standard, n, replicates, seed);
    let values = Arc::new(values);
    if let Ok(mut c) = cache.lock() {
        c.insert(key, Arc::clone(&values));
    }
    values
}

/// `A²` of `replicates` samples of size `n` drawn from `dist` and refitted
/// with its family (sorted; replicates whose fit fails are left out).
fn simulate(dist: &FittedDistribution, n: usize, replicates: usize, seed: u64) -> Vec<f64> {
    let key = dist.family as u64 + 1;
    let mut values: Vec<f64> = (0..replicates)
        .into_par_iter()
        .filter_map(|i| {
            let mut rng = Rng::new(replicate_seed(seed, key, i));
            let mut sample: Vec<f64> = (0..n).map(|_| dist.sample(&mut rng)).collect();
            sample.sort_by(f64::total_cmp);
            if sample[0] == sample[n - 1] {
                return None;
            }
            let refit = fit_sorted(&sample, dist.family).ok()?;
            let a2 = anderson_darling_statistic(&sample, &refit);
            a2.is_finite().then_some(a2)
        })
        .collect();
    values.sort_by(f64::total_cmp);
    values
}

/// The modification factor applied to `A²` before comparing samples of
/// different sizes (1 when none is published for the family).
fn modifier(family: FitFamily, n: usize) -> f64 {
    match family.standard() {
        Some((std, _)) if !family.has_threshold() => std.ad_modifier(n),
        _ if family == FitFamily::Exponential || family == FitFamily::Exponential2 => 1.0 + 0.6 / n as f64,
        _ => 1.0,
    }
}

/// p-value of `a2` against sorted simulated values, after scaling both by
/// their modification factors.
fn simulated_p(a2: f64, n: usize, family: FitFamily, null: &[f64], m: usize) -> f64 {
    let observed = a2 * modifier(family, n);
    let scale = modifier(family, m);
    let at_least = null.len() - null.partition_point(|&v| v * scale < observed);
    (1.0 + at_least as f64) / (null.len() as f64 + 1.0)
}

/// The Anderson–Darling test of `fit` for `sorted` data.
fn goodness_of_fit(sorted: &[f64], fit: &FittedDistribution, opts: &IdentifyOptions) -> Result<AndersonDarling> {
    let n = sorted.len();
    if fit.family == FitFamily::Normal {
        let t: NormalityTest = anderson_darling(sorted)?;
        return Ok(AndersonDarling { statistic: t.statistic, p_value: t.p_value, method: PValueMethod::Formula });
    }
    let a2 = anderson_darling_statistic(sorted, fit);
    let m = n.min(MAX_SIMULATED_N);
    let pivotal = !fit.family.has_threshold() || fit.family == FitFamily::Exponential2;
    let pivotal = pivotal && !matches!(fit.family, FitFamily::Gamma);
    let null: Arc<Vec<f64>> = if pivotal {
        pivotal_null(fit.family, m, opts.replicates, opts.seed)
    } else {
        let reps = if fit.family == FitFamily::Gamma { opts.gamma_replicates } else { opts.threshold_replicates };
        Arc::new(simulate(fit, m, reps, opts.seed))
    };
    if null.is_empty() {
        return Err(StatError::ConvergenceFailure { routine: "Anderson–Darling simulation", iterations: 0 });
    }
    Ok(AndersonDarling {
        statistic: a2,
        p_value: simulated_p(a2, n, fit.family, &null, m),
        method: PValueMethod::Simulation { replicates: null.len() },
    })
}

/// Fit each of `families` to `x` with its Anderson–Darling test and, for
/// threshold forms, the likelihood-ratio test of the threshold. Families
/// that cannot be fitted (non-positive data, no interior maximum) give their
/// error in place of a fit.
///
/// # Errors
/// [`StatError::NonFiniteInput`], [`StatError::InsufficientData`] (fewer
/// than 4 values) or [`StatError::InvalidArgument`] (constant data) for the
/// sample as a whole.
pub fn identify_distribution(
    x: &[f64],
    families: &[FitFamily],
    opts: &IdentifyOptions,
) -> Result<Vec<(FitFamily, Result<FamilyFit>)>> {
    let sorted = checked_sorted(x, 4)?;
    let mut fits: HashMap<FitFamily, Result<FittedDistribution>> = HashMap::new();
    for &f in families {
        fits.insert(f, fit_sorted(&sorted, f));
        if let Some(base) = f.without_threshold() {
            fits.entry(base).or_insert_with(|| fit_sorted(&sorted, base));
        }
    }
    let out = families
        .iter()
        .map(|&f| {
            let result = match &fits[&f] {
                Ok(fit) => goodness_of_fit(&sorted, fit, opts).map(|ad| {
                    let threshold_test = f.without_threshold().and_then(|base| {
                        let base_ll = fits.get(&base)?.as_ref().ok()?.log_likelihood;
                        let statistic = (2.0 * (fit.log_likelihood - base_ll)).max(0.0);
                        let p_value = ChiSquared::new(1.0).ok()?.sf(statistic).ok()?;
                        Some(ThresholdTest { statistic, p_value })
                    });
                    FamilyFit { fit: fit.clone(), anderson_darling: ad, threshold_test }
                }),
                Err(e) => Err(e.clone()),
            };
            (f, result)
        })
        .collect();
    Ok(out)
}

// ----------------------------------------------------------------------------
// Johnson transformation
// ----------------------------------------------------------------------------

/// A member of the Johnson system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JohnsonFamily {
    /// Bounded: `Z = γ + η ln((x − ε)/(λ + ε − x))`.
    Sb,
    /// Lognormal: `Z = γ + η ln(x − ε)`.
    Sl,
    /// Unbounded: `Z = γ + η sinh⁻¹((x − ε)/λ)`.
    Su,
}

impl JohnsonFamily {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sb => "SB (bounded)",
            Self::Sl => "SL (lognormal)",
            Self::Su => "SU (unbounded)",
        }
    }
}

/// A fitted Johnson transformation to normality.
#[derive(Debug, Clone, PartialEq)]
pub struct JohnsonTransformation {
    /// The member of the system.
    pub family: JohnsonFamily,
    /// Shape `γ`.
    pub gamma: f64,
    /// Shape `η > 0`.
    pub eta: f64,
    /// Location `ε`.
    pub epsilon: f64,
    /// Scale `λ > 0` (unused for SL).
    pub lambda: f64,
    /// The percentile spacing `z` chosen.
    pub z: f64,
    /// Anderson–Darling normality test of the transformed values.
    pub normality: NormalityTest,
}

impl JohnsonTransformation {
    /// The transformed value `Z` of `x` (NaN outside the support).
    #[must_use]
    pub fn transform(&self, x: f64) -> f64 {
        let (g, e, eps, l) = (self.gamma, self.eta, self.epsilon, self.lambda);
        match self.family {
            JohnsonFamily::Su => g + e * ((x - eps) / l).asinh(),
            JohnsonFamily::Sl => {
                if x > eps {
                    g + e * (x - eps).ln()
                } else {
                    f64::NAN
                }
            }
            JohnsonFamily::Sb => {
                if x > eps && x < eps + l {
                    g + e * ((x - eps) / (l + eps - x)).ln()
                } else {
                    f64::NAN
                }
            }
        }
    }

    /// The value `x` whose transform is `z`.
    #[must_use]
    pub fn inverse(&self, z: f64) -> f64 {
        let w = (z - self.gamma) / self.eta;
        match self.family {
            JohnsonFamily::Su => self.epsilon + self.lambda * w.sinh(),
            JohnsonFamily::Sl => self.epsilon + w.exp(),
            JohnsonFamily::Sb => self.epsilon + self.lambda / (1.0 + (-w).exp()),
        }
    }
}

/// Sample percentile at probability `p` by linear interpolation at
/// position `np + ½` (1-based), clamped to the sample.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    let pos = (n as f64 * p + 0.5).clamp(1.0, n as f64) - 1.0;
    let i = pos.floor() as usize;
    let frac = pos - i as f64;
    if i + 1 < n { sorted[i] + frac * (sorted[i + 1] - sorted[i]) } else { sorted[n - 1] }
}

/// The Slifker–Shapiro percentile fit at spacing `z`, if one exists.
fn johnson_at(sorted: &[f64], z: f64) -> Option<(JohnsonFamily, f64, f64, f64, f64)> {
    let phi = |t: f64| 0.5 * erfc(-t * std::f64::consts::FRAC_1_SQRT_2);
    let x3 = percentile(sorted, phi(3.0 * z));
    let x1 = percentile(sorted, phi(z));
    let xm1 = percentile(sorted, phi(-z));
    let xm3 = percentile(sorted, phi(-3.0 * z));
    let m = x3 - x1;
    let n = xm1 - xm3;
    let p = x1 - xm1;
    if !(m > 0.0 && n > 0.0 && p > 0.0) {
        return None;
    }
    let qr = m * n / (p * p);
    let mid = 0.5 * (x1 + xm1);
    if (qr - 1.0).abs() < 1e-9 {
        // SL (m/p = p/n): only for a right-skewed (m > p) sample.
        let mp = m / p;
        if mp <= 1.0 {
            return None;
        }
        let eta = 2.0 * z / mp.ln();
        let gamma = eta * ((mp - 1.0) / (p * mp.sqrt())).ln();
        let eps = mid - 0.5 * p * (mp + 1.0) / (mp - 1.0);
        return Some((JohnsonFamily::Sl, gamma, eta, eps, 1.0));
    }
    if qr > 1.0 {
        let (mp, np) = (m / p, n / p);
        let eta = 2.0 * z / (0.5 * (mp + np)).acosh();
        let gamma = eta * ((np - mp) / (2.0 * (mp * np - 1.0).sqrt())).asinh();
        let lambda = 2.0 * p * (mp * np - 1.0).sqrt() / ((mp + np - 2.0) * (mp + np + 2.0).sqrt());
        let eps = mid + p * (np - mp) / (2.0 * (mp + np - 2.0));
        Some((JohnsonFamily::Su, gamma, eta, eps, lambda))
    } else {
        let (pm, pn) = (p / m, p / n);
        let prod = (1.0 + pm) * (1.0 + pn);
        let eta = z / (0.5 * prod.sqrt()).acosh();
        let gamma = eta * ((pn - pm) * (prod - 4.0).sqrt() / (2.0 * (pm * pn - 1.0))).asinh();
        let lambda = p * ((prod - 2.0).powi(2) - 4.0).sqrt() / (pm * pn - 1.0);
        let eps = mid - 0.5 * lambda + p * (pn - pm) / (2.0 * (pm * pn - 1.0));
        Some((JohnsonFamily::Sb, gamma, eta, eps, lambda))
    }
}

/// Choose and fit a Johnson transformation to normality: for each percentile
/// spacing `z = 0.25, 0.26, …, 1.25` the Slifker–Shapiro percentile
/// estimates pick the SB, SL or SU member from the quantile ratio
/// `mn/p²` and fit it; among the fits that transform every value, the one
/// whose transformed values have the largest Anderson–Darling normality
/// p-value (ties: smallest `A²`) is returned.
///
/// # Errors
/// [`StatError::NonFiniteInput`], [`StatError::InsufficientData`] (fewer
/// than 4 values), [`StatError::InvalidArgument`] for constant data or when
/// no spacing gives a valid transformation.
pub fn johnson_transformation(x: &[f64]) -> Result<JohnsonTransformation> {
    let sorted = checked_sorted(x, 4)?;
    let mut best: Option<JohnsonTransformation> = None;
    for i in 0..=100 {
        let z = 0.25 + 0.01 * f64::from(i);
        let Some((family, gamma, eta, epsilon, lambda)) = johnson_at(&sorted, z) else { continue };
        if !(eta > 0.0 && lambda > 0.0) || ![gamma, eta, epsilon, lambda].iter().all(|v| v.is_finite()) {
            continue;
        }
        let mut candidate = JohnsonTransformation {
            family,
            gamma,
            eta,
            epsilon,
            lambda,
            z,
            normality: NormalityTest { statistic: 0.0, p_value: 0.0 },
        };
        let transformed: Vec<f64> = sorted.iter().map(|&v| candidate.transform(v)).collect();
        if transformed.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let Ok(test) = anderson_darling(&transformed) else { continue };
        candidate.normality = test;
        let better = best.as_ref().is_none_or(|b| {
            test.p_value > b.normality.p_value
                || (test.p_value == b.normality.p_value && test.statistic < b.normality.statistic)
        });
        if better {
            best = Some(candidate);
        }
    }
    best.ok_or_else(|| invalid("no Johnson transformation transforms every value"))
}

#[cfg(test)]
mod tests;
