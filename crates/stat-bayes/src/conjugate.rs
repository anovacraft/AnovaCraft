//! Conjugate Bayesian analyses with closed-form posteriors (Gelman et al.,
//! *Bayesian Data Analysis*, 3rd ed., 2013, chs. 2–3).
//!
//! | Analysis | Prior | Posterior |
//! |---|---|---|
//! | Proportion, `y` successes in `n` trials | Beta(`a`, `b`) | Beta(`a + y`, `b + n − y`) |
//! | Event rate, `y` events in exposure `t` | Gamma(`α`, `β`) | Gamma(`α + y`, `β + t`) |
//! | Normal mean, Jeffreys `p(μ, σ²) ∝ 1/σ²` | — | `μ ~ ȳ + (s/√n) t_{n−1}`; `σ² ~ Scaled-Inv-χ²(n − 1, s²)` |
//! | Normal mean, informative mean `μ₀` worth `κ₀` observations | `μ │ σ² ~ N(μ₀, σ²/κ₀)`, `p(σ²) ∝ 1/σ²` | `μ ~ μₙ + σₙ/√κₙ · t_n` (BDA eq. 3.6 with `ν₀ = 0`) |
//!
//! with `κₙ = κ₀ + n`, `μₙ = (κ₀μ₀ + nȳ)/κₙ` and
//! `nσₙ² = (n − 1)s² + κ₀n(ȳ − μ₀)²/κₙ`. Default priors: the uniform
//! Beta(1, 1) for a proportion (Bayes–Laplace), the Jeffreys Gamma(½, 0)
//! for a rate, and Jeffreys' prior for the normal model. An informative beta
//! or gamma prior is set by its mean and standard deviation (method of
//! moments: Beta with `a + b = m(1 − m)/sd² − 1`; Gamma with shape `m²/sd²`
//! and rate `m/sd²`).
//!
//! Two groups compare independent posteriors: the difference, ratio or odds
//! ratio of two proportions, the ratio of two rates (exactly a scaled F:
//! `λ₁/λ₂ ~ (α₁'β₂'/(β₁'α₂')) F(2α₁', 2α₂')`), and the difference of two
//! means — with equal variances, `t_{n₁+n₂−2}` around `ȳ₁ − ȳ₂` with the
//! pooled scale; with unequal variances, the Behrens–Fisher posterior, the
//! difference of two independent t posteriors (Box & Tiao 1973, §2.5). Their
//! distributions are computed by quadrature (see [`crate::univariate`]).
//!
//! For a proper prior, the Bayes factor for a point null `θ = θ₀` against
//! the prior is the Savage–Dickey density ratio
//! `BF₀₁ = p(θ₀ │ y) / p(θ₀)` (Dickey 1971, *Ann. Math. Stat.* 42).

use crate::univariate::{BetaDist, Combined, Contrast, GammaDist, ScaledT, SigmaDist, Summary, Univariate, summarize};
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// A beta prior for a proportion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BetaPrior {
    /// First shape (prior successes).
    pub a: f64,
    /// Second shape (prior failures).
    pub b: f64,
}

impl BetaPrior {
    /// Uniform Beta(1, 1).
    pub const UNIFORM: Self = Self { a: 1.0, b: 1.0 };
    /// Jeffreys Beta(½, ½).
    pub const JEFFREYS: Self = Self { a: 0.5, b: 0.5 };

    /// The beta with mean `mean` and standard deviation `sd`.
    ///
    /// # Errors
    /// A mean outside (0, 1), or an SD too large for a beta with that mean
    /// (`sd² ≥ m(1 − m)`).
    pub fn from_mean_sd(mean: f64, sd: f64) -> Result<Self> {
        if !(mean > 0.0 && mean < 1.0) {
            return Err(invalid("the prior mean of a proportion must lie between 0 and 1"));
        }
        let v = sd * sd;
        if !(sd > 0.0 && v < mean * (1.0 - mean)) {
            return Err(invalid(format!(
                "a beta prior with mean {mean} needs an SD between 0 and {:.4}",
                (mean * (1.0 - mean)).sqrt()
            )));
        }
        let k = mean * (1.0 - mean) / v - 1.0;
        Ok(Self { a: mean * k, b: (1.0 - mean) * k })
    }
}

