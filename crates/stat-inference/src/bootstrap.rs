//! The nonparametric bootstrap (Efron 1979; Efron & Tibshirani, *An
//! Introduction to the Bootstrap*, 1993): resampling observations with
//! replacement, the jackknife, and bootstrap standard errors, bias and
//! confidence intervals.
//!
//! * [`bootstrap_map`] evaluates any statistic on `B` resamples of the row
//!   positions `0..n` (optionally within strata), in parallel. Replicate `b`
//!   draws from its own generator seeded by [`replicate_seed`]`(seed, b)`, so
//!   the replicates are identical whatever the number of threads.
//! * [`jackknife_map`] evaluates a statistic on the `n` leave-one-out samples.
//! * [`summarize`] turns the replicate values (and jackknife values) into the
//!   bootstrap mean, bias `θ̄* − θ̂`, standard error (the standard deviation of
//!   the replicates, divisor `B − 1`; Efron & Tibshirani eq. 6.6), the
//!   percentile interval (ch. 13) and the bias-corrected and accelerated (BCa)
//!   interval (Efron 1987, *JASA* 82; Efron & Tibshirani ch. 14).
//!
//! # Interval formulas
//! With the `B` replicates `θ̂*₍b₎`, the percentile interval at level `1 − 2α`
//! is `(Ĝ⁻¹(α), Ĝ⁻¹(1 − α))`, `Ĝ⁻¹` the sample quantile of the replicates
//! by linear interpolation between order statistics (Hyndman & Fan type 7).
//!
//! BCa replaces `α` and `1 − α` by
//! `α₁ = Φ(ẑ₀ + (ẑ₀ + z_α) / (1 − â (ẑ₀ + z_α)))` and
//! `α₂ = Φ(ẑ₀ + (ẑ₀ + z_{1−α}) / (1 − â (ẑ₀ + z_{1−α})))`, where the bias
//! correction is `ẑ₀ = Φ⁻¹(p̂)`, `p̂` the share of replicates below `θ̂`
//! (replicates equal to `θ̂` count one half; Efron & Tibshirani eq. 14.14 for
//! continuous replicates), and the acceleration is
//! `â = Σᵢ (θ̄₍.₎ − θ̂₍ᵢ₎)³ / (6 [Σᵢ (θ̄₍.₎ − θ̂₍ᵢ₎)²]^{3/2})` (eq. 14.15) from the
//! jackknife values `θ̂₍ᵢ₎`. With strata (independent samples), the
//! jackknife deletes one observation at a time within each stratum `h` of
//! size `n_h`, and with `U_hi = (n_h − 1)(θ̄_h − θ̂₍hi₎)`,
//! `â = Σ_h Σᵢ U_hi³ / n_h³ / (6 [Σ_h Σᵢ U_hi² / n_h²]^{3/2})` (Efron &
//! Tibshirani eq. 15.36).

use rayon::prelude::*;
use stat_core::descriptive::{QuantileMethod, quantile_sorted};
use stat_core::rng::Rng;
use stat_core::{ContinuousDistribution, Normal, Result, StatError};

use crate::common::{check_level, invalid};

/// The seed of replicate `replicate`'s generator: the SplitMix64 finalizer
/// applied to `seed` mixed with the replicate number, so neighbouring
/// replicates draw unrelated streams (seeding consecutive SplitMix64 states
/// directly would give shifted copies of one stream).
#[must_use]
pub fn replicate_seed(seed: u64, replicate: usize) -> u64 {
    let mix = |mut z: u64| {
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    mix(seed ^ mix((replicate as u64).wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15)))
}

/// Members of each stratum, from per-observation stratum codes.
fn stratum_members(n: usize, strata: Option<&[usize]>) -> Result<Vec<Vec<usize>>> {
    match strata {
        None => Ok(vec![(0..n).collect()]),
        Some(codes) => {
            if codes.len() != n {
                return Err(StatError::DimensionMismatch {
                    context: "stratum codes vs. observations",
                    expected: n,
                    found: codes.len(),
                });
            }
            let k = codes.iter().max().map_or(0, |m| m + 1);
            let mut members = vec![Vec::new(); k];
            for (i, &c) in codes.iter().enumerate() {
                members[c].push(i);
            }
            members.retain(|m| !m.is_empty());
            Ok(members)
        }
    }
}

