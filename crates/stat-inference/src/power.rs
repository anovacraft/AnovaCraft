//! Power, sample size and detectable difference.
//!
//! Power is the probability that a test at level `α` rejects `H₀` when a
//! specified alternative is true. Each design below has a power function of
//! the sample size `n` (per group; the second group has `round(r·n)` units for
//! an allocation ratio `r`):
//!
//! | Test | Power (one-sided `>` shown; two-sided adds the other tail) |
//! |---|---|
//! | One-sample / paired t | `P(T'_{n−1,δ√n/σ} > t_{1−α})` — noncentral t |
//! | Two-sample t (pooled) | `P(T'_{n₁+n₂−2, δ/(σ√(1/n₁+1/n₂))} > t_{1−α})` |
//! | One-way ANOVA, `k` groups | `P(F'_{k−1, k(n−1), λ} > F_{1−α})`, `λ = nD²/(2σ²)` for a largest difference `D` between means (the least favourable spread: two means `D` apart, the rest midway) |
//! | One proportion (score test) | `Φ((p₁ − p₀ − z_{1−α}√(p₀q₀/n)) / √(p₁q₁/n))` |
//! | Two proportions (pooled z) | `Φ((p₁ − p₂ − z_{1−α}σ₀)/σ₁)`, `σ₀² = p̄q̄(1/n₁+1/n₂)`, `σ₁² = p₁q₁/n₁ + p₂q₂/n₂` |
//! | One standard deviation (χ²) | `P(χ²_{n−1} > χ²_{1−α}σ₀²/σ₁²)` |
//! | Two variances (F) | `P(F_{n₁−1,n₂−1} > F_{1−α}/ρ)`, `ρ = σ₁²/σ₂²` |
//! | Equivalence (TOST) | exact: `∫ f_{χ²_ν}(v) [Φ((U − cŝ − δ)/s) − Φ((L + cŝ − δ)/s)]₊ dv`, `ŝ = s√(v/ν)`, `c = t_{1−α,ν}` |
//! | One-sample z (σ known) | `Φ(δ√n/σ − z_{1−α})` |
//! | Paired t from the two SDs | as one-sample t with `σ_d = σ√(2(1 − ρ))`, `ρ` the correlation of the pair |
//! | One Poisson rate (score z) | `Φ((λ₁ − λ₀ − z_{1−α}√(λ₀/t))/√(λ₁/t))`, exposure `t = n·e` |
//! | Two Poisson rates (z) | `Φ((λ₁ − λ₂ − z_{1−α}σ₀)/σ₁)`, `σ₀² = λ̄(1/t₁ + 1/t₂)`, `σ₁² = λ₁/t₁ + λ₂/t₂`, `λ̄` the exposure-weighted mean |
//! | Two-level factorial or screening design, one effect `Δ` (high − low) | `P(F'_{1, ν_E, λ} > F_{1−α})`, `λ = r·m·Δ²/(4σ²)` for `r` replicates of `m` runs; `ν_E = rm + c − 1 − p` with `c` centre points and `p` model terms |
//! | General full factorial, one factor with `L` levels, largest difference `D` | `P(F'_{L−1, LC(r−1), λ} > F_{1−α})`, `λ = rCD²/(2σ²)`, `C` the cells of the other factors (full model) |
//!
//! **Estimation.** For a planned confidence interval the "power" is the
//! probability that the interval's margin of error (half-width, or the
//! distance from the estimate to the farther limit) is at most `E` (Kupper &
//! Hafner 1989, *Am. Statistician* 43; Beal 1989, *Biometrics* 45):
//!
//! | Interval | `P(margin ≤ E)` |
//! |---|---|
//! | Mean, `x̄ ± t_{1−α/2} s/√n` | `P(χ²_{n−1} ≤ (n−1)nE²/(t²σ²))` |
//! | Standard deviation, `s√((n−1)/χ²_{1−α/2})` to `s√((n−1)/χ²_{α/2})` | `P(χ²_{n−1} ≤ (n−1)(E/((b − 1)σ))²)`, `b = √((n−1)/χ²_{α/2})` |
//! | Proportion (Wilson score interval) | `Σₓ b(x; n, p)·[h(x) ≤ E]`, `h` the Wilson half-width at `x` |
//! | Poisson rate (score interval, exposure `t = n·e`) | `P(X ≤ (Et/z)² − z²/4)`, `X ~ Poisson(λt)`, half-width `z√(X + z²/4)/t` |
//!
//! **Tolerance intervals** (normal, factor `k` for content `P` with
//! confidence `γ`): the "power" is the probability that the interval does
//! not contain more than `P′ > P` of the population, `1 −
//! P(content ≥ P′)` (Faulkenberry & Weeks 1968, *Technometrics* 10; Odeh &
//! Owen, *Tables for Normal Tolerance Limits, Sampling Plans and
//! Screening*, 1980), computed exactly from
//! [`crate::tolerance::tolerance_content_probability`]. At `P′ = P` it is
//! `1 − γ`.
//!
//! **Distribution-free tolerance intervals** (Wilks 1941, *Ann. Math.
//! Statist.* 12; Guenther 1977, *Sampling Inspection in Statistical Quality
//! Control*, ch. 6): the interval from the `r`-th smallest to the `s`-th
//! largest of `n` values (any continuous population) leaves `m = r + s` order
//! statistics outside it, and its content is `Beta(n − m + 1, m)`. The
//! "power" is the confidence `P(content ≥ P) = P(Bin(n, P) ≤ n − m) =
//! Σ_{j=0}^{n−m} C(n, j) Pʲ(1 − P)^{n−j}`; the extremes give `1 − Pⁿ` for
//! one limit (`m = 1`) and `1 − nPⁿ⁻¹ + (n − 1)Pⁿ` for two (`m = 2`). The
//! sample size is the smallest `n` reaching the target confidence, and the
//! "effect" is `P` (the largest content reached with that confidence).
//!
//! (Cohen, *Statistical Power Analysis for the Behavioral Sciences*, 2nd ed.;
//! Chow, Shao & Wang, *Sample Size Calculations in Clinical Research*, 2nd ed.,
//! §§3.1–4.2, 10.3; Fleiss, Levin & Paik, *Statistical Methods for Rates and
//! Proportions*, 3rd ed., §4.2; Phillips 1990, *J. Pharmacokinet. Biopharm.*
//! 18:137 for TOST.) The proportion formulas are the usual normal
//! approximations.
//!
//! The required sample size is the smallest `n` whose power reaches the
//! target (found by doubling then bisection); the detectable difference is the
//! smallest effect, in the direction of the alternative, with that power.

use crate::common::{Alternative, invalid};
use crate::tolerance::{one_sided_tolerance_factor, tolerance_content_probability, two_sided_tolerance_factor};
use stat_core::discrete::{binomial_cdf, binomial_pmf, poisson_cdf};
use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
use stat_core::quadrature::CompositeRule;
use stat_core::{NoncentralF, NoncentralT, Result};

