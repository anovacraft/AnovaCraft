//! Maximum-likelihood fits of count distributions to censored counts:
//! counts reported only as "below the detection limit" (left-censored),
//! "too numerous to count" (right-censored) or within a range
//! (interval-censored), alongside exact counts (Helsel, *Statistics for
//! Censored Environmental Data Using Minitab and R*, 2nd ed., 2012, ch. 6;
//! Cameron & Trivedi, *Regression Analysis of Count Data*, 2nd ed., 2013,
//! §3.3 and ch. 4 on censored counts).
//!
//! # Data
//! Each [`CountObservation`] states that the count `X` satisfies
//! `lower ≤ X ≤ upper` (integers, `upper` may be `+∞`), with a frequency
//! weight. An exact count has `lower = upper`; a count below a detection
//! limit `d` is `0 ≤ X ≤ d − 1`; a count of at least `c` is
//! `c ≤ X ≤ +∞`.
//!
//! # Families
//! * **Poisson** with mean `λ`: `P(X = x) = λˣ e^{−λ} / x!`.
//! * **Negative binomial** with mean `μ` and size (dispersion) `k`:
//!   `P(X = x) = Γ(x + k) / (Γ(k) x!) · pᵏ (1 − p)ˣ`, `p = k / (k + μ)`, so
//!   `Var X = μ + μ²/k`; it tends to the Poisson as `k → ∞`. This is the
//!   "NB2" parametrization (Cameron & Trivedi §3.3).
//!
//! # Likelihood
//! `ℓ = Σ w ln P(lower ≤ X ≤ upper)`. Exact counts use the probability
//! function; ranges of up to 1 000 values sum it (in log space), so
//! left-censored counts are exact; other tails use the regularized incomplete
//! gamma function (`P(X ≥ a) = P(a, λ)` for the Poisson) and the regularized
//! incomplete beta function (`P(X ≤ b) = I_p(k, b + 1)`,
//! `P(X ≥ a) = I_{1−p}(a, k)` for the negative binomial), taking the
//! difference in whichever tail is smaller.
//!
//! # Estimation and intervals
//! The log-likelihood is maximized over the log parameters (`ln λ`, or
//! `ln μ` and `ln k`) by damped Newton steps. Standard errors come from the
//! observed information on the log scale; Wald limits are
//! `exp(ln θ̂ ± z · se)` and the reported standard error of `θ̂` is
//! `θ̂ · se(ln θ̂)` (delta method). Likelihood-ratio limits are the set
//! `{θ : 2[ℓ̂ − ℓ_p(θ)] ≤ χ²₁(level)}`, with the other parameter
//! re-maximized (Meeker & Escobar 1998, §8.3); a limit more than a factor
//! `e²⁰` from the estimate is reported as unbounded (`0` or `+∞`), which
//! happens to the size `k` of nearly Poisson counts.

use stat_core::special::{
    ln_gamma, ln_gamma_difference, regularized_gamma_p, regularized_gamma_q, regularized_incomplete_beta_pair,
};
use stat_core::{ChiSquared, ContinuousDistribution, Normal, Result, StatError};

use crate::censored_fit::{IntervalEstimate, derivatives, inverse_spd, maximize};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Ranges with at most this many values are summed term by term.
const SUM_LIMIT: f64 = 1000.0;

/// A count distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CountFamily {
    /// Poisson with mean `λ`.
    Poisson,
    /// Negative binomial with mean `μ` and size `k` (`Var = μ + μ²/k`).
    NegativeBinomial,
}

impl CountFamily {
    /// Every family, in menu order.
    pub const ALL: [Self; 2] = [Self::Poisson, Self::NegativeBinomial];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Poisson => "Poisson",
            Self::NegativeBinomial => "Negative binomial",
        }
    }

    /// Stable key for project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Poisson => "poisson",
            Self::NegativeBinomial => "negative-binomial",
        }
    }

    /// The family with this key (also `"negbin"` and `"nb"`).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "negbin" | "nb" => Some(Self::NegativeBinomial),
            _ => Self::ALL.into_iter().find(|f| f.key() == key),
        }
    }

    /// Number of parameters.
    #[must_use]
    pub fn n_parameters(self) -> usize {
        match self {
            Self::Poisson => 1,
            Self::NegativeBinomial => 2,
        }
    }
}