/// A gamma prior for an event rate (`rate = 0` with `shape = ½` is the
/// improper Jeffreys prior).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GammaPrior {
    /// Shape (prior events).
    pub shape: f64,
    /// Rate (prior exposure).
    pub rate: f64,
}

impl GammaPrior {
    /// Jeffreys `p(λ) ∝ λ^{−1/2}`.
    pub const JEFFREYS: Self = Self { shape: 0.5, rate: 0.0 };

    /// The gamma with mean `mean` and standard deviation `sd`.
    ///
    /// # Errors
    /// A non-positive mean or SD.
    pub fn from_mean_sd(mean: f64, sd: f64) -> Result<Self> {
        if !(mean > 0.0 && sd > 0.0 && mean.is_finite() && sd.is_finite()) {
            return Err(invalid("a gamma prior needs a positive mean and SD"));
        }
        Ok(Self { shape: mean * mean / (sd * sd), rate: mean / (sd * sd) })
    }

    /// `true` for a proper prior.
    #[must_use]
    pub fn is_proper(&self) -> bool {
        self.rate > 0.0
    }
}

/// The prior of a normal mean.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeanPrior {
    /// Jeffreys `p(μ, σ²) ∝ 1/σ²`.
    Jeffreys,
    /// `μ │ σ² ~ N(mean, σ²/weight)` with `p(σ²) ∝ 1/σ²`: a prior mean worth
    /// `weight` observations.
    Informative {
        /// Prior mean `μ₀`.
        mean: f64,
        /// Prior weight `κ₀` in observations (positive).
        weight: f64,
    },
}

/// Probability statements about a posterior: below and above a reference
/// value, and inside a region of practical equivalence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probabilities {
    /// Reference value.
    pub reference: f64,
    /// `P(θ < reference │ y)`.
    pub below: f64,
    /// `P(θ > reference │ y)`.
    pub above: f64,
    /// Region of practical equivalence `(low, high)` and `P(low < θ < high │ y)`.
    pub rope: Option<(f64, f64, f64)>,
}

/// `P(θ < r)`, `P(θ > r)` and the ROPE probability of `d`.
#[must_use]
pub fn probabilities(d: &dyn Univariate, reference: f64, rope: Option<(f64, f64)>) -> Probabilities {
    let below = d.cdf(reference);
    Probabilities {
        reference,
        below,
        above: 1.0 - below,
        rope: rope.map(|(lo, hi)| (lo, hi, (d.cdf(hi) - d.cdf(lo)).max(0.0))),
    }
}

/// A one-group conjugate analysis: prior, posterior and summaries.
#[derive(Debug, Clone)]
pub struct OneGroup<D> {
    /// The prior (`None` for an improper prior).
    pub prior: Option<D>,
    /// The posterior.
    pub posterior: D,
    /// Posterior summary.
    pub summary: Summary,
    /// Probability statements about the reference value.
    pub probabilities: Probabilities,
    /// Savage–Dickey Bayes factor `BF₁₀` of "θ ≠ reference" against
    /// "θ = reference" (`None` for an improper prior).
    pub bayes_factor: Option<f64>,
}

fn one_group<D: Univariate + Clone>(
    prior: Option<D>,
    posterior: D,
    level: f64,
    reference: f64,
    rope: Option<(f64, f64)>,
) -> OneGroup<D> {
    let bayes_factor = prior.as_ref().map(|p| p.pdf(reference) / posterior.pdf(reference));
    OneGroup {
        summary: summarize(&posterior, level),
        probabilities: probabilities(&posterior, reference, rope),
        bayes_factor: bayes_factor.filter(|b| b.is_finite()),
        prior,
        posterior,
    }
}

fn check_level(level: f64) -> Result<()> {
    if level > 0.0 && level < 1.0 { Ok(()) } else { Err(invalid("the credible level must lie between 0 and 1")) }
}