/// A design whose power is to be computed. Effects are expressed in the
/// units of the data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerDesign {
    /// One-sample or paired t test: true difference `δ = μ − μ₀`, standard
    /// deviation `σ` (of the differences, for paired data).
    OneSampleT {
        /// `δ`.
        difference: f64,
        /// `σ`.
        std_dev: f64,
    },
    /// Two-sample pooled t test: true difference of means, common `σ`, and
    /// allocation ratio `n₂/n₁`.
    TwoSampleT {
        /// `μ₁ − μ₂`.
        difference: f64,
        /// `σ`.
        std_dev: f64,
        /// `n₂/n₁`.
        ratio: f64,
    },
    /// One-way ANOVA with `groups` groups of equal size, the largest
    /// difference `D` between two means, and error standard deviation `σ`.
    OneWayAnova {
        /// Number of groups `k ≥ 2`.
        groups: usize,
        /// `D`.
        max_difference: f64,
        /// `σ`.
        std_dev: f64,
    },
    /// Score test of one proportion: null `p₀`, true `p₁`.
    OneProportion {
        /// `p₀`.
        null: f64,
        /// `p₁`.
        actual: f64,
    },
    /// Pooled z test of two proportions: true `p₁`, `p₂`, allocation `n₂/n₁`.
    TwoProportions {
        /// `p₁`.
        first: f64,
        /// `p₂`.
        second: f64,
        /// `n₂/n₁`.
        ratio: f64,
    },
    /// Chi-square test of one standard deviation: null `σ₀`, true `σ₁`.
    OneStdDev {
        /// `σ₀`.
        null: f64,
        /// `σ₁`.
        actual: f64,
    },
    /// F test of two variances: true ratio `σ₁²/σ₂²`, allocation `n₂/n₁`.
    TwoVariances {
        /// `σ₁²/σ₂²`.
        variance_ratio: f64,
        /// `n₂/n₁`.
        ratio: f64,
    },
    /// TOST equivalence of one mean (or paired difference) within
    /// `(lower, upper)`, true difference `δ`, standard deviation `σ`.
    EquivalenceOneSample {
        /// `L`.
        lower: f64,
        /// `U`.
        upper: f64,
        /// `δ`.
        difference: f64,
        /// `σ`.
        std_dev: f64,
    },
    /// TOST equivalence of two means (pooled), allocation `n₂/n₁`.
    EquivalenceTwoSample {
        /// `L`.
        lower: f64,
        /// `U`.
        upper: f64,
        /// `μ₁ − μ₂`.
        difference: f64,
        /// `σ`.
        std_dev: f64,
        /// `n₂/n₁`.
        ratio: f64,
    },
    /// One-sample z test with a known standard deviation.
    OneSampleZ {
        /// `δ = μ − μ₀`.
        difference: f64,
        /// Known `σ`.
        std_dev: f64,
    },
    /// Paired t test planned from the standard deviation of each
    /// measurement and their correlation.
    PairedT {
        /// Mean difference `δ`.
        difference: f64,
        /// Standard deviation of each measurement `σ`.
        std_dev: f64,
        /// Correlation `ρ` between the paired measurements.
        correlation: f64,
    },
    /// Test of one Poisson rate (events per unit of exposure); each sampled
    /// unit contributes `exposure`.
    OneRate {
        /// `λ₀`.
        null: f64,
        /// `λ₁`.
        actual: f64,
        /// Exposure per unit `e`.
        exposure: f64,
    },
    /// Comparison of two Poisson rates; allocation `n₂/n₁`.
    TwoRates {
        /// `λ₁`.
        first: f64,
        /// `λ₂`.
        second: f64,
        /// Exposure per unit `e`.
        exposure: f64,
        /// `n₂/n₁`.
        ratio: f64,
    },
    /// Confidence interval for a mean (`t`, σ unknown): margin `E`.
    MeanMargin {
        /// Largest acceptable half-width `E`.
        margin: f64,
        /// Planning value of `σ`.
        std_dev: f64,
    },
    /// Wilson score interval for a proportion: margin `E`.
    ProportionMargin {
        /// Largest acceptable half-width `E`.
        margin: f64,
        /// Planning value of `p`.
        proportion: f64,
    },
    /// Chi-square interval for a standard deviation: margin `E` (from the
    /// estimate to the farther, upper, limit).
    StdDevMargin {
        /// Largest acceptable margin `E`.
        margin: f64,
        /// Planning value of `σ`.
        std_dev: f64,
    },
    /// Score interval for a Poisson rate: margin `E`.
    RateMargin {
        /// Largest acceptable half-width `E`.
        margin: f64,
        /// Planning value of `λ`.
        rate: f64,
        /// Exposure per unit `e`.
        exposure: f64,
    },
    /// Normal tolerance interval containing `proportion` with `confidence`,
    /// not to contain more than `max_proportion`.
    ToleranceInterval {
        /// Content `P`.
        proportion: f64,
        /// Confidence `γ`.
        confidence: f64,
        /// `P′ > P`: content the interval should rarely exceed.
        max_proportion: f64,
        /// Two limits (`false`: one limit).
        two_sided: bool,
    },
    /// Distribution-free tolerance interval between order statistics: the
    /// "power" is the confidence that it contains at least `proportion` of
    /// the population (see the module documentation).
    DistributionFreeTolerance {
        /// Content `P`.
        proportion: f64,
        /// Order statistics outside the interval, `m = r + s` (1 for the
        /// sample minimum or maximum as one limit, 2 for both extremes).
        removed: usize,
        /// Two limits (`false`: one limit); two limits need `m ≥ 2`.
        two_sided: bool,
    },
    /// Two-level full or fractional factorial `2^{k−q}`; the sample size is
    /// the number of replicates. The model has the main effects, and the
    /// two-factor interactions when `interactions`.
    TwoLevelFactorial {
        /// Factors `k`.
        factors: usize,
        /// Fraction exponent `q` (0 for a full factorial).
        fraction: usize,
        /// Centre points in the whole experiment.
        center_points: usize,
        /// Two-factor interactions in the model.
        interactions: bool,
        /// Effect `Δ` (mean at high − mean at low) of one factor.
        effect: f64,
        /// `σ`.
        std_dev: f64,
    },
    /// Screening design (Plackett–Burman) with `runs` runs per replicate and
    /// a main-effects model.
    Screening {
        /// Runs per replicate (a multiple of four).
        runs: usize,
        /// Factors.
        factors: usize,
        /// Centre points in the whole experiment.
        center_points: usize,
        /// Effect `Δ` of one factor.
        effect: f64,
        /// `σ`.
        std_dev: f64,
    },
    /// General full factorial (all interactions in the model); the sample
    /// size is the replicates per cell. Power for one factor's main effect.
    GeneralFactorial {
        /// Levels of the factor of interest `L ≥ 2`.
        levels: usize,
        /// Cells of the other factors together `C` (product of their levels; 1 for one factor).
        other_cells: usize,
        /// Largest difference `D` between the factor's level means.
        max_difference: f64,
        /// `σ`.
        std_dev: f64,
    },
}

/// Result of a sample-size search.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleSize {
    /// Units in the first (or only, or each) group.
    pub n: usize,
    /// Units in the second group (two-sample designs), else 0.
    pub n_second: usize,
    /// Total units.
    pub total: usize,
    /// Power achieved with these sizes.
    pub power: f64,
}

const MAX_N: usize = 50_000_000;

