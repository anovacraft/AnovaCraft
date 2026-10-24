//! Alert and action limits for environmental monitoring: colony counts,
//! positive samples out of samples taken, or non-negative continuous
//! readings, with the limits set from the best-fitting distribution of
//! historical data (USP ⟨1116⟩, *Microbiological Control and Monitoring of
//! Aseptic Processing Environments*; PDA Technical Report 13, *Fundamentals
//! of an Environmental Monitoring Program*, 2022).
//!
//! **Count families** (maximum likelihood; Hilbe 2011, *Negative Binomial
//! Regression*, 2nd ed., ch. 8 and 11; Lambert 1992, "Zero-inflated Poisson
//! regression", *Technometrics* 34):
//!
//! * Poisson, `P(x) = e^{−λ}λˣ/x!`, with `λ̂ = x̄`;
//! * negative binomial with mean `μ` and size `k`,
//!   `P(x) = Γ(x + k)/(Γ(k) x!) · (k/(k + μ))ᵏ (μ/(k + μ))ˣ`, variance
//!   `μ + μ²/k`; `μ̂ = x̄` and `k` maximizes the profile likelihood;
//! * zero-inflated Poisson: a zero with probability `π`, else a Poisson
//!   count. The estimates solve `x̄ = (1 − π)λ` and
//!   `n₀/n = π + (1 − π)e^{−λ}`, so `λ̂` is the root of
//!   `λ(1 − n₀/n) = x̄(1 − e^{−λ})`;
//! * zero-inflated negative binomial: a zero with probability `π`, else a
//!   negative binomial count (Nelder–Mead on `logit π, ln μ, ln k`).
//!
//! **Positive samples** `x` out of `n` taken (per row): the binomial, with
//! `p̂ = Σx/Σn`, and the beta-binomial with mean `μ` and intra-class
//! correlation `ρ = 1/(a + b + 1)`,
//! `P(x) = C(n, x) B(x + a, n − x + b)/B(a, b)` (Nelder–Mead on
//! `logit μ, ln(a + b)`).
//!
//! A family whose extra parameter goes to its boundary (no overdispersion
//! for the negative binomial and beta-binomial, no excess zeros for the
//! zero-inflated forms) is reported as not fitted: it reduces to the simpler
//! family. **Continuous readings** use the fourteen families of
//! [`stat_core::distribution_fit`]. The best family has the smallest AIC.
//!
//! **Limits.** The alert and action levels are quantiles of the fitted
//! distribution (0.975 and 0.9985 by default). For a count the level is the
//! smallest `L` with `F(L) ≥ q`, so that a count above `L` has probability
//! at most `1 − q`; an **excursion** is a reading strictly above a level.
//! Alternatively the levels are the mean plus 2 and 3 standard deviations
//! (for positives, `p̄ + k√(p̄(1 − p̄)/n)` on the proportion scale).
//!
//! **Early-warning rules** ([`warning_rules`]) on each location's readings
//! in time order, numbered as reported: 1, an action excursion; 2, an alert
//! excursion; 3, `n` or more alert (or action) excursions within `m`
//! consecutive readings; 4, a run of `k` consecutive increases.
//!
//! # Failure modes
//! Negative or fractional counts, positives above the samples taken,
//! fewer than two readings, constant readings (continuous), and the
//! boundary cases above are errors; a quantile outside `(0, 1)` is an error.

use stat_core::distribution_fit::{FitFamily, FittedDistribution, fit_distribution};
use stat_core::optimize::minimize_in_box;
use stat_core::special::{ln_beta, ln_gamma};
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// A distribution family for counts or positive samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CountFamily {
    /// Poisson counts.
    Poisson,
    /// Negative binomial (overdispersed) counts.
    NegativeBinomial,
    /// Zero-inflated Poisson counts.
    ZeroInflatedPoisson,
    /// Zero-inflated negative binomial counts.
    ZeroInflatedNegativeBinomial,
    /// Binomial positives out of samples taken.
    Binomial,
    /// Beta-binomial (overdispersed) positives out of samples taken.
    BetaBinomial,
}