/// Posterior of a proportion from `successes` in `trials`, with Bayes factor
/// and probabilities about `reference` (e.g. 0.5).
///
/// # Errors
/// Successes outside `0..=trials`, an invalid prior, or level.
pub fn proportion(
    successes: f64,
    trials: f64,
    prior: BetaPrior,
    level: f64,
    reference: f64,
    rope: Option<(f64, f64)>,
) -> Result<OneGroup<BetaDist>> {
    check_level(level)?;
    if !(trials > 0.0 && successes >= 0.0 && successes <= trials) {
        return Err(invalid("successes must lie between 0 and the number of trials, which must be positive"));
    }
    let p = BetaDist::new(prior.a, prior.b)?;
    let post = BetaDist::new(prior.a + successes, prior.b + trials - successes)?;
    Ok(one_group(Some(p), post, level, reference, rope))
}

/// Posterior of an event rate from `events` over `exposure`.
///
/// # Errors
/// Negative events, non-positive exposure, an invalid prior, or level.
pub fn rate(
    events: f64,
    exposure: f64,
    prior: GammaPrior,
    level: f64,
    reference: f64,
    rope: Option<(f64, f64)>,
) -> Result<OneGroup<GammaDist>> {
    check_level(level)?;
    if !(events >= 0.0 && exposure > 0.0 && events.is_finite() && exposure.is_finite()) {
        return Err(invalid("events must be non-negative and the exposure positive"));
    }
    if !(prior.shape > 0.0 && prior.rate >= 0.0) {
        return Err(invalid("the gamma prior needs a positive shape and a non-negative rate"));
    }
    let p = if prior.is_proper() { Some(GammaDist::new(prior.shape, prior.rate)?) } else { None };
    let post = GammaDist::new(prior.shape + events, prior.rate + exposure)?;
    Ok(one_group(p, post, level, reference, rope))
}

/// Summary statistics of a normal sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleStats {
    /// Size.
    pub n: f64,
    /// Mean.
    pub mean: f64,
    /// Standard deviation (divisor `n − 1`).
    pub sd: f64,
}

impl SampleStats {
    /// Statistics of `x` (finite values only).
    ///
    /// # Errors
    /// Fewer than two finite values, or values so large that their mean or
    /// spread overflows.
    pub fn of(x: &[f64]) -> Result<Self> {
        let v: Vec<f64> = x.iter().copied().filter(|v| v.is_finite()).collect();
        if v.len() < 2 {
            return Err(StatError::InsufficientData { required: 2, found: v.len() });
        }
        let n = v.len() as f64;
        let mean = stat_core::summation::sum(&v) / n;
        let ss: f64 = v.iter().map(|x| (x - mean) * (x - mean)).sum();
        let sd = (ss / (n - 1.0)).sqrt();
        if !(mean.is_finite() && sd.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "the values are too large to summarize" });
        }
        Ok(Self { n, mean, sd })
    }
}

/// A normal-model analysis of one sample: the mean and the standard
/// deviation.
#[derive(Debug, Clone)]
pub struct NormalMean {
    /// Data summary.
    pub stats: SampleStats,
    /// The mean's analysis (prior `None`: improper).
    pub mean: OneGroup<ScaledT>,
    /// Posterior of the standard deviation.
    pub sigma: SigmaDist,
    /// Its summary.
    pub sigma_summary: Summary,
}