impl PowerDesign {
    /// Short name.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::OneSampleT { .. } => "One-sample or paired t",
            Self::TwoSampleT { .. } => "Two-sample t",
            Self::OneWayAnova { .. } => "One-way ANOVA",
            Self::OneProportion { .. } => "One proportion",
            Self::TwoProportions { .. } => "Two proportions",
            Self::OneStdDev { .. } => "One standard deviation",
            Self::TwoVariances { .. } => "Two variances",
            Self::EquivalenceOneSample { .. } => "Equivalence, one sample or paired",
            Self::EquivalenceTwoSample { .. } => "Equivalence, two samples",
            Self::OneSampleZ { .. } => "One-sample z",
            Self::PairedT { .. } => "Paired t",
            Self::OneRate { .. } => "One Poisson rate",
            Self::TwoRates { .. } => "Two Poisson rates",
            Self::MeanMargin { .. } => "Margin of error: mean",
            Self::ProportionMargin { .. } => "Margin of error: proportion",
            Self::StdDevMargin { .. } => "Margin of error: standard deviation",
            Self::RateMargin { .. } => "Margin of error: rate",
            Self::ToleranceInterval { .. } => "Tolerance interval",
            Self::DistributionFreeTolerance { .. } => "Tolerance interval, distribution-free",
            Self::TwoLevelFactorial { .. } => "Two-level factorial",
            Self::Screening { .. } => "Screening design",
            Self::GeneralFactorial { .. } => "General full factorial",
        }
    }

    /// `true` when the calculation depends on the alternative hypothesis
    /// (the t, z, proportion, variance and rate tests).
    #[must_use]
    pub fn uses_alternative(&self) -> bool {
        !matches!(
            self,
            Self::OneWayAnova { .. }
                | Self::EquivalenceOneSample { .. }
                | Self::EquivalenceTwoSample { .. }
                | Self::TwoLevelFactorial { .. }
                | Self::Screening { .. }
                | Self::GeneralFactorial { .. }
        ) && !self.is_estimation()
    }

    /// `true` for the estimation and tolerance-interval designs, whose
    /// "power" is the probability that the interval is as narrow as wanted.
    #[must_use]
    pub fn is_estimation(&self) -> bool {
        matches!(
            self,
            Self::MeanMargin { .. }
                | Self::ProportionMargin { .. }
                | Self::StdDevMargin { .. }
                | Self::RateMargin { .. }
                | Self::ToleranceInterval { .. }
                | Self::DistributionFreeTolerance { .. }
        )
    }

    /// `true` for the factorial designs, whose sample size is a number of
    /// replicates.
    #[must_use]
    pub fn is_factorial(&self) -> bool {
        matches!(self, Self::TwoLevelFactorial { .. } | Self::Screening { .. } | Self::GeneralFactorial { .. })
    }

    /// Runs per replicate, centre points and model terms (besides the
    /// intercept) of a two-level or screening design.
    fn two_level_layout(&self) -> Option<(usize, usize, usize)> {
        match *self {
            Self::TwoLevelFactorial { factors, fraction, center_points, interactions, .. } => {
                let runs = 1usize.checked_shl(factors.saturating_sub(fraction) as u32).unwrap_or(usize::MAX);
                let terms = factors + if interactions { factors * factors.saturating_sub(1) / 2 } else { 0 };
                Some((runs, center_points, terms))
            }
            Self::Screening { runs, factors, center_points, .. } => Some((runs, center_points, factors)),
            _ => None,
        }
    }

    /// Smallest meaningful `n` per group (replicates for factorial designs).
    #[must_use]
    pub fn min_n(&self) -> usize {
        match self {
            Self::OneProportion { .. }
            | Self::TwoProportions { .. }
            | Self::OneSampleZ { .. }
            | Self::OneRate { .. }
            | Self::TwoRates { .. }
            | Self::ProportionMargin { .. }
            | Self::RateMargin { .. } => 1,
            Self::DistributionFreeTolerance { removed, .. } => (*removed).max(1),
            Self::TwoLevelFactorial { .. } | Self::Screening { .. } => {
                let (runs, centre, terms) = self.two_level_layout().unwrap_or((2, 0, 1));
                if runs + centre > terms + 1 { 1 } else { 2 }
            }
            _ => 2,
        }
    }

    /// Size of the second group for `n` in the first (0 for one-sample designs).
    #[must_use]
    pub fn second_group(&self, n: usize) -> usize {
        match self {
            Self::TwoSampleT { ratio, .. }
            | Self::TwoProportions { ratio, .. }
            | Self::TwoVariances { ratio, .. }
            | Self::EquivalenceTwoSample { ratio, .. }
            | Self::TwoRates { ratio, .. } => ((ratio * n as f64).round() as usize).max(self.min_n()),
            _ => 0,
        }
    }

    /// Total units for `n` per (first) group (runs, for factorial designs).
    #[must_use]
    pub fn total(&self, n: usize) -> usize {
        match self {
            Self::OneWayAnova { groups, .. } => groups * n,
            Self::GeneralFactorial { levels, other_cells, .. } => levels * other_cells * n,
            _ => match self.two_level_layout() {
                Some((runs, centre, _)) => runs.saturating_mul(n).saturating_add(centre),
                None => n + self.second_group(n),
            },
        }
    }

    fn validate(&self, alpha: f64) -> Result<()> {
        if !(alpha > 0.0 && alpha < 0.5) {
            return Err(invalid(format!("α must lie in (0, 0.5) (got {alpha})")));
        }
        let positive = |v: f64, what: &str| {
            if v > 0.0 && v.is_finite() { Ok(()) } else { Err(invalid(format!("{what} must be positive (got {v})"))) }
        };
        let probability = |v: f64, what: &str| {
            if v > 0.0 && v < 1.0 { Ok(()) } else { Err(invalid(format!("{what} must lie in (0, 1) (got {v})"))) }
        };
        match *self {
            Self::OneSampleT { std_dev, difference } => {
                positive(std_dev, "the standard deviation")?;
                if !difference.is_finite() {
                    return Err(invalid("the difference must be finite"));
                }
            }
            Self::TwoSampleT { std_dev, ratio, difference } => {
                positive(std_dev, "the standard deviation")?;
                positive(ratio, "the allocation ratio")?;
                if !difference.is_finite() {
                    return Err(invalid("the difference must be finite"));
                }
            }
            Self::OneWayAnova { groups, max_difference, std_dev } => {
                if groups < 2 {
                    return Err(invalid("ANOVA power needs at least two groups"));
                }
                positive(std_dev, "the standard deviation")?;
                if !(max_difference >= 0.0 && max_difference.is_finite()) {
                    return Err(invalid("the largest difference must be non-negative"));
                }
            }
            Self::OneProportion { null, actual } => {
                probability(null, "the null proportion")?;
                probability(actual, "the true proportion")?;
            }
            Self::TwoProportions { first, second, ratio } => {
                probability(first, "the first proportion")?;
                probability(second, "the second proportion")?;
                positive(ratio, "the allocation ratio")?;
            }
            Self::OneStdDev { null, actual } => {
                positive(null, "the null standard deviation")?;
                positive(actual, "the true standard deviation")?;
            }
            Self::TwoVariances { variance_ratio, ratio } => {
                positive(variance_ratio, "the variance ratio")?;
                positive(ratio, "the allocation ratio")?;
            }
            Self::EquivalenceOneSample { lower, upper, std_dev, .. }
            | Self::EquivalenceTwoSample { lower, upper, std_dev, .. } => {
                positive(std_dev, "the standard deviation")?;
                if lower >= upper || lower.is_nan() || upper.is_nan() {
                    return Err(invalid("the equivalence bounds need lower < upper"));
                }
                if let Self::EquivalenceTwoSample { ratio, .. } = *self {
                    positive(ratio, "the allocation ratio")?;
                }
            }
            Self::OneSampleZ { difference, std_dev } | Self::PairedT { difference, std_dev, .. } => {
                positive(std_dev, "the standard deviation")?;
                if !difference.is_finite() {
                    return Err(invalid("the difference must be finite"));
                }
                if let Self::PairedT { correlation, .. } = *self {
                    if !(correlation > -1.0 && correlation < 1.0) {
                        return Err(invalid(format!("the correlation must lie in (−1, 1) (got {correlation})")));
                    }
                }
            }
            Self::OneRate { null, actual, exposure } => {
                positive(null, "the null rate")?;
                positive(actual, "the true rate")?;
                positive(exposure, "the exposure per unit")?;
            }
            Self::TwoRates { first, second, exposure, ratio } => {
                positive(first, "the first rate")?;
                positive(second, "the second rate")?;
                positive(exposure, "the exposure per unit")?;
                positive(ratio, "the allocation ratio")?;
            }
            Self::MeanMargin { margin, std_dev } | Self::StdDevMargin { margin, std_dev } => {
                positive(margin, "the margin of error")?;
                positive(std_dev, "the standard deviation")?;
            }
            Self::ProportionMargin { margin, proportion } => {
                positive(margin, "the margin of error")?;
                probability(proportion, "the planning proportion")?;
            }
            Self::RateMargin { margin, rate, exposure } => {
                positive(margin, "the margin of error")?;
                positive(rate, "the planning rate")?;
                positive(exposure, "the exposure per unit")?;
            }
            Self::ToleranceInterval { proportion, confidence, max_proportion, .. } => {
                probability(proportion, "the proportion covered")?;
                probability(confidence, "the confidence")?;
                if !(max_proportion > proportion && max_proportion < 1.0) {
                    return Err(invalid(format!(
                        "the largest acceptable content must lie between the proportion covered and 1 (got {max_proportion})"
                    )));
                }
            }
            Self::DistributionFreeTolerance { proportion, removed, two_sided } => {
                probability(proportion, "the proportion covered")?;
                let least = if two_sided { 2 } else { 1 };
                if removed < least {
                    return Err(invalid(format!(
                        "a {} interval leaves at least {least} order statistic{} outside it (got {removed})",
                        if two_sided { "two-sided" } else { "one-sided" },
                        if least == 1 { "" } else { "s" }
                    )));
                }
            }
            Self::TwoLevelFactorial { factors, fraction, effect, std_dev, .. } => {
                if !(1..=20).contains(&factors) {
                    return Err(invalid("a two-level factorial needs 1 to 20 factors"));
                }
                if fraction >= factors {
                    return Err(invalid("the fraction must leave at least two runs (fraction < factors)"));
                }
                positive(std_dev, "the standard deviation")?;
                if !effect.is_finite() {
                    return Err(invalid("the effect must be finite"));
                }
                let (runs, _, terms) = self.two_level_layout().unwrap_or_default();
                if terms >= runs {
                    return Err(invalid(format!(
                        "{runs} runs per replicate cannot estimate {terms} model terms; use a smaller fraction or leave out the interactions"
                    )));
                }
            }
            Self::Screening { runs, factors, effect, std_dev, .. } => {
                if runs < 4 || runs % 4 != 0 {
                    return Err(invalid(format!("screening designs have a multiple of four runs (got {runs})")));
                }
                if factors < 1 || factors >= runs {
                    return Err(invalid(format!("{runs} runs screen at most {} factors", runs - 1)));
                }
                positive(std_dev, "the standard deviation")?;
                if !effect.is_finite() {
                    return Err(invalid("the effect must be finite"));
                }
            }
            Self::GeneralFactorial { levels, other_cells, max_difference, std_dev } => {
                if levels < 2 {
                    return Err(invalid("the factor needs at least two levels"));
                }
                if other_cells < 1 {
                    return Err(invalid("the other factors have at least one cell"));
                }
                positive(std_dev, "the standard deviation")?;
                if !(max_difference >= 0.0 && max_difference.is_finite()) {
                    return Err(invalid("the largest difference must be non-negative"));
                }
            }
        }
        Ok(())
    }

    /// Power with `n` units per (first) group at level `alpha`. The
    /// alternative is ignored by ANOVA and equivalence designs.
    ///
    /// # Errors
    /// Invalid design parameters, `α ∉ (0, 0.5)`, or `n` below
    /// [`min_n`](Self::min_n).
    pub fn power(&self, n: usize, alpha: f64, alternative: Alternative) -> Result<f64> {
        self.validate(alpha)?;
        if n < self.min_n() {
            return Err(invalid(format!("the sample size must be at least {}", self.min_n())));
        }
        let nf = n as f64;
        let n2 = self.second_group(n) as f64;
        let normal = Normal::standard();
        let p = match *self {
            Self::OneSampleT { difference, std_dev } => {
                t_power(nf - 1.0, difference * nf.sqrt() / std_dev, alpha, alternative)?
            }
            Self::TwoSampleT { difference, std_dev, .. } => {
                let se = std_dev * (1.0 / nf + 1.0 / n2).sqrt();
                t_power(nf + n2 - 2.0, difference / se, alpha, alternative)?
            }
            Self::OneWayAnova { groups, max_difference, std_dev } => {
                let k = groups as f64;
                let (d1, d2) = (k - 1.0, k * (nf - 1.0));
                let crit = FisherSnedecor::new(d1, d2)?.quantile(1.0 - alpha)?;
                let lambda = nf * max_difference * max_difference / (2.0 * std_dev * std_dev);
                NoncentralF::new(d1, d2, lambda)?.sf(crit)?
            }
            Self::OneProportion { null, actual } => {
                let s0 = (null * (1.0 - null) / nf).sqrt();
                let s1 = (actual * (1.0 - actual) / nf).sqrt();
                z_power(actual - null, s0, s1, alpha, alternative, &normal)?
            }
            Self::TwoProportions { first, second, .. } => {
                let pbar = (first * nf + second * n2) / (nf + n2);
                let s0 = (pbar * (1.0 - pbar) * (1.0 / nf + 1.0 / n2)).sqrt();
                let s1 = (first * (1.0 - first) / nf + second * (1.0 - second) / n2).sqrt();
                z_power(first - second, s0, s1, alpha, alternative, &normal)?
            }
            Self::OneStdDev { null, actual } => {
                let chi = ChiSquared::new(nf - 1.0)?;
                let scale = (null / actual).powi(2);
                let upper = |a: f64| -> Result<f64> { chi.sf(chi.quantile(1.0 - a)? * scale) };
                let lower = |a: f64| -> Result<f64> { chi.cdf(chi.quantile(a)? * scale) };
                match alternative {
                    Alternative::Greater => upper(alpha)?,
                    Alternative::Less => lower(alpha)?,
                    Alternative::TwoSided => upper(alpha / 2.0)? + lower(alpha / 2.0)?,
                }
            }
            Self::TwoVariances { variance_ratio, .. } => {
                let f = FisherSnedecor::new(nf - 1.0, n2 - 1.0)?;
                let upper = |a: f64| -> Result<f64> { f.sf(f.quantile(1.0 - a)? / variance_ratio) };
                let lower = |a: f64| -> Result<f64> { f.cdf(f.quantile(a)? / variance_ratio) };
                match alternative {
                    Alternative::Greater => upper(alpha)?,
                    Alternative::Less => lower(alpha)?,
                    Alternative::TwoSided => upper(alpha / 2.0)? + lower(alpha / 2.0)?,
                }
            }
            Self::EquivalenceOneSample { lower, upper, difference, std_dev } => {
                tost_power(lower, upper, difference, std_dev / nf.sqrt(), nf - 1.0, alpha)?
            }
            Self::EquivalenceTwoSample { lower, upper, difference, std_dev, .. } => {
                let se = std_dev * (1.0 / nf + 1.0 / n2).sqrt();
                tost_power(lower, upper, difference, se, nf + n2 - 2.0, alpha)?
            }
            Self::OneSampleZ { difference, std_dev } => {
                let se = std_dev / nf.sqrt();
                z_power(difference, se, se, alpha, alternative, &normal)?
            }
            Self::PairedT { difference, std_dev, correlation } => {
                let sd = std_dev * (2.0 * (1.0 - correlation)).sqrt();
                t_power(nf - 1.0, difference * nf.sqrt() / sd, alpha, alternative)?
            }
            Self::OneRate { null, actual, exposure } => {
                let t = nf * exposure;
                z_power(actual - null, (null / t).sqrt(), (actual / t).sqrt(), alpha, alternative, &normal)?
            }
            Self::TwoRates { first, second, exposure, .. } => {
                let (t1, t2) = (nf * exposure, n2 * exposure);
                let pooled = (first * t1 + second * t2) / (t1 + t2);
                let s0 = (pooled * (1.0 / t1 + 1.0 / t2)).sqrt();
                let s1 = (first / t1 + second / t2).sqrt();
                z_power(first - second, s0, s1, alpha, alternative, &normal)?
            }
            Self::MeanMargin { margin, std_dev } => {
                let t = StudentsT::new(nf - 1.0)?.quantile(1.0 - alpha / 2.0)?;
                ChiSquared::new(nf - 1.0)?.cdf((nf - 1.0) * nf * (margin / (t * std_dev)).powi(2))?
            }
            Self::StdDevMargin { margin, std_dev } => {
                let chi = ChiSquared::new(nf - 1.0)?;
                let b = ((nf - 1.0) / chi.quantile(alpha / 2.0)?).sqrt();
                chi.cdf((nf - 1.0) * (margin / ((b - 1.0) * std_dev)).powi(2))?
            }
            Self::ProportionMargin { margin, proportion } => wilson_margin_probability(n, proportion, margin, alpha)?,
            Self::RateMargin { margin, rate, exposure } => {
                let z = normal.quantile(1.0 - alpha / 2.0)?;
                let t = nf * exposure;
                let limit = (margin * t / z).powi(2) - 0.25 * z * z;
                if limit < 0.0 { 0.0 } else { poisson_cdf(limit.floor().min(1e15) as u64, rate * t)? }
            }
            Self::ToleranceInterval { proportion, confidence, max_proportion, two_sided } => {
                let k = if two_sided {
                    two_sided_tolerance_factor(n, proportion, confidence)?
                } else {
                    one_sided_tolerance_factor(n, proportion, confidence)?
                };
                1.0 - tolerance_content_probability(n, max_proportion, k, two_sided)?
            }
            Self::DistributionFreeTolerance { proportion, removed, .. } => {
                binomial_cdf((n - removed) as u64, n as u64, proportion)?
            }
            Self::TwoLevelFactorial { effect, std_dev, .. } | Self::Screening { effect, std_dev, .. } => {
                let (runs, centre, terms) = self.two_level_layout().unwrap_or_default();
                let replicated = nf * runs as f64;
                let df_error = replicated + centre as f64 - 1.0 - terms as f64;
                if df_error < 1.0 {
                    return Err(invalid("no degrees of freedom for error: add replicates or center points"));
                }
                let crit = FisherSnedecor::new(1.0, df_error)?.quantile(1.0 - alpha)?;
                let lambda = replicated * effect * effect / (4.0 * std_dev * std_dev);
                NoncentralF::new(1.0, df_error, lambda)?.sf(crit)?
            }
            Self::GeneralFactorial { levels, other_cells, max_difference, std_dev } => {
                let (l, c) = (levels as f64, other_cells as f64);
                let (d1, d2) = (l - 1.0, l * c * (nf - 1.0));
                let crit = FisherSnedecor::new(d1, d2)?.quantile(1.0 - alpha)?;
                let lambda = nf * c * max_difference * max_difference / (2.0 * std_dev * std_dev);
                NoncentralF::new(d1, d2, lambda)?.sf(crit)?
            }
        };
        Ok(p.clamp(0.0, 1.0))
    }

    /// Smallest `n` per (first) group with power at least `target`.
    ///
    /// # Errors
    /// As [`power`](Self::power); [`stat_core::StatError::InvalidArgument`] if
    /// `target ∉ (0, 1)`, if `target ≤ α` for a test (α is the power when
    /// there is no effect, a bound every sample size meets; estimation and
    /// tolerance-interval designs have no such bound), or if the target is not
    /// reached by 5·10⁷ units (e.g. a zero effect).
    pub fn sample_size(&self, target: f64, alpha: f64, alternative: Alternative) -> Result<SampleSize> {
        if !(target > 0.0 && target < 1.0) {
            return Err(invalid(format!("the target power must lie in (0, 1) (got {target})")));
        }
        if !self.is_estimation() && target <= alpha {
            return Err(invalid(format!("the target power must exceed α = {alpha} (got {target})")));
        }
        let at = |n: usize| self.power(n, alpha, alternative);
        let mut lo = self.min_n();
        if at(lo)? >= target {
            return Ok(self.size_at(lo, at(lo)?));
        }
        let mut hi = lo.max(2) * 2;
        while at(hi)? < target {
            lo = hi;
            hi *= 2;
            if hi > MAX_N {
                return Err(invalid("the target power is not reached with any practical sample size"));
            }
        }
        // Invariant: power(lo) < target ≤ power(hi).
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if at(mid)? >= target {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        Ok(self.size_at(hi, at(hi)?))
    }

    fn size_at(&self, n: usize, power: f64) -> SampleSize {
        SampleSize { n, n_second: self.second_group(n), total: self.total(n), power }
    }

    /// The effect parameter that [`with_effect`](Self::with_effect) varies:
    /// the difference (t, ANOVA, equivalence), `p₁` (proportions), `σ₁`, or the
    /// variance ratio.
    #[must_use]
    pub fn effect(&self) -> f64 {
        match *self {
            Self::OneSampleT { difference, .. }
            | Self::TwoSampleT { difference, .. }
            | Self::EquivalenceOneSample { difference, .. }
            | Self::EquivalenceTwoSample { difference, .. } => difference,
            Self::OneWayAnova { max_difference, .. } => max_difference,
            Self::OneProportion { actual, .. } | Self::OneStdDev { actual, .. } => actual,
            Self::TwoProportions { first, .. } => first,
            Self::TwoVariances { variance_ratio, .. } => variance_ratio,
            Self::OneSampleZ { difference, .. } | Self::PairedT { difference, .. } => difference,
            Self::OneRate { actual, .. } => actual,
            Self::TwoRates { first, .. } => first,
            Self::MeanMargin { margin, .. }
            | Self::ProportionMargin { margin, .. }
            | Self::StdDevMargin { margin, .. }
            | Self::RateMargin { margin, .. } => margin,
            Self::ToleranceInterval { max_proportion, .. } => max_proportion,
            Self::DistributionFreeTolerance { proportion, .. } => proportion,
            Self::TwoLevelFactorial { effect, .. } | Self::Screening { effect, .. } => effect,
            Self::GeneralFactorial { max_difference, .. } => max_difference,
        }
    }

    /// The same design with its effect parameter set to `e`.
    #[must_use]
    pub fn with_effect(&self, e: f64) -> Self {
        let mut d = *self;
        match &mut d {
            Self::OneSampleT { difference, .. }
            | Self::TwoSampleT { difference, .. }
            | Self::EquivalenceOneSample { difference, .. }
            | Self::EquivalenceTwoSample { difference, .. } => *difference = e,
            Self::OneWayAnova { max_difference, .. } => *max_difference = e,
            Self::OneProportion { actual, .. } | Self::OneStdDev { actual, .. } => *actual = e,
            Self::TwoProportions { first, .. } => *first = e,
            Self::TwoVariances { variance_ratio, .. } => *variance_ratio = e,
            Self::OneSampleZ { difference, .. } | Self::PairedT { difference, .. } => *difference = e,
            Self::OneRate { actual, .. } => *actual = e,
            Self::TwoRates { first, .. } => *first = e,
            Self::MeanMargin { margin, .. }
            | Self::ProportionMargin { margin, .. }
            | Self::StdDevMargin { margin, .. }
            | Self::RateMargin { margin, .. } => *margin = e,
            Self::ToleranceInterval { max_proportion, .. } => *max_proportion = e,
            Self::DistributionFreeTolerance { proportion, .. } => *proportion = e,
            Self::TwoLevelFactorial { effect, .. } | Self::Screening { effect, .. } => *effect = e,
            Self::GeneralFactorial { max_difference, .. } => *max_difference = e,
        }
        d
    }

    /// The effect value at which the null hypothesis holds (where the power
    /// equals `α`; for estimation designs, a margin of zero; for tolerance
    /// intervals, the content `P`).
    #[must_use]
    pub fn null_effect(&self) -> f64 {
        match *self {
            Self::OneProportion { null, .. } | Self::OneStdDev { null, .. } | Self::OneRate { null, .. } => null,
            Self::TwoProportions { second, .. } | Self::TwoRates { second, .. } => second,
            Self::TwoVariances { .. } => 1.0,
            Self::ToleranceInterval { proportion, .. } => proportion,
            // Confidence falls to 0 as the content approaches 1.
            Self::DistributionFreeTolerance { .. } => 1.0,
            _ => 0.0,
        }
    }

    /// The effect value at which `H₀` holds (where power equals `α`), and the
    /// furthest admissible value in the direction of `alternative`.
    fn effect_range(&self, alternative: Alternative) -> Option<(f64, f64)> {
        let down = alternative == Alternative::Less;
        match *self {
            Self::OneSampleT { std_dev, .. } | Self::TwoSampleT { std_dev, .. } => {
                Some((0.0, if down { -1e4 * std_dev } else { 1e4 * std_dev }))
            }
            Self::OneWayAnova { std_dev, .. } => Some((0.0, 1e4 * std_dev)),
            Self::OneProportion { null, .. } => Some((null, if down { 1e-9 } else { 1.0 - 1e-9 })),
            Self::TwoProportions { second, .. } => Some((second, if down { 1e-9 } else { 1.0 - 1e-9 })),
            Self::OneStdDev { null, .. } => Some((null, if down { null * 1e-4 } else { null * 1e4 })),
            Self::TwoVariances { .. } => Some((1.0, if down { 1e-6 } else { 1e6 })),
            Self::EquivalenceOneSample { .. } | Self::EquivalenceTwoSample { .. } => None,
            Self::OneSampleZ { std_dev, .. } => Some((0.0, if down { -1e4 * std_dev } else { 1e4 * std_dev })),
            Self::PairedT { std_dev, correlation, .. } => {
                let sd = std_dev * (2.0 * (1.0 - correlation)).sqrt();
                Some((0.0, if down { -1e4 * sd } else { 1e4 * sd }))
            }
            Self::OneRate { null, .. } => Some((null, if down { null * 1e-4 } else { null * 1e4 })),
            Self::TwoRates { second, .. } => Some((second, if down { second * 1e-4 } else { second * 1e4 })),
            Self::MeanMargin { std_dev, .. } | Self::StdDevMargin { std_dev, .. } => Some((0.0, 1e4 * std_dev)),
            Self::ProportionMargin { .. } => Some((0.0, 1.0)),
            Self::RateMargin { rate, .. } => Some((0.0, 1e4 * rate)),
            Self::ToleranceInterval { proportion, .. } => Some((proportion, 1.0 - 1e-12)),
            // The confidence rises as the content P falls: from P near 1 to near 0.
            Self::DistributionFreeTolerance { .. } => Some((1.0 - 1e-12, 1e-9)),
            Self::TwoLevelFactorial { std_dev, .. }
            | Self::Screening { std_dev, .. }
            | Self::GeneralFactorial { std_dev, .. } => Some((0.0, 1e4 * std_dev)),
        }
    }

    /// Smallest effect (in the direction of `alternative`; upward for
    /// two-sided tests) detected with power `target` using `n` units per
    /// (first) group.
    ///
    /// # Errors
    /// As [`power`](Self::power); [`stat_core::StatError::InvalidArgument`] for
    /// equivalence designs, a target outside (power with no effect, 1) — α
    /// for a test, but e.g. 1 − γ for a tolerance interval and 0 for a
    /// margin of error — or when no admissible effect reaches the target.
    pub fn detectable_effect(&self, n: usize, target: f64, alpha: f64, alternative: Alternative) -> Result<f64> {
        if !(target > 0.0 && target < 1.0) {
            return Err(invalid(format!("the target power must lie in (0, 1) (got {target})")));
        }
        let Some((null, far)) = self.effect_range(alternative) else {
            return Err(invalid("a detectable difference is not defined for equivalence designs"));
        };
        let at = |e: f64| self.with_effect(e).power(n, alpha, alternative);
        // The design's own floor: its power where there is no effect (just
        // beside it for designs undefined exactly there, such as P′ = P).
        let mut floor = at(null).or_else(|_| at(null + (far - null) * 1e-12)).unwrap_or(alpha);
        if !self.is_estimation() {
            // A test's floor is α itself (its computed value may round below).
            floor = floor.max(alpha);
        }
        if target <= floor {
            return Err(invalid(format!(
                "the target power must exceed {}, the power when there is no effect (got {target})",
                (floor * 1e4).round() / 1e4
            )));
        }
        if at(far)? < target {
            return Err(invalid("no admissible effect reaches the target power with this sample size"));
        }
        // Bisection on a geometric/linear scale between the null and `far`.
        let ratio_scale = matches!(
            self,
            Self::OneStdDev { .. } | Self::TwoVariances { .. } | Self::OneRate { .. } | Self::TwoRates { .. }
        );
        let (mut lo, mut hi) = (null, far);
        for _ in 0..200 {
            let mid = if ratio_scale { (lo * hi).sqrt() } else { 0.5 * (lo + hi) };
            if at(mid)? >= target {
                hi = mid;
            } else {
                lo = mid;
            }
            if (hi - lo).abs() <= 1e-12 * hi.abs().max(1e-300) {
                break;
            }
        }
        Ok(hi)
    }
}