impl CountFamily {
    /// The families for counts.
    pub const COUNTS: [Self; 4] =
        [Self::Poisson, Self::NegativeBinomial, Self::ZeroInflatedPoisson, Self::ZeroInflatedNegativeBinomial];
    /// The families for positive samples out of samples taken.
    pub const POSITIVES: [Self; 2] = [Self::Binomial, Self::BetaBinomial];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Poisson => "Poisson",
            Self::NegativeBinomial => "Negative binomial",
            Self::ZeroInflatedPoisson => "Zero-inflated Poisson",
            Self::ZeroInflatedNegativeBinomial => "Zero-inflated negative binomial",
            Self::Binomial => "Binomial",
            Self::BetaBinomial => "Beta-binomial",
        }
    }

    /// Stable key for saved files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Poisson => "poisson",
            Self::NegativeBinomial => "negative-binomial",
            Self::ZeroInflatedPoisson => "zero-inflated-poisson",
            Self::ZeroInflatedNegativeBinomial => "zero-inflated-negative-binomial",
            Self::Binomial => "binomial",
            Self::BetaBinomial => "beta-binomial",
        }
    }

    /// Number of estimated parameters.
    #[must_use]
    pub fn n_parameters(self) -> usize {
        match self {
            Self::Poisson | Self::Binomial => 1,
            Self::NegativeBinomial | Self::ZeroInflatedPoisson | Self::BetaBinomial => 2,
            Self::ZeroInflatedNegativeBinomial => 3,
        }
    }
}

/// A fitted count or positive-sample distribution.
#[derive(Debug, Clone, PartialEq)]
pub struct CountFit {
    /// The family.
    pub family: CountFamily,
    /// Mean `λ`, `μ` (counts) or probability `p`, `μ` (positives).
    pub mean: f64,
    /// Negative binomial size `k` or beta-binomial `a + b` (`∞` otherwise).
    pub size: f64,
    /// Zero-inflation probability `π` (0 otherwise).
    pub zero_inflation: f64,
    /// Maximized log-likelihood.
    pub log_likelihood: f64,
    /// Number of readings.
    pub n: usize,
}

/// `ln P(x)` of the negative binomial with mean `mu` and size `k`.
fn nb_ln_pmf(x: f64, mu: f64, k: f64) -> f64 {
    ln_gamma(x + k) - ln_gamma(k) - ln_gamma(x + 1.0)
        + k * (k / (k + mu)).ln()
        + if x > 0.0 { x * (mu / (k + mu)).ln() } else { 0.0 }
}

fn poisson_ln_pmf(x: f64, lambda: f64) -> f64 {
    if lambda == 0.0 {
        return if x == 0.0 { 0.0 } else { f64::NEG_INFINITY };
    }
    x * lambda.ln() - lambda - ln_gamma(x + 1.0)
}

/// `ln P(x)` of the beta-binomial with `n` trials and parameters `a`, `b`.
fn beta_binomial_ln_pmf(x: f64, n: f64, a: f64, b: f64) -> f64 {
    ln_gamma(n + 1.0) - ln_gamma(x + 1.0) - ln_gamma(n - x + 1.0) + ln_beta(x + a, n - x + b) - ln_beta(a, b)
}

fn binomial_ln_pmf(x: f64, n: f64, p: f64) -> f64 {
    let choose = ln_gamma(n + 1.0) - ln_gamma(x + 1.0) - ln_gamma(n - x + 1.0);
    let a = if x > 0.0 { x * p.ln() } else { 0.0 };
    let b = if n - x > 0.0 { (n - x) * (-p).ln_1p() } else { 0.0 };
    choose + a + b
}

impl CountFit {
    /// Akaike's information criterion `−2ℓ + 2k`.
    #[must_use]
    pub fn aic(&self) -> f64 {
        -2.0 * self.log_likelihood + 2.0 * self.family.n_parameters() as f64
    }