/// Posterior of a normal mean (and SD) under `prior`.
///
/// # Errors
/// Fewer than two values, zero spread with the Jeffreys prior, a
/// non-positive prior weight, or an invalid level.
pub fn normal_mean(
    stats: SampleStats,
    prior: MeanPrior,
    level: f64,
    reference: f64,
    rope: Option<(f64, f64)>,
) -> Result<NormalMean> {
    check_level(level)?;
    let SampleStats { n, mean, sd } = stats;
    if n < 2.0 {
        return Err(StatError::InsufficientData { required: 2, found: n as usize });
    }
    let (location, scale, df, sigma_df, sigma2, prior_dist) = match prior {
        MeanPrior::Jeffreys => {
            if sd.is_nan() || sd <= 0.0 {
                return Err(invalid("the values do not vary, so the posterior of the mean is degenerate"));
            }
            (mean, sd / n.sqrt(), n - 1.0, n - 1.0, sd * sd, None)
        }
        MeanPrior::Informative { mean: m0, weight: k0 } => {
            if !(k0 > 0.0 && m0.is_finite()) {
                return Err(invalid("the prior weight must be a positive number of observations"));
            }
            let kn = k0 + n;
            let mn = (k0 * m0 + n * mean) / kn;
            let s2 = ((n - 1.0) * sd * sd + k0 * n * (mean - m0) * (mean - m0) / kn) / n;
            if s2.is_nan() || s2 <= 0.0 {
                return Err(invalid("the values do not vary, so the posterior of the mean is degenerate"));
            }
            // The prior on μ alone is improper (σ² has no prior scale).
            (mn, (s2 / kn).sqrt(), n, n, s2, None)
        }
    };
    let post = ScaledT::new(location, scale, df)?;
    let sigma = SigmaDist::new(sigma_df, sigma2)?;
    Ok(NormalMean {
        stats,
        mean: one_group(prior_dist, post, level, reference, rope),
        sigma_summary: summarize(&sigma, level),
        sigma,
    })
}

/// How two proportions are compared.
pub type ProportionContrast = Contrast;

/// A two-group analysis: the posteriors and their contrast.
#[derive(Debug, Clone)]
pub struct TwoGroups<D> {
    /// Posterior of the first group.
    pub first: OneGroup<D>,
    /// Posterior of the second group.
    pub second: OneGroup<D>,
    /// The contrast.
    pub contrast: Contrast,
    /// Summary of the contrast's posterior.
    pub summary: Summary,
    /// Probability statements about the contrast (reference 0 for a
    /// difference, 1 for ratios).
    pub probabilities: Probabilities,
    /// Density of the contrast on a grid spanning its 0.1%–99.9% range.
    pub density: Vec<[f64; 2]>,
}

fn contrast_density(d: &dyn Univariate, lo: f64, hi: f64) -> Vec<[f64; 2]> {
    let n = 160;
    (0..=n)
        .map(|i| {
            let x = lo + (hi - lo) * i as f64 / n as f64;
            [x, d.pdf(x)]
        })
        .collect()
}

fn two_groups<D: Univariate + Clone>(
    first: OneGroup<D>,
    second: OneGroup<D>,
    contrast: Contrast,
    level: f64,
    rope: Option<(f64, f64)>,
) -> TwoGroups<D> {
    let c = Combined::new(&first.posterior, &second.posterior, contrast);
    let reference = if contrast == Contrast::Difference { 0.0 } else { 1.0 };
    let summary = summarize(&c, level);
    let probabilities = probabilities(&c, reference, rope);
    let density = contrast_density(&c, c.quantile(0.001), c.quantile(0.999));
    TwoGroups { first, second, contrast, summary, probabilities, density }
}

/// Two independent proportions compared by `contrast`.
///
/// # Errors
/// As [`proportion`] for either group.
pub fn two_proportions(
    first: (f64, f64),
    second: (f64, f64),
    prior: BetaPrior,
    contrast: Contrast,
    level: f64,
    rope: Option<(f64, f64)>,
) -> Result<TwoGroups<BetaDist>> {
    let a = proportion(first.0, first.1, prior, level, 0.5, None)?;
    let b = proportion(second.0, second.1, prior, level, 0.5, None)?;
    Ok(two_groups(a, b, contrast, level, rope))
}

/// The ratio of two event rates.
///
/// # Errors
/// As [`rate`] for either group.
pub fn two_rates(
    first: (f64, f64),
    second: (f64, f64),
    prior: GammaPrior,
    level: f64,
    rope: Option<(f64, f64)>,
) -> Result<TwoGroups<GammaDist>> {
    let a = rate(first.0, first.1, prior, level, 0.0, None)?;
    let b = rate(second.0, second.1, prior, level, 0.0, None)?;
    Ok(two_groups(a, b, Contrast::Ratio, level, rope))
}