/// A possibly censored count: `lower ≤ X ≤ upper`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CountObservation {
    /// Smallest possible count (a non-negative integer).
    pub lower: f64,
    /// Largest possible count (an integer `≥ lower`, or `+∞`).
    pub upper: f64,
    /// Frequency weight (`> 0`).
    pub weight: f64,
}

impl CountObservation {
    /// An exact count.
    #[must_use]
    pub fn exact(x: u64, weight: f64) -> Self {
        Self { lower: x as f64, upper: x as f64, weight }
    }

    /// A count below the detection limit `limit` (`0 ≤ X ≤ limit − 1`;
    /// `limit ≥ 1`).
    #[must_use]
    pub fn below(limit: u64, weight: f64) -> Self {
        Self { lower: 0.0, upper: limit as f64 - 1.0, weight }
    }

    /// A count of at least `x` (`x ≤ X`).
    #[must_use]
    pub fn at_least(x: u64, weight: f64) -> Self {
        Self { lower: x as f64, upper: f64::INFINITY, weight }
    }

    /// A count between `lower` and `upper` inclusive.
    #[must_use]
    pub fn between(lower: u64, upper: u64, weight: f64) -> Self {
        Self { lower: lower as f64, upper: upper as f64, weight }
    }

    fn valid(&self) -> bool {
        let int = |v: f64| v.is_finite() && v.fract() == 0.0;
        int(self.lower)
            && self.lower >= 0.0
            && (self.upper == f64::INFINITY || int(self.upper))
            && self.upper >= self.lower
            && self.weight.is_finite()
            && self.weight > 0.0
    }
}

/// A count distribution with given parameters.
#[derive(Debug, Clone, Copy)]
struct Count {
    family: CountFamily,
    mu: f64,
    k: f64,
}

fn ln_add(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY {
        return b;
    }
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    hi + (lo - hi).exp().ln_1p()
}

impl Count {
    fn ln_pmf(&self, x: f64) -> f64 {
        match self.family {
            CountFamily::Poisson => {
                if x == 0.0 {
                    -self.mu
                } else {
                    x * self.mu.ln() - self.mu - ln_gamma(x + 1.0)
                }
            }
            CountFamily::NegativeBinomial => {
                let ln_p = -(self.mu / self.k).ln_1p();
                let tail = if x == 0.0 { 0.0 } else { x * (self.mu.ln() - (self.mu + self.k).ln()) };
                ln_gamma_difference(self.k, x) - ln_gamma(x + 1.0) + self.k * ln_p + tail
            }
        }
    }

    /// `P(X ≤ b)`.
    fn cdf(&self, b: f64) -> f64 {
        if b < 0.0 {
            return 0.0;
        }
        match self.family {
            CountFamily::Poisson => regularized_gamma_q(b + 1.0, self.mu).unwrap_or(f64::NAN),
            CountFamily::NegativeBinomial => {
                let (p, q) = (self.k / (self.k + self.mu), self.mu / (self.k + self.mu));
                regularized_incomplete_beta_pair(self.k, b + 1.0, p, q).unwrap_or(f64::NAN)
            }
        }
    }

    /// `P(X ≥ a)`.
    fn sf_inclusive(&self, a: f64) -> f64 {
        if a <= 0.0 {
            return 1.0;
        }
        match self.family {
            CountFamily::Poisson => regularized_gamma_p(a, self.mu).unwrap_or(f64::NAN),
            CountFamily::NegativeBinomial => {
                let (p, q) = (self.k / (self.k + self.mu), self.mu / (self.k + self.mu));
                regularized_incomplete_beta_pair(a, self.k, q, p).unwrap_or(f64::NAN)
            }
        }
    }

    fn mean(&self) -> f64 {
        self.mu
    }

    /// `ln P(a ≤ X ≤ b)`.
    fn ln_prob(&self, a: f64, b: f64) -> f64 {
        if a == b {
            return self.ln_pmf(a);
        }
        if b == f64::INFINITY {
            return self.sf_inclusive(a).ln();
        }
        if b - a <= SUM_LIMIT {
            let mut total = f64::NEG_INFINITY;
            let mut x = a;
            while x <= b {
                total = ln_add(total, self.ln_pmf(x));
                x += 1.0;
            }
            return total;
        }
        let v = if a > self.mean() {
            self.sf_inclusive(a) - self.sf_inclusive(b + 1.0)
        } else {
            self.cdf(b) - self.cdf(a - 1.0)
        };
        v.ln()
    }
}