/// One bootstrap resample of the positions `0..n`: position `i` is replaced
/// by a position drawn uniformly, with replacement, from the members of its
/// own stratum (from all of `0..n` without strata), so every stratum keeps
/// its size.
fn resample(members: &[Vec<usize>], stratum_of: &[usize], rng: &mut Rng) -> Vec<usize> {
    stratum_of
        .iter()
        .map(|&h| {
            let m = &members[h];
            m[rng.below(m.len())]
        })
        .collect()
}

/// Stratum index (into the member lists) of each position.
fn stratum_index(n: usize, members: &[Vec<usize>]) -> Vec<usize> {
    let mut of = vec![0; n];
    for (h, m) in members.iter().enumerate() {
        for &i in m {
            of[i] = h;
        }
    }
    of
}

/// The positions of bootstrap replicate `replicate` of `n` observations
/// (see [`bootstrap_map`] for the resampling scheme), as `bootstrap_map`
/// passes them to its statistic.
///
/// # Errors
/// As for [`bootstrap_map`].
pub fn bootstrap_sample(n: usize, strata: Option<&[usize]>, seed: u64, replicate: usize) -> Result<Vec<usize>> {
    let members = stratum_members(n, strata)?;
    let of = stratum_index(n, &members);
    Ok(resample(&members, &of, &mut Rng::new(replicate_seed(seed, replicate))))
}

/// Evaluate `statistic` on `replicates` bootstrap resamples of the
/// positions `0..n`, in parallel.
///
/// Each resample has `n` positions drawn with replacement; with `strata`
/// (one code per position) every position is replaced by one drawn from its
/// own stratum, so the stratum sizes are those of the data (the stratified
/// bootstrap of independent samples; Efron & Tibshirani §8.3). Replicate `b`
/// uses a generator seeded with [`replicate_seed`]`(seed, b)`, so results do
/// not depend on the number of threads. The results are in replicate order.
///
/// # Errors
/// [`StatError::InsufficientData`] when `n == 0` or `replicates == 0`;
/// [`StatError::DimensionMismatch`] when `strata` does not have `n` codes.
pub fn bootstrap_map<T, F>(
    n: usize,
    strata: Option<&[usize]>,
    replicates: usize,
    seed: u64,
    statistic: F,
) -> Result<Vec<T>>
where
    T: Send,
    F: Fn(&[usize]) -> T + Sync,
{
    if n == 0 || replicates == 0 {
        return Err(StatError::InsufficientData { required: 1, found: 0 });
    }
    let members = stratum_members(n, strata)?;
    let of = stratum_index(n, &members);
    let interrupt = stat_core::interrupt::current();
    if let Some(i) = &interrupt {
        i.begin(replicates);
    }
    (0..replicates)
        .into_par_iter()
        .map(|b| {
            let mut rng = Rng::new(replicate_seed(seed, b));
            let sample = resample(&members, &of, &mut rng);
            with_interrupt(interrupt.as_ref(), || statistic(&sample))
        })
        .collect()
}

/// Run `f` for one replicate on a rayon thread: stop with
/// [`StatError::Cancelled`] once the caller's interrupt is cancelled, count
/// the replicate, and give `f` the interrupt so routines it calls can stop
/// too (see [`stat_core::interrupt`]).
fn with_interrupt<T>(interrupt: Option<&std::sync::Arc<stat_core::interrupt::Interrupt>>, f: impl FnOnce() -> T) -> Result<T> {
    match interrupt {
        None => Ok(f()),
        Some(i) => {
            i.check()?;
            let out = stat_core::interrupt::with(std::sync::Arc::clone(i), f);
            i.step();
            Ok(out)
        }
    }
}