    /// `ln P(X = x)` for a reading with `trials` samples taken (ignored for
    /// counts).
    #[must_use]
    pub fn ln_pmf(&self, x: f64, trials: f64) -> f64 {
        let pi = self.zero_inflation;
        match self.family {
            CountFamily::Poisson => poisson_ln_pmf(x, self.mean),
            CountFamily::NegativeBinomial => nb_ln_pmf(x, self.mean, self.size),
            CountFamily::ZeroInflatedPoisson | CountFamily::ZeroInflatedNegativeBinomial => {
                let base = if self.family == CountFamily::ZeroInflatedPoisson {
                    poisson_ln_pmf(x, self.mean)
                } else {
                    nb_ln_pmf(x, self.mean, self.size)
                };
                if x == 0.0 { (pi + (1.0 - pi) * base.exp()).ln() } else { (-pi).ln_1p() + base }
            }
            CountFamily::Binomial => binomial_ln_pmf(x, trials, self.mean),
            CountFamily::BetaBinomial => {
                beta_binomial_ln_pmf(x, trials, self.mean * self.size, (1.0 - self.mean) * self.size)
            }
        }
    }

    /// Mean of a reading (with `trials` samples taken for positives).
    #[must_use]
    pub fn expected(&self, trials: f64) -> f64 {
        match self.family {
            CountFamily::Binomial | CountFamily::BetaBinomial => trials * self.mean,
            _ => (1.0 - self.zero_inflation) * self.mean,
        }
    }

    /// The smallest `L` with `P(X ≤ L) ≥ q` (`trials` samples taken for
    /// positives); `NaN` unless `0 < q < 1`.
    #[must_use]
    pub fn quantile(&self, q: f64, trials: f64) -> f64 {
        if !(q > 0.0 && q < 1.0) {
            return f64::NAN;
        }
        let top = match self.family {
            CountFamily::Binomial | CountFamily::BetaBinomial => trials,
            _ => f64::INFINITY,
        };
        let mut total = 0.0;
        let mut x = 0.0;
        loop {
            total += self.ln_pmf(x, trials).exp();
            if total >= q * (1.0 - 1e-12) || x >= top || x > 1e7 {
                return x;
            }
            x += 1.0;
        }
    }

    /// The parameters with their names.
    #[must_use]
    pub fn parameters(&self) -> Vec<(&'static str, f64)> {
        match self.family {
            CountFamily::Poisson => vec![("Mean λ", self.mean)],
            CountFamily::NegativeBinomial => vec![("Mean μ", self.mean), ("Size k", self.size)],
            CountFamily::ZeroInflatedPoisson => vec![("Mean λ", self.mean), ("Zero inflation π", self.zero_inflation)],
            CountFamily::ZeroInflatedNegativeBinomial => {
                vec![("Mean μ", self.mean), ("Size k", self.size), ("Zero inflation π", self.zero_inflation)]
            }
            CountFamily::Binomial => vec![("Probability p", self.mean)],
            CountFamily::BetaBinomial => {
                vec![("Mean probability μ", self.mean), ("Intra-class correlation ρ", 1.0 / (self.size + 1.0))]
            }
        }
    }
}

fn check_counts(x: &[f64]) -> Result<()> {
    if x.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: x.len() });
    }
    if x.iter().any(|v| !(v.is_finite() && *v >= 0.0 && v.fract() == 0.0)) {
        return Err(invalid("counts must be whole numbers of zero or more"));
    }
    Ok(())
}

fn log_likelihood(fit: &CountFit, x: &[f64], trials: &[f64]) -> f64 {
    x.iter().enumerate().map(|(i, &v)| fit.ln_pmf(v, trials.get(i).copied().unwrap_or(0.0))).sum()
}