/// A count distribution fitted to censored counts.
#[derive(Debug, Clone, PartialEq)]
pub struct CountFit {
    /// The family.
    pub family: CountFamily,
    /// Estimates with standard errors, Wald and likelihood-ratio limits:
    /// the mean (`λ` or `μ`), then for the negative binomial the size `k`.
    pub parameters: Vec<IntervalEstimate>,
    /// Covariance of the log parameters (`ln λ`, or `ln μ, ln k`) from the
    /// observed information (`None` when it is not positive definite).
    pub covariance: Option<Vec<Vec<f64>>>,
    /// Maximized log-likelihood.
    pub log_likelihood: f64,
    /// Akaike's criterion `−2ℓ + 2m` (`m` parameters).
    pub aic: f64,
    /// Schwarz's criterion `−2ℓ + m ln n`.
    pub bic: f64,
    /// Number of observations (total frequency weight).
    pub n: f64,
    /// Confidence level of the intervals.
    pub level: f64,
}

impl CountFit {
    fn dist(&self) -> Count {
        Count {
            family: self.family,
            mu: self.parameters[0].estimate,
            k: self.parameters.get(1).map_or(f64::INFINITY, |p| p.estimate),
        }
    }

    /// Fitted mean.
    #[must_use]
    pub fn mean(&self) -> f64 {
        self.parameters[0].estimate
    }

    /// Fitted size `k` (negative binomial only).
    #[must_use]
    pub fn size(&self) -> Option<f64> {
        self.parameters.get(1).map(|p| p.estimate)
    }

    /// Fitted variance: `λ`, or `μ + μ²/k`.
    #[must_use]
    pub fn variance(&self) -> f64 {
        let m = self.mean();
        self.size().map_or(m, |k| m + m * m / k)
    }

    /// Fitted probability `P(X = x)`.
    #[must_use]
    pub fn pmf(&self, x: u64) -> f64 {
        self.dist().ln_pmf(x as f64).exp()
    }

    /// Fitted distribution function `P(X ≤ x)`.
    #[must_use]
    pub fn cdf(&self, x: u64) -> f64 {
        let d = self.dist();
        let x = x as f64;
        if x <= SUM_LIMIT && x <= d.mean() { d.ln_prob(0.0, x).exp() } else { d.cdf(x) }
    }
}

fn dist_of(family: CountFamily, th: &[f64]) -> Count {
    Count { family, mu: th[0].exp(), k: th.get(1).map_or(f64::INFINITY, |v| v.exp()) }
}

fn log_likelihood(obs: &[CountObservation], family: CountFamily, th: &[f64]) -> f64 {
    let d = dist_of(family, th);
    if !(d.mu > 0.0 && d.mu.is_finite() && d.k > 0.0) {
        return f64::NEG_INFINITY;
    }
    let mut total = 0.0;
    for o in obs {
        let c = d.ln_prob(o.lower, o.upper);
        if !c.is_finite() {
            return f64::NEG_INFINITY;
        }
        total += o.weight * c;
    }
    if total.is_nan() { f64::NEG_INFINITY } else { total }
}