/// Evaluate `statistic` on the `n` leave-one-out samples of `0..n` (sample
/// `i` holds every position but `i`, in order), in parallel. Results are in
/// the order of the deleted position.
///
/// # Errors
/// [`StatError::InsufficientData`] when `n < 2`.
pub fn jackknife_map<T, F>(n: usize, statistic: F) -> Result<Vec<T>>
where
    T: Send,
    F: Fn(&[usize]) -> T + Sync,
{
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    let interrupt = stat_core::interrupt::current();
    if let Some(i) = &interrupt {
        i.begin(n);
    }
    (0..n)
        .into_par_iter()
        .map(|i| {
            let sample: Vec<usize> = (0..n).filter(|&j| j != i).collect();
            with_interrupt(interrupt.as_ref(), || statistic(&sample))
        })
        .collect()
}

/// The BCa acceleration `â` from jackknife values (see the module notes):
/// `jackknife[i]` is the statistic without observation `i`, and `strata`
/// (one code per observation, or `None` for one sample) gives the samples
/// the jackknife deletes within.
///
/// Returns `None` when there are fewer than two values in every stratum or
/// the jackknife values do not vary (the acceleration is undefined), or a
/// value is not finite.
///
/// # Errors
/// [`StatError::DimensionMismatch`] when `strata` does not have one code per value.
pub fn acceleration(jackknife: &[f64], strata: Option<&[usize]>) -> Result<Option<f64>> {
    let members = stratum_members(jackknife.len(), strata)?;
    if jackknife.iter().any(|v| !v.is_finite()) {
        return Ok(None);
    }
    let (mut num, mut den) = (0.0, 0.0);
    for m in members.iter().filter(|m| m.len() >= 2) {
        let nh = m.len() as f64;
        let mean = m.iter().map(|&i| jackknife[i]).sum::<f64>() / nh;
        let (mut s3, mut s2) = (0.0, 0.0);
        for &i in m {
            let u = (nh - 1.0) * (mean - jackknife[i]);
            s3 += u * u * u;
            s2 += u * u;
        }
        num += s3 / nh.powi(3);
        den += s2 / nh.powi(2);
    }
    if den <= 0.0 || !den.is_finite() {
        return Ok(None);
    }
    Ok(Some(num / (6.0 * den.powf(1.5))))
}

/// The BCa interval with the quantities it is built from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BcaInterval {
    /// Lower limit.
    pub lower: f64,
    /// Upper limit.
    pub upper: f64,
    /// Bias correction `ẑ₀`.
    pub bias_correction: f64,
    /// Acceleration `â`.
    pub acceleration: f64,
    /// Adjusted lower percentile `α₁` (a fraction).
    pub lower_percentile: f64,
    /// Adjusted upper percentile `α₂` (a fraction).
    pub upper_percentile: f64,
}

/// Bootstrap summary of one statistic.
#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapSummary {
    /// The statistic on the original data, `θ̂`.
    pub estimate: f64,
    /// Number of replicates with a finite value (used below).
    pub replicates: usize,
    /// Number of replicates without a finite value (the statistic could not
    /// be computed on that resample); they are left out, and counted here.
    pub failed: usize,
    /// Mean of the replicates `θ̄*`.
    pub mean: f64,
    /// Bias estimate `θ̄* − θ̂` (Efron & Tibshirani eq. 10.2).
    pub bias: f64,
    /// Bootstrap standard error: standard deviation of the replicates with
    /// divisor `B − 1` (eq. 6.6).
    pub std_error: f64,
    /// Percentile interval `(lower, upper)`.
    pub percentile: (f64, f64),
    /// BCa interval, when the jackknife was given and both `ẑ₀` and `â` are
    /// defined (`None` otherwise, e.g. when every replicate lies on one side
    /// of `θ̂`).
    pub bca: Option<BcaInterval>,
    /// Two-sided confidence level of the intervals.
    pub level: f64,
}