/// Maximum-likelihood fit of a count family to `counts` (whole numbers of
/// zero or more).
///
/// # Errors
/// See the module notes; a positive-sample family is an error here (use
/// [`fit_positives`]).
pub fn fit_counts(counts: &[f64], family: CountFamily) -> Result<CountFit> {
    check_counts(counts)?;
    let n = counts.len();
    let nf = n as f64;
    let mean = counts.iter().sum::<f64>() / nf;
    let var = counts.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (nf - 1.0);
    let zeros = counts.iter().filter(|v| **v == 0.0).count() as f64;
    let mut fit = CountFit { family, mean, size: f64::INFINITY, zero_inflation: 0.0, log_likelihood: 0.0, n };
    let none = |what: &str| invalid(format!("the {} fit reduces to a simpler family: {what}", family.label()));
    match family {
        CountFamily::Poisson => {}
        CountFamily::NegativeBinomial => {
            if var <= mean || mean == 0.0 {
                return Err(none("the counts are not overdispersed"));
            }
            let mut f = |t: &[f64]| -counts.iter().map(|&x| nb_ln_pmf(x, mean, t[0].exp())).sum::<f64>();
            let start = (mean * mean / (var - mean)).ln().clamp(-9.0, 14.0);
            let best = minimize_in_box(&mut f, &[start], &[-10.0], &[15.0], 4000);
            if best.x[0] >= 14.9 {
                return Err(none("the size parameter grows without bound"));
            }
            fit.size = best.x[0].exp();
        }
        CountFamily::ZeroInflatedPoisson => {
            let p0 = zeros / nf;
            if mean == 0.0 || p0 <= (-mean).exp() {
                return Err(none("there are no more zeros than a Poisson distribution expects"));
            }
            // Root of g(λ) = λ(1 − p₀) − x̄(1 − e^{−λ}) on (0, ∞): g(0) = 0,
            // g′(0) < 0 here, and g grows linearly, so bisect above zero.
            let g = |l: f64| l * (1.0 - p0) + mean * (-l).exp_m1();
            let (mut lo, mut hi) = (1e-12, mean.max(1.0));
            while g(hi) <= 0.0 {
                hi *= 2.0;
            }
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if g(mid) > 0.0 { hi = mid } else { lo = mid }
            }
            let lambda = 0.5 * (lo + hi);
            fit.mean = lambda;
            fit.zero_inflation = 1.0 - mean / lambda;
        }
        CountFamily::ZeroInflatedNegativeBinomial => {
            if mean == 0.0 || zeros == 0.0 {
                return Err(none("there are no zeros to inflate"));
            }
            let nll = |t: &[f64]| {
                let pi = 1.0 / (1.0 + (-t[0]).exp());
                let trial =
                    CountFit { family, mean: t[1].exp(), size: t[2].exp(), zero_inflation: pi, log_likelihood: 0.0, n };
                -log_likelihood(&trial, counts, &[])
            };
            // Starts from the negative binomial (little inflation) and from
            // the zero-inflated Poisson (large size).
            let mut starts = Vec::new();
            if let Ok(nb) = fit_counts(counts, CountFamily::NegativeBinomial) {
                starts.push([-4.0, nb.mean.ln(), nb.size.ln()]);
            }
            if let Ok(zip) = fit_counts(counts, CountFamily::ZeroInflatedPoisson) {
                let logit = (zip.zero_inflation / (1.0 - zip.zero_inflation)).ln();
                starts.push([logit.clamp(-12.0, 12.0), zip.mean.ln(), 3.0]);
            }
            let positive_mean = counts.iter().sum::<f64>() / (nf - zeros);
            starts.push([0.0, positive_mean.max(1e-3).ln(), 0.0]);
            let lower = [-15.0, (mean * 1e-3).ln(), -10.0];
            let upper = [15.0, (mean * 1e3).ln(), 15.0];
            let best = starts
                .iter()
                .map(|s| {
                    let mut f = nll;
                    let s: Vec<f64> =
                        s.iter().zip(lower.iter().zip(&upper)).map(|(v, (l, u))| v.clamp(*l, *u)).collect();
                    minimize_in_box(&mut f, &s, &lower, &upper, 20_000)
                })
                .min_by(|a, b| a.value.total_cmp(&b.value))
                .expect("at least one start");
            if best.x[0] <= -14.9 {
                return Err(none("the zero-inflation probability goes to zero"));
            }
            if best.x[2] >= 14.9 {
                return Err(none("the size parameter grows without bound"));
            }
            fit.zero_inflation = 1.0 / (1.0 + (-best.x[0]).exp());
            fit.mean = best.x[1].exp();
            fit.size = best.x[2].exp();
        }
        CountFamily::Binomial | CountFamily::BetaBinomial => {
            return Err(invalid("positive-sample families need the number of samples taken"));
        }
    }
    fit.log_likelihood = log_likelihood(&fit, counts, &[]);
    Ok(fit)
}