/// The difference of two normal means under Jeffreys priors: with
/// `equal_variances`, the pooled-variance t posterior; otherwise the
/// Behrens–Fisher posterior. Returns the two groups' mean posteriors and the
/// difference.
///
/// # Errors
/// Fewer than two values in a group, or no spread.
pub fn two_means(
    first: SampleStats,
    second: SampleStats,
    equal_variances: bool,
    level: f64,
    rope: Option<(f64, f64)>,
) -> Result<TwoGroups<ScaledT>> {
    check_level(level)?;
    let a = normal_mean(first, MeanPrior::Jeffreys, level, 0.0, None)?;
    let b = normal_mean(second, MeanPrior::Jeffreys, level, 0.0, None)?;
    if !equal_variances {
        return Ok(two_groups(a.mean, b.mean, Contrast::Difference, level, rope));
    }
    let df = first.n + second.n - 2.0;
    let sp2 = ((first.n - 1.0) * first.sd * first.sd + (second.n - 1.0) * second.sd * second.sd) / df;
    let d = ScaledT::new(first.mean - second.mean, (sp2 * (1.0 / first.n + 1.0 / second.n)).sqrt(), df)?;
    let summary = summarize(&d, level);
    let probabilities = probabilities(&d, 0.0, rope);
    let density = contrast_density(&d, d.quantile(0.001), d.quantile(0.999));
    Ok(TwoGroups { first: a.mean, second: b.mean, contrast: Contrast::Difference, summary, probabilities, density })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrate::integrate;

    #[test]
    fn beta_binomial_posterior() {
        // BDA §2.4: placenta previa, 437 female of 980 births, uniform prior:
        // posterior mean 0.446, sd 0.016, 95% interval [0.415, 0.477].
        let r = proportion(437.0, 980.0, BetaPrior::UNIFORM, 0.95, 0.485, None).unwrap();
        assert!((r.summary.mean - 438.0 / 982.0).abs() < 1e-14);
        assert!((r.summary.sd - 0.016).abs() < 5e-4);
        assert!((r.summary.equal_tailed.0 - 0.415).abs() < 5e-4);
        assert!((r.summary.equal_tailed.1 - 0.477).abs() < 5e-4);
        assert!(r.probabilities.above < 0.01);
        let p = BetaPrior::from_mean_sd(0.3, 0.1).unwrap();
        assert!((p.a - 6.0).abs() < 1e-12 && (p.b - 14.0).abs() < 1e-12);
        assert!(BetaPrior::from_mean_sd(0.5, 0.6).is_err());
    }

    #[test]
    fn savage_dickey_matches_marginal_likelihoods() {
        // BF₁₀ = m₁/m₀ with m₁ = C(n,y) B(a+y, b+n−y)/B(a,b), m₀ = C(n,y) θ₀^y (1−θ₀)^{n−y}.
        let (y, n) = (7.0, 20.0);
        let r = proportion(y, n, BetaPrior::UNIFORM, 0.95, 0.5, None).unwrap();
        let ln_m1 = stat_core::special::ln_beta(1.0 + y, 1.0 + n - y);
        let ln_m0 = n * 0.5f64.ln();
        assert!((r.bayes_factor.unwrap().ln() - (ln_m1 - ln_m0)).abs() < 1e-10);
    }

    #[test]
    fn rates_and_ratio() {
        let r = rate(13.0, 4.5, GammaPrior::JEFFREYS, 0.95, 2.0, None).unwrap();
        assert!((r.summary.mean - 13.5 / 4.5).abs() < 1e-14);
        assert!(r.bayes_factor.is_none());
        let two = two_rates((13.0, 4.5), (5.0, 3.0), GammaPrior::JEFFREYS, 0.95, None).unwrap();
        // λ₁/λ₂ ~ (13.5/4.5)/(5.5/3) · F(27, 11).
        let k = (13.5 / 4.5) / (5.5 / 3.0);
        let f = stat_core::FisherSnedecor::new(27.0, 11.0).unwrap();
        use stat_core::ContinuousDistribution;
        let q = k * f.quantile(0.975).unwrap();
        assert!((two.summary.equal_tailed.1 / q - 1.0).abs() < 1e-8);
        assert!((two.probabilities.above - f.sf(1.0 / k).unwrap()).abs() < 1e-9);
    }

    #[test]
    fn normal_means() {
        let x = [9.8, 10.2, 10.4, 9.9, 10.0, 10.6, 10.1];
        let s = SampleStats::of(&x).unwrap();
        let r = normal_mean(s, MeanPrior::Jeffreys, 0.95, 10.0, Some((9.9, 10.1))).unwrap();
        // Jeffreys: the credible interval equals the t confidence interval.
        use stat_core::ContinuousDistribution;
        let t = stat_core::StudentsT::new(6.0).unwrap().quantile(0.975).unwrap();
        assert!((r.mean.summary.equal_tailed.1 - (s.mean + t * s.sd / 7f64.sqrt())).abs() < 1e-12);
        // Informative prior pulls the mean toward μ₀ by κ₀/(κ₀ + n).
        let i = normal_mean(s, MeanPrior::Informative { mean: 9.0, weight: 3.0 }, 0.95, 10.0, None).unwrap();
        assert!((i.mean.summary.mean - (3.0 * 9.0 + 7.0 * s.mean) / 10.0).abs() < 1e-12);
        // Two means: P(δ > 0) for the Behrens–Fisher posterior equals
        // ∫ f₂(x) S₁(x) dx.
        let y = [9.1, 9.6, 9.9, 9.4, 10.3];
        let s2 = SampleStats::of(&y).unwrap();
        let d = two_means(s, s2, false, 0.95, None).unwrap();
        let direct = integrate(
            |v| d.second.posterior.pdf(v) * d.first.posterior.sf(v),
            d.second.posterior.quantile(1e-12),
            d.second.posterior.quantile(1.0 - 1e-12),
            1e-12,
            16,
        );
        assert!((d.probabilities.above - direct).abs() < 1e-9);
        let pooled = two_means(s, s2, true, 0.95, None).unwrap();
        assert!((pooled.summary.mean - (s.mean - s2.mean)).abs() < 1e-12);
    }

    /// Zero events in the second group: E[1/X₂] diverges (Beta(1, 11) after
    /// 0 of 10 under the uniform prior; Gamma(½, 3) under Jeffreys), so the
    /// ratio's mean and SD are undefined (NaN, shown as "—"), not a large
    /// number set by the quadrature cut-off. The median and intervals stay.
    #[test]
    fn divergent_ratio_moments_are_undefined() {
        let two = two_proportions((3.0, 10.0), (0.0, 10.0), BetaPrior::UNIFORM, Contrast::Ratio, 0.95, None).unwrap();
        assert!(two.summary.mean.is_nan() && two.summary.sd.is_nan(), "{:?}", two.summary);
        assert!(two.summary.median.is_finite() && two.summary.hpd.0.is_finite() && two.summary.hpd.1.is_finite());
        let r = two_rates((13.0, 4.5), (0.0, 3.0), GammaPrior::JEFFREYS, 0.95, None).unwrap();
        assert!(r.summary.mean.is_nan() && r.summary.sd.is_nan(), "{:?}", r.summary);
        assert!(r.summary.median.is_finite());
        // Odds ratio with every trial a success in the first group: Beta(11, 1)
        // has no E[X/(1 − X)].
        let or =
            two_proportions((10.0, 10.0), (4.0, 10.0), BetaPrior::UNIFORM, Contrast::OddsRatio, 0.95, None).unwrap();
        assert!(or.summary.mean.is_nan());
        // Beta(3, 9) has E[X⁻¹] and E[X⁻²] (a = 3 > 2): mean and SD exist.
        let ok = two_proportions((3.0, 10.0), (2.0, 10.0), BetaPrior::UNIFORM, Contrast::Ratio, 0.95, None).unwrap();
        // E[X₁]·E[1/X₂] for Beta(4, 8) and Beta(3, 9): (4/12)·(11/2).
        let mean = (4.0 / 12.0) * (11.0 / 2.0);
        assert!((ok.summary.mean - mean).abs() < 1e-8 * mean, "{}", ok.summary.mean);
        // E[X₁²] = 4·5/(12·13), E[X₂⁻²] = 11·10/(2·1).
        let sd = (20.0 / 156.0 * 55.0 - mean * mean).sqrt();
        assert!((ok.summary.sd - sd).abs() < 1e-7 * sd, "{} vs {sd}", ok.summary.sd);
    }
}