/// Linear-interpolation (type 7) quantile of sorted values.
fn sorted_quantile(sorted: &[f64], p: f64) -> Result<f64> {
    quantile_sorted(sorted, p.clamp(0.0, 1.0), QuantileMethod::Linear)
}

/// The percentile interval of `replicates` at two-sided level `level`
/// (non-finite replicates are ignored).
///
/// # Errors
/// [`StatError::InvalidArgument`] for a level outside (0, 1);
/// [`StatError::InsufficientData`] for fewer than two finite replicates.
pub fn percentile_interval(replicates: &[f64], level: f64) -> Result<(f64, f64)> {
    check_level(level)?;
    let sorted = sorted_finite(replicates)?;
    let alpha = (1.0 - level) / 2.0;
    Ok((sorted_quantile(&sorted, alpha)?, sorted_quantile(&sorted, 1.0 - alpha)?))
}

fn sorted_finite(replicates: &[f64]) -> Result<Vec<f64>> {
    let mut sorted: Vec<f64> = replicates.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: sorted.len() });
    }
    sorted.sort_by(f64::total_cmp);
    Ok(sorted)
}

/// The BCa interval of `replicates` (non-finite ones ignored) around the
/// estimate `estimate`, with acceleration `acceleration` (from
/// [`acceleration`]), at two-sided level `level`. `None` when `ẑ₀` is
/// infinite (every replicate on one side of the estimate) or an adjusted
/// percentile is undefined.
///
/// # Errors
/// As for [`percentile_interval`].
pub fn bca_interval(estimate: f64, replicates: &[f64], acceleration: f64, level: f64) -> Result<Option<BcaInterval>> {
    check_level(level)?;
    let sorted = sorted_finite(replicates)?;
    let b = sorted.len() as f64;
    let below = sorted.iter().filter(|&&v| v < estimate).count() as f64;
    let at_or_below = sorted.iter().filter(|&&v| v <= estimate).count() as f64;
    let share = (below + at_or_below) / (2.0 * b);
    if !(share > 0.0 && share < 1.0) || !acceleration.is_finite() || !estimate.is_finite() {
        return Ok(None);
    }
    let normal = Normal::standard();
    let z0 = normal.quantile(share)?;
    let alpha = (1.0 - level) / 2.0;
    let z_alpha = normal.quantile(alpha)?;
    let adjusted = |z: f64| -> Result<f64> {
        let num = z0 + z;
        let den = 1.0 - acceleration * num;
        if den <= 0.0 {
            return Err(invalid("BCa: the acceleration is too large for this level"));
        }
        normal.cdf(z0 + num / den)
    };
    let (Ok(a1), Ok(a2)) = (adjusted(z_alpha), adjusted(-z_alpha)) else {
        return Ok(None);
    };
    Ok(Some(BcaInterval {
        lower: sorted_quantile(&sorted, a1)?,
        upper: sorted_quantile(&sorted, a2)?,
        bias_correction: z0,
        acceleration,
        lower_percentile: a1,
        upper_percentile: a2,
    }))
}

/// Summarize the bootstrap replicates of a statistic.
///
/// `estimate` is the statistic on the original data, `replicates` its value
/// on each resample (non-finite values mark failed replicates; they are
/// counted and left out), `jackknife` its leave-one-out values for the BCa
/// acceleration (`None`: no BCa interval), `strata` the stratum codes the
/// jackknife deletes within (as given to [`bootstrap_map`]), `level` the
/// two-sided confidence level.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a level outside (0, 1) or a
/// non-finite estimate; [`StatError::InsufficientData`] for fewer than two
/// finite replicates; [`StatError::DimensionMismatch`] for strata that do not
/// match the jackknife values.
pub fn summarize(
    estimate: f64,
    replicates: &[f64],
    jackknife: Option<&[f64]>,
    strata: Option<&[usize]>,
    level: f64,
) -> Result<BootstrapSummary> {
    check_level(level)?;
    if !estimate.is_finite() {
        return Err(invalid("the statistic is not finite on the original data"));
    }
    let valid: Vec<f64> = replicates.iter().copied().filter(|v| v.is_finite()).collect();
    if valid.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: valid.len() });
    }
    let b = valid.len() as f64;
    let mean = valid.iter().sum::<f64>() / b;
    let var = valid.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (b - 1.0);
    let percentile = percentile_interval(&valid, level)?;
    let bca = match jackknife {
        Some(jack) => match acceleration(jack, strata)? {
            Some(a) => bca_interval(estimate, &valid, a, level)?,
            None => None,
        },
        None => None,
    };
    Ok(BootstrapSummary {
        estimate,
        replicates: valid.len(),
        failed: replicates.len() - valid.len(),
        mean,
        bias: mean - estimate,
        std_error: var.sqrt(),
        percentile,
        bca,
        level,
    })
}