/// Fit a Poisson or negative binomial distribution to censored counts by
/// maximum likelihood (see the module docs), with intervals at confidence
/// `level`.
///
/// # Errors
/// [`StatError::InvalidArgument`] when there are no observations, a bound is
/// negative or not an integer, `upper < lower`, a weight is not positive,
/// `level` is not in `(0, 1)`, no observation carries information (every
/// count is `0 ≤ X ≤ ∞`), the mean estimate is zero (every count can be 0),
/// the negative binomial size runs off to infinity (the counts are not
/// overdispersed: fit the Poisson), or the likelihood has no maximum.
pub fn fit_censored_counts(obs: &[CountObservation], family: CountFamily, level: f64) -> Result<CountFit> {
    if obs.is_empty() {
        return Err(invalid("no counts"));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if obs.iter().any(|o| !o.valid()) {
        return Err(invalid("counts must be non-negative integers with lower ≤ upper and positive frequencies"));
    }
    if obs.iter().all(|o| o.lower == 0.0 && o.upper == f64::INFINITY) {
        return Err(invalid("every count is unrestricted, so nothing can be estimated"));
    }
    let n: f64 = obs.iter().map(|o| o.weight).sum();
    // Starting values from representative counts.
    let rep = |o: &CountObservation| {
        if o.upper == f64::INFINITY {
            o.lower
        } else if o.lower == 0.0 && o.upper > 0.0 {
            0.5 * o.upper
        } else {
            0.5 * (o.lower + o.upper)
        }
    };
    let mean = obs.iter().map(|o| o.weight * rep(o)).sum::<f64>() / n;
    let var = obs.iter().map(|o| o.weight * (rep(o) - mean).powi(2)).sum::<f64>() / n;
    let mean = mean.max(0.1);
    let ll = |th: &[f64]| log_likelihood(obs, family, th);
    let steps = |th: &[f64]| vec![1e-4; th.len()];
    let starts: Vec<Vec<f64>> = match family {
        CountFamily::Poisson => vec![vec![mean.ln()], vec![mean.ln() + 1.0], vec![mean.ln() - 1.0]],
        CountFamily::NegativeBinomial => {
            let k0 = if var > mean { (mean * mean / (var - mean)).clamp(0.05, 100.0) } else { 10.0 };
            vec![vec![mean.ln(), k0.ln()], vec![mean.ln(), 0.0], vec![mean.ln() + 0.5, (0.3f64).ln()]]
        }
    };
    let mut best: Option<(Vec<f64>, f64)> = None;
    for x0 in &starts {
        if let Some(m) = maximize(&ll, x0, &steps) {
            if best.as_ref().is_none_or(|b| m.1 > b.1) {
                best = Some(m);
            }
        }
    }
    let (theta, value) = best.ok_or_else(|| invalid("the likelihood could not be maximized"))?;
    if theta[0] < -25.0 {
        return Err(invalid("the estimated mean is zero (every count may be zero)"));
    }
    if family == CountFamily::NegativeBinomial && theta[1] > 15.0 {
        return Err(invalid(
            "the negative binomial size runs off to infinity (no overdispersion); fit the Poisson instead",
        ));
    }
    let (_, _, hess) = derivatives(&ll, &theta, &vec![3e-4; theta.len()]);
    let neg: Vec<Vec<f64>> = hess.iter().map(|r| r.iter().map(|v| -v).collect()).collect();
    let covariance = inverse_spd(&neg);
    let z = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
    let crit = ChiSquared::new(1.0).and_then(|c| c.quantile(level)).unwrap_or(f64::NAN);
    let names: [&'static str; 2] = match family {
        CountFamily::Poisson => ["Mean λ", ""],
        CountFamily::NegativeBinomial => ["Mean μ", "Size k"],
    };
    let parameters = (0..theta.len())
        .map(|i| {
            let se = covariance.as_ref().map_or(f64::NAN, |c| c[i][i].max(0.0).sqrt());
            let estimate = theta[i].exp();
            let lr = profile_limits(&ll, &theta, value, i, se, crit);
            IntervalEstimate {
                name: names[i],
                estimate,
                std_error: estimate * se,
                wald: ((theta[i] - z * se).exp(), (theta[i] + z * se).exp()),
                likelihood_ratio: Some((lr.0.exp(), lr.1.exp())),
            }
        })
        .collect();
    let m = theta.len() as f64;
    Ok(CountFit {
        family,
        parameters,
        covariance,
        log_likelihood: value,
        aic: -2.0 * value + 2.0 * m,
        bic: -2.0 * value + m * n.ln(),
        n,
        level,
    })
}

/// Limits further than this from the estimate on the log scale (a factor
/// `e²⁰ ≈ 4.9 × 10⁸`) are reported as unbounded.
const MAX_EXCURSION: f64 = 20.0;

/// Likelihood-ratio limits for log parameter `i` (the other re-maximized);
/// `±∞` when the profile stays within the critical value up to
/// [`MAX_EXCURSION`] (as for the size `k` when the counts are close to
/// Poisson, whose likelihood is the limit of the profile as `k → ∞`).
fn profile_limits(ll: &dyn Fn(&[f64]) -> f64, theta: &[f64], l_hat: f64, i: usize, se: f64, crit: f64) -> (f64, f64) {
    let d = theta.len();
    let profile = |s: f64, warm: &mut Vec<f64>| -> f64 {
        if d == 1 {
            return ll(&[s]);
        }
        let assemble = |x: &[f64]| {
            let mut th = x.to_vec();
            th.insert(i, s);
            th
        };
        let f = |x: &[f64]| ll(&assemble(x));
        let steps = |x: &[f64]| vec![1e-4; x.len()];
        match maximize(&f, warm, &steps) {
            Some((x, v)) => {
                *warm = x;
                v
            }
            None => f64::NEG_INFINITY,
        }
    };
    let base = if se.is_finite() && se > 0.0 { se } else { 0.1 };
    let side = |dir: f64| -> f64 {
        let mut warm: Vec<f64> = theta.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, v)| *v).collect();
        let g = |s: f64, warm: &mut Vec<f64>| -> f64 {
            let v = profile(s, warm);
            if v.is_finite() { 2.0 * (l_hat - v) - crit } else { f64::INFINITY }
        };
        let mut inside = theta[i];
        let mut outside = None;
        let mut delta = base.min(0.5 * MAX_EXCURSION);
        while delta <= MAX_EXCURSION {
            let s = theta[i] + dir * delta;
            if g(s, &mut warm) > 0.0 {
                outside = Some(s);
                break;
            }
            inside = s;
            delta = if delta < MAX_EXCURSION { (delta * 1.6).min(MAX_EXCURSION) } else { f64::INFINITY };
        }
        let Some(mut out) = outside else { return dir * f64::INFINITY };
        let mut warm_in = warm.clone();
        for _ in 0..200 {
            if (out - inside).abs() < 1e-12 * (1.0 + inside.abs()) {
                break;
            }
            let mid = 0.5 * (inside + out);
            let mut w = warm_in.clone();
            if g(mid, &mut w) > 0.0 {
                out = mid;
            } else {
                inside = mid;
                warm_in = w;
            }
        }
        0.5 * (inside + out)
    };
    (side(-1.0), side(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for f in CountFamily::ALL {
            assert_eq!(CountFamily::from_key(f.key()), Some(f));
        }
        assert_eq!(CountFamily::from_key("nb"), Some(CountFamily::NegativeBinomial));
    }

    /// With exact counts only, the Poisson estimate is the sample mean and
    /// its standard error `√(λ̂/n)`.
    #[test]
    fn exact_poisson_is_the_mean() {
        let counts = [3u64, 5, 2, 7, 4, 4, 6, 1];
        let obs: Vec<CountObservation> = counts.iter().map(|&c| CountObservation::exact(c, 1.0)).collect();
        let fit = fit_censored_counts(&obs, CountFamily::Poisson, 0.95).unwrap();
        let mean = counts.iter().sum::<u64>() as f64 / counts.len() as f64;
        assert!((fit.mean() - mean).abs() < 1e-8);
        assert!((fit.parameters[0].std_error - (mean / 8.0).sqrt()).abs() < 1e-5);
        let total: f64 = (0..60).map(|k| fit.pmf(k)).sum();
        assert!((total - 1.0).abs() < 1e-12);
        assert!((fit.cdf(4) - (0..=4).map(|k| fit.pmf(k)).sum::<f64>()).abs() < 1e-12);
    }

    #[test]
    fn tails_agree_with_sums() {
        for family in CountFamily::ALL {
            let d = Count { family, mu: 7.3, k: 2.1 };
            let sum: f64 = (0..=12).map(|x| d.ln_pmf(f64::from(x)).exp()).sum();
            assert!((d.cdf(12.0) - sum).abs() < 1e-13);
            assert!((d.sf_inclusive(13.0) - (1.0 - sum)).abs() < 1e-13);
        }
    }

    #[test]
    fn invalid_counts_are_rejected() {
        let bad = [CountObservation { lower: 1.5, upper: 2.0, weight: 1.0 }];
        assert!(fit_censored_counts(&bad, CountFamily::Poisson, 0.95).is_err());
        let none = [CountObservation::at_least(0, 1.0)];
        assert!(fit_censored_counts(&none, CountFamily::Poisson, 0.95).is_err());
    }
}