fn t_power(df: f64, ncp: f64, alpha: f64, alternative: Alternative) -> Result<f64> {
    let t = StudentsT::new(df)?;
    let d = NoncentralT::new(df, ncp)?;
    Ok(match alternative {
        Alternative::Greater => d.sf(t.quantile(1.0 - alpha)?)?,
        Alternative::Less => d.cdf(-t.quantile(1.0 - alpha)?)?,
        Alternative::TwoSided => {
            let c = t.quantile(1.0 - alpha / 2.0)?;
            d.sf(c)? + d.cdf(-c)?
        }
    })
}

fn z_power(diff: f64, s0: f64, s1: f64, alpha: f64, alternative: Alternative, normal: &Normal) -> Result<f64> {
    let upper = |a: f64| -> Result<f64> { normal.cdf((diff - normal.quantile(1.0 - a)? * s0) / s1) };
    let lower = |a: f64| -> Result<f64> { normal.cdf((-diff - normal.quantile(1.0 - a)? * s0) / s1) };
    Ok(match alternative {
        Alternative::Greater => upper(alpha)?,
        Alternative::Less => lower(alpha)?,
        Alternative::TwoSided => upper(alpha / 2.0)? + lower(alpha / 2.0)?,
    })
}

/// `P(Wilson half-width ≤ margin)` for `n` trials with success probability
/// `p`, summing the binomial probabilities within 12 standard deviations of
/// `np` (the rest is below 10⁻³⁰).
fn wilson_margin_probability(n: usize, p: f64, margin: f64, alpha: f64) -> Result<f64> {
    let z = Normal::standard().quantile(1.0 - alpha / 2.0)?;
    let nf = n as f64;
    let spread = 12.0 * (nf * p * (1.0 - p)).sqrt() + 12.0;
    let lo = (nf * p - spread).floor().max(0.0) as u64;
    let hi = (nf * p + spread).ceil().min(nf) as u64;
    let mut total = 0.0;
    for x in lo..=hi {
        let ph = x as f64 / nf;
        let half = z / (1.0 + z * z / nf) * (ph * (1.0 - ph) / nf + z * z / (4.0 * nf * nf)).sqrt();
        if half <= margin {
            total += binomial_pmf(x, n as u64, p)?;
        }
    }
    Ok(total.min(1.0))
}