/// Bootstrap a statistic of one sample: `statistic` is evaluated on `x`,
/// on `replicates` resamples of `x` (seeded by `seed`) and on the jackknife
/// samples, and summarized at `level` with percentile and BCa intervals.
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than two values; otherwise as
/// for [`summarize`].
pub fn bootstrap_statistic<F>(
    x: &[f64],
    statistic: F,
    replicates: usize,
    seed: u64,
    level: f64,
) -> Result<BootstrapSummary>
where
    F: Fn(&[f64]) -> f64 + Sync,
{
    if x.len() < 2 {
        return Err(StatError::InsufficientData { required: 2, found: x.len() });
    }
    let take = |idx: &[usize]| statistic(&idx.iter().map(|&i| x[i]).collect::<Vec<_>>());
    let reps = bootstrap_map(x.len(), None, replicates, seed, take)?;
    let jack = jackknife_map(x.len(), take)?;
    summarize(statistic(x), &reps, Some(&jack), None, level)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mean(x: &[f64]) -> f64 {
        x.iter().sum::<f64>() / x.len() as f64
    }

    #[test]
    fn replicates_do_not_depend_on_threads() {
        let f = |idx: &[usize]| idx.iter().map(|&i| i as f64).sum::<f64>();
        let a = bootstrap_map(20, None, 200, 7, f).unwrap();
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let b = pool.install(|| bootstrap_map(20, None, 200, 7, f).unwrap());
        assert_eq!(a, b);
        assert_eq!(bootstrap_sample(20, None, 7, 3).unwrap().iter().map(|&i| i as f64).sum::<f64>(), a[3]);
        let c = bootstrap_map(20, None, 200, 8, f).unwrap();
        assert_ne!(a, c, "another seed draws other resamples");
        assert_ne!(replicate_seed(1, 0), replicate_seed(1, 1));
    }

    /// An interrupt counts replicates, reaches the statistic's own thread,
    /// and stops the resampling once cancelled.
    #[test]
    fn interrupt_counts_and_cancels_replicates() {
        use stat_core::interrupt::{Interrupt, check, with};
        let f = |idx: &[usize]| (idx.len(), check().is_ok());
        let i = Interrupt::new();
        let reps = with(std::sync::Arc::clone(&i), || bootstrap_map(10, None, 40, 1, f)).unwrap();
        assert_eq!(i.progress(), (40, 40));
        assert!(reps.iter().all(|r| *r == (10, true)));
        i.cancel();
        assert!(matches!(with(std::sync::Arc::clone(&i), || bootstrap_map(10, None, 40, 1, f)), Err(StatError::Cancelled)));
        assert!(matches!(with(i, || jackknife_map(10, f)), Err(StatError::Cancelled)));
        assert_eq!(bootstrap_map(10, None, 3, 1, f).unwrap().len(), 3, "without an interrupt nothing stops");
    }

    #[test]
    fn strata_keep_their_sizes() {
        let strata = [0, 0, 0, 1, 1, 2, 2, 2, 2, 2];
        for b in 0..50 {
            let s = bootstrap_sample(10, Some(&strata), 11, b).unwrap();
            for (i, &j) in s.iter().enumerate() {
                assert_eq!(strata[i], strata[j], "position {i} drew {j} from another stratum");
            }
        }
        assert!(bootstrap_map(10, Some(&strata[..3]), 5, 1, |_| 0).is_err());
        assert!(bootstrap_map(0, None, 5, 1, |_| 0).is_err());
    }

    #[test]
    fn standard_error_of_the_mean() {
        // The ideal bootstrap SE of a mean is σ̂/√n with σ̂² = Σ(x − x̄)²/n
        // (Efron & Tibshirani eq. 5.12 and §6.2): s/√n · √((n − 1)/n).
        let mut rng = Rng::new(5);
        let x: Vec<f64> = (0..30).map(|_| rng.normal() * 2.0 + 10.0).collect();
        let n = x.len() as f64;
        let m = mean(&x);
        let s = (x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
        let ideal = s / n.sqrt() * ((n - 1.0) / n).sqrt();
        let summary = bootstrap_statistic(&x, mean, 20_000, 99, 0.95).unwrap();
        assert!((summary.std_error / ideal - 1.0).abs() < 0.02, "{} vs {ideal}", summary.std_error);
        assert!(summary.bias.abs() < 0.02 * s);
        assert_eq!(summary.replicates, 20_000);
        assert_eq!(summary.failed, 0);
        let bca = summary.bca.unwrap();
        assert!(bca.lower < m && m < bca.upper);
        assert!(bca.acceleration.is_finite() && bca.bias_correction.is_finite());
    }

    #[test]
    fn failed_replicates_are_counted() {
        let reps = [1.0, f64::NAN, 2.0, 3.0, f64::INFINITY, 4.0];
        let s = summarize(2.5, &reps, None, None, 0.9).unwrap();
        assert_eq!((s.replicates, s.failed), (4, 2));
        assert!((s.mean - 2.5).abs() < 1e-15 && s.bca.is_none());
        assert!(summarize(1.0, &[1.0, f64::NAN], None, None, 0.9).is_err());
        assert!(summarize(f64::NAN, &reps, None, None, 0.9).is_err());
        assert!(summarize(1.0, &reps, None, None, 1.5).is_err());
        // Every replicate above the estimate: ẑ₀ is infinite, no BCa interval.
        assert!(bca_interval(0.0, &[1.0, 2.0, 3.0], 0.0, 0.9).unwrap().is_none());
        assert!(acceleration(&[1.0, 1.0, 1.0], None).unwrap().is_none());
    }

    #[test]
    fn coverage_on_simulated_samples() {
        // Percentile and BCa 90% intervals for the mean should cover the true
        // mean about 90% of the time, for normal and for skewed (exponential)
        // data. 400 samples of 25, 1000 replicates each; binomial standard
        // error of the coverage ≈ 1.5%. BCa corrects the skewness the
        // percentile interval misses (Efron & Tibshirani §14.3).
        for (skewed, truth) in [(false, 0.0), (true, 1.0)] {
            let mut rng = Rng::new(if skewed { 21 } else { 20 });
            let (mut pct, mut bca, trials) = (0, 0, 400);
            for t in 0..trials {
                let x: Vec<f64> =
                    (0..25).map(|_| if skewed { -(1.0 - rng.next_f64()).ln() } else { rng.normal() }).collect();
                let s = bootstrap_statistic(&x, mean, 1000, t, 0.9).unwrap();
                pct += usize::from(s.percentile.0 <= truth && truth <= s.percentile.1);
                let b = s.bca.unwrap();
                bca += usize::from(b.lower <= truth && truth <= b.upper);
            }
            let (pct, bca) = (pct as f64 / trials as f64, bca as f64 / trials as f64);
            assert!((0.83..=0.95).contains(&pct), "percentile coverage {pct} (skewed {skewed})");
            assert!((0.84..=0.96).contains(&bca), "BCa coverage {bca} (skewed {skewed})");
        }
    }
}