/// Maximum-likelihood fit of the binomial or beta-binomial to `positives`
/// out of `trials` samples taken per reading.
///
/// # Errors
/// See the module notes; a count family is an error here.
pub fn fit_positives(positives: &[f64], trials: &[f64], family: CountFamily) -> Result<CountFit> {
    check_counts(positives)?;
    if trials.len() != positives.len() {
        return Err(invalid("give the number of samples taken for every reading"));
    }
    if trials.iter().zip(positives).any(|(n, x)| !(n.is_finite() && *n >= 1.0 && n.fract() == 0.0 && x <= n)) {
        return Err(invalid("samples taken must be whole numbers of at least one, and no fewer than the positives"));
    }
    let n = positives.len();
    let p = positives.iter().sum::<f64>() / trials.iter().sum::<f64>();
    let mut fit = CountFit { family, mean: p, size: f64::INFINITY, zero_inflation: 0.0, log_likelihood: 0.0, n };
    match family {
        CountFamily::Binomial => {}
        CountFamily::BetaBinomial => {
            if p == 0.0 || p == 1.0 {
                return Err(invalid("the beta-binomial needs both positive and negative samples"));
            }
            let mut f = |t: &[f64]| {
                let mu = 1.0 / (1.0 + (-t[0]).exp());
                let s = t[1].exp();
                -positives
                    .iter()
                    .zip(trials)
                    .map(|(&x, &m)| beta_binomial_ln_pmf(x, m, mu * s, (1.0 - mu) * s))
                    .sum::<f64>()
            };
            let logit = (p / (1.0 - p)).ln();
            let lower = [-20.0, -10.0];
            let upper = [20.0, 18.0];
            let best = [2.0_f64, 6.0, 12.0]
                .iter()
                .map(|s0| minimize_in_box(&mut f, &[logit.clamp(-19.0, 19.0), *s0], &lower, &upper, 10_000))
                .min_by(|a, b| a.value.total_cmp(&b.value))
                .expect("starts");
            if best.x[1] >= 17.9 {
                return Err(invalid(
                    "the Beta-binomial fit reduces to a simpler family: the positives are not overdispersed",
                ));
            }
            fit.mean = 1.0 / (1.0 + (-best.x[0]).exp());
            fit.size = best.x[1].exp();
        }
        _ => return Err(invalid("count families do not take the number of samples taken")),
    }
    fit.log_likelihood = log_likelihood(&fit, positives, trials);
    Ok(fit)
}

/// A fitted distribution of either kind.
#[derive(Debug, Clone, PartialEq)]
pub enum MonitoringFit {
    /// Counts or positives.
    Count(CountFit),
    /// Continuous readings.
    Continuous(FittedDistribution),
}

impl MonitoringFit {
    /// Display name of the family.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Count(f) => f.family.label(),
            Self::Continuous(f) => f.family.label(),
        }
    }

    /// AIC.
    #[must_use]
    pub fn aic(&self) -> f64 {
        match self {
            Self::Count(f) => f.aic(),
            Self::Continuous(f) => f.aic(),
        }
    }

    /// Log-likelihood.
    #[must_use]
    pub fn log_likelihood(&self) -> f64 {
        match self {
            Self::Count(f) => f.log_likelihood,
            Self::Continuous(f) => f.log_likelihood,
        }
    }

    /// The parameters with their names.
    #[must_use]
    pub fn parameters(&self) -> Vec<(&'static str, f64)> {
        match self {
            Self::Count(f) => f.parameters(),
            Self::Continuous(f) => f.parameters().into_iter().map(|p| (p.name, p.value)).collect(),
        }
    }

    /// The `q` quantile of a reading (`trials` samples taken for positives).
    #[must_use]
    pub fn quantile(&self, q: f64, trials: f64) -> f64 {
        match self {
            Self::Count(f) => f.quantile(q, trials),
            Self::Continuous(f) => f.quantile(q),
        }
    }
}