/// Exact TOST power by integrating over the sampling distribution of the
/// standard error (substituting `v = w²` to remove the χ² density's
/// singularity at zero).
fn tost_power(lower: f64, upper: f64, delta: f64, se: f64, df: f64, alpha: f64) -> Result<f64> {
    let c = StudentsT::new(df)?.quantile(1.0 - alpha)?;
    let normal = Normal::standard();
    let v_max = df * ((upper - lower) / (2.0 * c * se)).powi(2);
    let chi = ChiSquared::new(df)?;
    let w_max = v_max.sqrt().min(chi.quantile(1.0 - 1e-15)?.sqrt());
    let rule = CompositeRule::new(20, 24);
    let mut failure = None;
    let value = rule.integrate(0.0, w_max, |w| {
        let v = w * w;
        let s_hat = se * (v / df).sqrt();
        let hi = (upper - c * s_hat - delta) / se;
        let lo = (lower + c * s_hat - delta) / se;
        if hi <= lo {
            return 0.0;
        }
        match (normal.cdf(hi), normal.cdf(lo)) {
            (Ok(a), Ok(b)) => (a - b) * chi.pdf(v) * 2.0 * w,
            (Err(e), _) | (_, Err(e)) => {
                failure = Some(e);
                0.0
            }
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    fn t1(d: f64) -> PowerDesign {
        PowerDesign::OneSampleT { difference: d, std_dev: 1.0 }
    }

    /// Cohen's (1988) tables: for a medium effect (d = 0.5) at two-sided
    /// α = 0.05 and power 0.80, 64 per group for two samples and 34 for one
    /// sample; for f = 0.25 with four groups, 45 per group. Power values from
    /// SciPy 1.18 `nct`/`ncf`.
    #[test]
    fn t_and_anova_sample_sizes() {
        let two = PowerDesign::TwoSampleT { difference: 0.5, std_dev: 1.0, ratio: 1.0 };
        let s = two.sample_size(0.8, 0.05, Alternative::TwoSided).unwrap();
        assert_eq!((s.n, s.n_second, s.total), (64, 64, 128));
        close(s.power, 0.801_459_557_922_254_2, 1e-9);
        let one = t1(0.5).sample_size(0.8, 0.05, Alternative::TwoSided).unwrap();
        assert_eq!(one.n, 34);
        close(one.power, 0.807_777_501_279_273_8, 1e-9);
        close(t1(0.5).power(20, 0.05, Alternative::Greater).unwrap(), 0.695_149_338_244_341_2, 1e-9);
        close(t1(-0.5).power(20, 0.05, Alternative::Less).unwrap(), 0.695_149_338_244_341_2, 1e-9);
        // f = 0.25 with k = 4: the D giving Σ(μ−μ̄)²/k = f²σ² is D = f√(2k)σ.
        let d = 0.25 * 8f64.sqrt();
        let anova = PowerDesign::OneWayAnova { groups: 4, max_difference: d, std_dev: 1.0 };
        let a = anova.sample_size(0.8, 0.05, Alternative::TwoSided).unwrap();
        assert_eq!((a.n, a.total), (45, 180));
        close(a.power, 0.803_986_912_865_175_8, 1e-8);
    }

    /// Normal-approximation formulas evaluated with SciPy 1.18.
    #[test]
    fn proportion_and_variance_power() {
        let two = PowerDesign::TwoProportions { first: 0.6, second: 0.5, ratio: 1.0 };
        let s = two.sample_size(0.8, 0.05, Alternative::TwoSided).unwrap();
        assert_eq!(s.n, 388);
        close(two.power(388, 0.05, Alternative::TwoSided).unwrap(), 0.800_672_010_678_347_7, 1e-9);
        let one = PowerDesign::OneProportion { null: 0.5, actual: 0.6 };
        close(one.power(100, 0.05, Alternative::TwoSided).unwrap(), 0.516_323_421_027_341_6, 1e-9);
        let sd = PowerDesign::OneStdDev { null: 1.0, actual: 1.5 };
        close(sd.power(20, 0.05, Alternative::Greater).unwrap(), 0.817_635_125_498_507_5, 1e-9);
        let vr = PowerDesign::TwoVariances { variance_ratio: 3.0, ratio: 1.0 };
        close(vr.power(15, 0.05, Alternative::TwoSided).unwrap(), 0.505_354_974_397_994_4, 1e-9);
    }

    /// TOST power by direct numerical integration (SciPy 1.18 `quad`), and a
    /// Monte-Carlo check of 10⁶ simulated trials (0.7811 ± 0.0004).
    #[test]
    fn equivalence_power() {
        let e = PowerDesign::EquivalenceOneSample { lower: -0.5, upper: 0.5, difference: 0.1, std_dev: 1.0 };
        close(e.power(40, 0.05, Alternative::TwoSided).unwrap(), 0.781_106_501_814_946_3, 1e-8);
        let e2 =
            PowerDesign::EquivalenceTwoSample { lower: -0.5, upper: 0.5, difference: 0.0, std_dev: 1.0, ratio: 1.0 };
        close(e2.power(70, 0.05, Alternative::TwoSided).unwrap(), 0.805_931_181_627_329_1, 1e-8);
        assert!(e.detectable_effect(40, 0.8, 0.05, Alternative::TwoSided).is_err());
        // Power grows with the sample size; with very few units only a lucky
        // small standard deviation lets the interval fit inside the bounds.
        let small = e.power(3, 0.05, Alternative::TwoSided).unwrap();
        assert!(small < e.power(10, 0.05, Alternative::TwoSided).unwrap() && small < 0.1, "{small}");
        let n = e.sample_size(0.9, 0.05, Alternative::TwoSided).unwrap();
        assert!(n.power >= 0.9 && e.power(n.n - 1, 0.05, Alternative::TwoSided).unwrap() < 0.9);
    }

    #[test]
    fn detectable_difference_inverts_power() {
        let d = t1(1.0).detectable_effect(34, 0.8, 0.05, Alternative::TwoSided).unwrap();
        close(t1(d).power(34, 0.05, Alternative::TwoSided).unwrap(), 0.8, 1e-9);
        assert!(d < 0.5 && d > 0.48, "{d}");
        let p = PowerDesign::OneProportion { null: 0.5, actual: 0.6 };
        let e = p.detectable_effect(200, 0.9, 0.05, Alternative::Less).unwrap();
        assert!(e < 0.5);
        close(p.with_effect(e).power(200, 0.05, Alternative::Less).unwrap(), 0.9, 1e-9);
        let v = PowerDesign::OneStdDev { null: 2.0, actual: 3.0 };
        let s = v.detectable_effect(25, 0.8, 0.05, Alternative::Greater).unwrap();
        close(v.with_effect(s).power(25, 0.05, Alternative::Greater).unwrap(), 0.8, 1e-9);
        // At the null value the power equals α.
        close(t1(0.0).power(30, 0.05, Alternative::TwoSided).unwrap(), 0.05, 1e-10);
        assert!(t1(0.0).sample_size(0.8, 0.05, Alternative::TwoSided).is_err());
    }

    /// z, paired t and Poisson-rate power from the closed forms, evaluated
    /// independently with SciPy 1.18 (`norm`, `nct`). The z sample size for
    /// δ/σ = ½ at two-sided α = 0.05 and power 0.8 is the textbook
    /// `⌈((z₀.₉₇₅ + z₀.₈)σ/δ)²⌉ = ⌈31.4⌉ = 32`.
    #[test]
    fn z_paired_and_rate_power() {
        let z = PowerDesign::OneSampleZ { difference: 0.5, std_dev: 1.0 };
        let s = z.sample_size(0.8, 0.05, Alternative::TwoSided).unwrap();
        assert_eq!(s.n, 32);
        close(s.power, 0.807_430_419_432_557_2, 1e-10);
        close(z.power(31, 0.05, Alternative::TwoSided).unwrap(), 0.795_008_028_401_812_1, 1e-10);
        let paired = PowerDesign::PairedT { difference: 1.0, std_dev: 2.0, correlation: 0.6 };
        close(paired.power(20, 0.05, Alternative::TwoSided).unwrap(), 0.659_949_876_782_130_1, 1e-8);
        // The same as a one-sample t on differences with σ_d = σ√(2(1 − ρ)).
        let direct = PowerDesign::OneSampleT { difference: 1.0, std_dev: 2.0 * 0.8f64.sqrt() };
        close(
            paired.power(20, 0.05, Alternative::Less).unwrap(),
            direct.power(20, 0.05, Alternative::Less).unwrap(),
            1e-12,
        );
        let one = PowerDesign::OneRate { null: 2.0, actual: 2.5, exposure: 1.5 };
        close(one.power(30, 0.05, Alternative::Greater).unwrap(), 0.742_192_171_053_949, 1e-10);
        let two = PowerDesign::TwoRates { first: 3.0, second: 2.0, exposure: 2.0, ratio: 1.0 };
        close(two.power(25, 0.05, Alternative::TwoSided).unwrap(), 0.885_379_140_762_351_4, 1e-10);
        assert_eq!(two.second_group(25), 25);
        let e = one.detectable_effect(30, 0.9, 0.05, Alternative::Greater).unwrap();
        close(one.with_effect(e).power(30, 0.05, Alternative::Greater).unwrap(), 0.9, 1e-9);
        assert!(
            PowerDesign::PairedT { difference: 1.0, std_dev: 1.0, correlation: 1.0 }
                .power(10, 0.05, Alternative::TwoSided)
                .is_err()
        );
    }

    /// Probability that the interval's margin is at most E, from the closed
    /// forms and an exact binomial sum, evaluated independently in Python
    /// (SciPy 1.18); the mean case was also simulated (2·10⁵ samples of 60:
    /// 0.3858 ± 0.0011).
    #[test]
    fn margin_of_error_probabilities() {
        let mean = PowerDesign::MeanMargin { margin: 0.5, std_dev: 2.0 };
        close(mean.power(60, 0.05, Alternative::TwoSided).unwrap(), 0.385_777_059_246_570_85, 1e-9);
        let sd = PowerDesign::StdDevMargin { margin: 0.2, std_dev: 1.0 };
        close(sd.power(50, 0.05, Alternative::TwoSided).unwrap(), 0.031_990_185_789_503_91, 1e-8);
        let prop = PowerDesign::ProportionMargin { margin: 0.05, proportion: 0.3 };
        close(prop.power(350, 0.05, Alternative::TwoSided).unwrap(), 0.990_830_261_446_206_2, 1e-10);
        let rate = PowerDesign::RateMargin { margin: 0.5, rate: 4.0, exposure: 1.0 };
        close(rate.power(70, 0.05, Alternative::TwoSided).unwrap(), 0.986_216_172_460_021_7, 1e-10);
        // Sample size for a margin: power rises with n, and the answer is minimal.
        let s = mean.sample_size(0.9, 0.05, Alternative::TwoSided).unwrap();
        assert!(s.power >= 0.9 && mean.power(s.n - 1, 0.05, Alternative::TwoSided).unwrap() < 0.9);
        // The smallest margin reachable with probability 0.5 at n = 60 is about
        // the usual t·σ/√n.
        let m = mean.detectable_effect(60, 0.5, 0.05, Alternative::TwoSided).unwrap();
        assert!((m - 2.0 * 2.0 / 60f64.sqrt()).abs() < 0.03, "{m}");
        assert!(mean.is_estimation() && !mean.uses_alternative());
    }

    /// Tolerance intervals: the probability that the content stays below
    /// P′. One-sided from SciPy's noncentral t; two-sided from an independent
    /// adaptive-quadrature evaluation of Wald & Wolfowitz's integral (and a
    /// simulation, 0.7478 ± 0.001). At P′ = P it equals 1 − γ.
    #[test]
    fn tolerance_interval_sample_size() {
        let one = PowerDesign::ToleranceInterval {
            proportion: 0.9,
            confidence: 0.95,
            max_proportion: 0.97,
            two_sided: false,
        };
        close(one.power(40, 0.05, Alternative::TwoSided).unwrap(), 0.784_087_524_590_770_4, 1e-8);
        let two =
            PowerDesign::ToleranceInterval { proportion: 0.9, confidence: 0.95, max_proportion: 0.97, two_sided: true };
        close(two.power(40, 0.05, Alternative::TwoSided).unwrap(), 0.747_143_116_670_330_4, 1e-7);
        close(two.with_effect(0.9 + 1e-12).power(40, 0.05, Alternative::TwoSided).unwrap(), 0.05, 1e-6);
        let s = two.sample_size(0.9, 0.05, Alternative::TwoSided).unwrap();
        assert!(s.power >= 0.9 && two.power(s.n - 1, 0.05, Alternative::TwoSided).unwrap() < 0.9, "{s:?}");
        assert!(
            PowerDesign::ToleranceInterval { proportion: 0.9, confidence: 0.95, max_proportion: 0.8, two_sided: true }
                .power(40, 0.05, Alternative::TwoSided)
                .is_err()
        );
    }

    /// Distribution-free tolerance intervals: sample sizes and confidences
    /// from the exact binomial sum evaluated in rational arithmetic (Python
    /// `fractions`); 59, 93, 299 and 473 are the classic Wilks values.
    #[test]
    fn distribution_free_tolerance_sample_size() {
        let df =
            |proportion, removed, two_sided| PowerDesign::DistributionFreeTolerance { proportion, removed, two_sided };
        let a = Alternative::TwoSided;
        for (p, gamma, m, two, n, achieved) in [
            (0.95, 0.95, 1, false, 59, 0.951_505_474_750_576_8),
            (0.95, 0.95, 2, true, 93, 0.950_024_204_757_383_5),
            (0.99, 0.95, 1, false, 299, 0.950_463_743_362_337_5),
            (0.99, 0.95, 2, true, 473, 0.950_202_461_180_151),
            (0.9, 0.9, 2, true, 38, 0.904_704_869_924_907_3),
            (0.95, 0.99, 4, true, 198, 0.990_242_894_377_535_9),
        ] {
            let s = df(p, m, two).sample_size(gamma, 0.05, a).unwrap();
            assert_eq!(s.n, n, "P = {p}, γ = {gamma}, m = {m}");
            close(s.power, achieved, 1e-10);
        }
        // Closed forms of the extremes: 1 − Pⁿ and 1 − nPⁿ⁻¹ + (n − 1)Pⁿ.
        close(df(0.9, 1, false).power(20, 0.05, a).unwrap(), 1.0 - 0.9f64.powi(20), 1e-12);
        let two = 1.0 - 20.0 * 0.9f64.powi(19) + 19.0 * 0.9f64.powi(20);
        close(df(0.9, 2, true).power(20, 0.05, a).unwrap(), two, 1e-12);
        close(df(0.9, 3, true).power(50, 0.05, a).unwrap(), 0.888_271_243_653_652_8, 1e-10);
        // The largest content covered with 95% confidence by 93 values is 0.95.
        let p = df(0.5, 2, true).detectable_effect(93, 0.95, 0.05, a).unwrap();
        assert!((0.95..0.9502).contains(&p), "{p}");
        assert!(df(0.9, 1, true).power(20, 0.05, a).is_err(), "two limits need m ≥ 2");
        assert!(df(0.9, 3, true).power(2, 0.05, a).is_err(), "n below m");
        assert!(df(0.9, 2, true).is_estimation() && !df(0.9, 2, true).uses_alternative());
    }

    /// Factorial designs: noncentral F power evaluated with SciPy 1.18
    /// (`ncf`, `f`).
    #[test]
    fn factorial_power() {
        let full = PowerDesign::TwoLevelFactorial {
            factors: 3,
            fraction: 0,
            center_points: 0,
            interactions: false,
            effect: 1.0,
            std_dev: 1.0,
        };
        close(full.power(2, 0.05, Alternative::TwoSided).unwrap(), 0.452_193_700_935_480_9, 1e-9);
        assert_eq!(full.total(2), 16);
        assert_eq!(full.min_n(), 1);
        let half = PowerDesign::TwoLevelFactorial {
            factors: 5,
            fraction: 1,
            center_points: 3,
            interactions: true,
            effect: 1.5,
            std_dev: 1.0,
        };
        close(half.power(1, 0.05, Alternative::TwoSided).unwrap(), 0.532_792_607_261_341_7, 1e-9);
        assert_eq!(half.total(1), 19);
        let no_error = PowerDesign::TwoLevelFactorial {
            factors: 5,
            fraction: 1,
            center_points: 0,
            interactions: true,
            effect: 1.5,
            std_dev: 1.0,
        };
        assert_eq!(no_error.min_n(), 2);
        assert!(no_error.power(1, 0.05, Alternative::TwoSided).is_err());
        let pb = PowerDesign::Screening { runs: 12, factors: 7, center_points: 0, effect: 1.2, std_dev: 1.5 };
        close(pb.power(2, 0.05, Alternative::TwoSided).unwrap(), 0.453_023_293_656_538_43, 1e-9);
        assert!(
            PowerDesign::Screening { runs: 10, factors: 7, center_points: 0, effect: 1.0, std_dev: 1.0 }
                .power(2, 0.05, Alternative::TwoSided)
                .is_err()
        );
        let general = PowerDesign::GeneralFactorial { levels: 3, other_cells: 4, max_difference: 2.0, std_dev: 1.5 };
        close(general.power(3, 0.05, Alternative::TwoSided).unwrap(), 0.789_085_826_672_608_4, 1e-9);
        assert_eq!(general.total(3), 36);
        let s = general.sample_size(0.9, 0.05, Alternative::TwoSided).unwrap();
        assert!(s.power >= 0.9 && general.power(s.n - 1, 0.05, Alternative::TwoSided).unwrap() < 0.9);
        let d = full.detectable_effect(3, 0.8, 0.05, Alternative::TwoSided).unwrap();
        close(full.with_effect(d).power(3, 0.05, Alternative::TwoSided).unwrap(), 0.8, 1e-9);
        assert!(full.is_factorial() && !full.uses_alternative());
    }

    /// A target power between α and a design's own floor: a tolerance
    /// interval with γ = 0.99 has "power" 1 − γ = 0.01 at P′ = P, so a target
    /// of 0.05 (= α, refused before) is reachable; at or below the floor it is
    /// refused. The answer inverts the power.
    #[test]
    fn detectable_effect_uses_each_designs_floor() {
        let tol = PowerDesign::ToleranceInterval {
            proportion: 0.9,
            confidence: 0.99,
            max_proportion: 0.97,
            two_sided: false,
        };
        let e = tol.detectable_effect(40, 0.05, 0.05, Alternative::TwoSided).unwrap();
        close(tol.with_effect(e).power(40, 0.05, Alternative::TwoSided).unwrap(), 0.05, 1e-6);
        assert!(tol.detectable_effect(40, 0.005, 0.05, Alternative::TwoSided).is_err());
        // A margin of error has power 0 at margin 0: any positive target works.
        let mean = PowerDesign::MeanMargin { margin: 0.5, std_dev: 2.0 };
        assert!(mean.detectable_effect(60, 0.03, 0.05, Alternative::TwoSided).is_ok());
        // A test still needs more than α.
        assert!(t1(1.0).detectable_effect(34, 0.05, 0.05, Alternative::TwoSided).is_err());
    }

    /// `sample_size` refuses a test's target at or below α (every n meets it)
    /// instead of answering with the smallest n.
    #[test]
    fn sample_size_target_must_exceed_alpha_for_tests() {
        assert!(t1(1.0).sample_size(0.05, 0.05, Alternative::TwoSided).is_err());
        assert!(t1(1.0).sample_size(0.01, 0.05, Alternative::TwoSided).is_err());
        let tol = PowerDesign::ToleranceInterval {
            proportion: 0.9,
            confidence: 0.99,
            max_proportion: 0.97,
            two_sided: false,
        };
        assert!(tol.sample_size(0.05, 0.05, Alternative::TwoSided).is_ok());
    }
}