/// What the readings are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadingKind {
    /// Counts (colony-forming units and the like).
    #[default]
    Counts,
    /// Positive samples out of samples taken.
    Positives,
    /// Non-negative continuous readings.
    Continuous,
}

/// One candidate family and its fit (or why it could not be fitted).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Family name.
    pub label: &'static str,
    /// The fit, or the reason it failed.
    pub fit: std::result::Result<MonitoringFit, String>,
}

/// Fit every family of `kind` to the readings (with `trials` per reading
/// for positives); the best is the successful fit with the smallest AIC.
#[must_use]
pub fn fit_candidates(values: &[f64], trials: &[f64], kind: ReadingKind) -> Vec<Candidate> {
    match kind {
        ReadingKind::Counts => CountFamily::COUNTS
            .iter()
            .map(|&f| Candidate {
                label: f.label(),
                fit: fit_counts(values, f).map(MonitoringFit::Count).map_err(|e| e.to_string()),
            })
            .collect(),
        ReadingKind::Positives => CountFamily::POSITIVES
            .iter()
            .map(|&f| Candidate {
                label: f.label(),
                fit: fit_positives(values, trials, f).map(MonitoringFit::Count).map_err(|e| e.to_string()),
            })
            .collect(),
        ReadingKind::Continuous => FitFamily::ALL
            .iter()
            .map(|&f| Candidate {
                label: f.label(),
                fit: if values.iter().any(|v| *v < 0.0) {
                    Err("readings must not be negative".to_owned())
                } else {
                    fit_distribution(values, f).map(MonitoringFit::Continuous).map_err(|e| e.to_string())
                },
            })
            .collect(),
    }
}

/// Index of the candidate with the smallest AIC, if any fitted.
#[must_use]
pub fn best_candidate(candidates: &[Candidate]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.fit.as_ref().ok().map(|f| (i, f.aic())))
        .filter(|(_, a)| a.is_finite())
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The early-warning rules applied to each location's readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarningRules {
    /// Rule 3: at least `.0` alert (or action) excursions within `.1`
    /// consecutive readings (`None` to skip).
    pub alerts_in_window: Option<(usize, usize)>,
    /// Rule 4: this many consecutive increases (`None` to skip).
    pub rising_run: Option<usize>,
}

impl Default for WarningRules {
    fn default() -> Self {
        Self { alerts_in_window: Some((2, 5)), rising_run: Some(3) }
    }
}

/// The rule numbers each reading breaks (see the module notes), for one
/// location's readings in time order with their alert and action levels.
#[must_use]
pub fn warning_rules(values: &[f64], alert: &[f64], action: &[f64], rules: &WarningRules) -> Vec<Vec<u8>> {
    let n = values.len();
    let above_alert: Vec<bool> = (0..n).map(|i| values[i] > alert[i] || values[i] > action[i]).collect();
    (0..n)
        .map(|i| {
            let mut broken = Vec::new();
            if values[i] > action[i] {
                broken.push(1);
            } else if values[i] > alert[i] {
                broken.push(2);
            }
            if let Some((k, m)) = rules.alerts_in_window {
                let start = (i + 1).saturating_sub(m.max(1));
                if above_alert[i] && k > 0 && above_alert[start..=i].iter().filter(|b| **b).count() >= k {
                    broken.push(3);
                }
            }
            if let Some(k) = rules.rising_run
                && k > 0
                && i >= k
                && (i + 1 - k..=i).all(|j| values[j] > values[j - 1])
            {
                broken.push(4);
            }
            broken
        })
        .collect()
}

#[cfg(test)]
mod tests;
