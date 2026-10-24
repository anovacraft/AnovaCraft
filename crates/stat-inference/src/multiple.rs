//! Comparisons of every pair of groups with control of the familywise error
//! rate (Hsu, *Multiple Comparisons*, 1996, ch. 5; Hochberg & Tamhane,
//! *Multiple Comparison Procedures*, 1987).
//!
//! * **Tukey–Kramer:** pooled `MSE` on `N − k` df; for groups `i, j`,
//!   `SE = √(MSE(1/nᵢ + 1/nⱼ))`, `q = √2·|ȳᵢ − ȳⱼ|/SE` referred to the
//!   studentized range with `k` means; simultaneous interval
//!   `ȳᵢ − ȳⱼ ± q_{1−α}(k, N − k)/√2 · SE` (Kramer 1956).
//! * **Games–Howell** (1976): as Tukey–Kramer but `SE = √(sᵢ²/nᵢ + sⱼ²/nⱼ)` on
//!   the Welch df of each pair — for unequal variances.
//! * **Holm-adjusted t:** pooled-variance t tests with Holm's (1979)
//!   step-down adjustment of the p-values; intervals use Bonferroni's
//!   `α/m` for the `m = k(k−1)/2` pairs.
//! * **Dunn** (1964): rank-based; `z = (R̄ᵢ − R̄ⱼ)/√([N(N+1)/12 − Σ(t³−t)/(12(N−1))](1/nᵢ + 1/nⱼ))`
//!   from mean ranks of the pooled data, Holm-adjusted.
//! * **Bonferroni, Šidák, Holm–Šidák, Fisher LSD:** pooled-variance t tests
//!   (`t = (ȳᵢ − ȳⱼ)/√(MSE(1/nᵢ + 1/nⱼ))` on `N − k` df) with p-values
//!   `min(1, m p)`, `1 − (1 − p)ᵐ`, the step-down
//!   `p̃₍ᵢ₎ = maxⱼ≤ᵢ 1 − (1 − p₍ⱼ₎)^{m−j+1}` (Holm 1979 with Šidák's
//!   inequality), or unadjusted (Fisher's least significant difference,
//!   meant to follow a significant overall F test). Intervals use
//!   `t_{1−α/(2m)}`, `t_{(1+(1−α)^{1/m})/2}` (Šidák's also for Holm–Šidák)
//!   or `t_{1−α/2}`.
//! * **Newman–Keuls** (Newman 1939; Keuls 1952): the means are ranked and a
//!   pair whose means are `r` apart in that order (`r = 2` for neighbours,
//!   `k` for the extremes) is tested by Tukey–Kramer's `q` against the
//!   studentized range with `r` means. Testing is step-down: a pair is
//!   declared different only if every pair spanning it is, so each reported
//!   p-value is the largest raw p-value among the pairs whose span contains
//!   it. No simultaneous intervals exist for this procedure.
//! * **Tamhane T2** (Tamhane 1979) and **Dunnett T3** (Dunnett 1980,
//!   *JASA* 75): Welch t statistics `t = (ȳᵢ − ȳⱼ)/√(sᵢ²/nᵢ + sⱼ²/nⱼ)` on the
//!   Welch–Satterthwaite df `νᵢⱼ` of each pair, for unequal variances. T2
//!   applies Šidák's adjustment to each pair's t p-value; T3 refers `|t|` to
//!   the studentized maximum modulus with `m` comparisons and `νᵢⱼ` df,
//!   `P(M ≤ c) = ∫₀^∞ [2Φ(cs) − 1]ᵐ f_S(s) ds` with `S = √(χ²_ν/ν)`, and
//!   uses its quantile for the intervals.
//!
//! Here `m = k(k − 1)/2` is the number of pairs.
//!
//! **Linear trend** across ordered groups (Kirk, *Experimental Design*, 4th
//! ed., §5.3): with equally spaced coefficients `cᵢ = i − (k + 1)/2`, the
//! contrast `L = Σ cᵢ ȳᵢ` has `SE = √(MSE Σ cᵢ²/nᵢ)` and `t = L/SE` on
//! `N − k` df; its sum of squares `L²/Σ(cᵢ²/nᵢ)` leaves
//! `SS_between − SS_L` on `k − 2` df for the test of departure from a
//! straight line.

use crate::common::{check_level, increasing_root, invalid, mean_var, ranks_with_ties};
use rayon::prelude::*;
use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
use stat_core::quadrature::gauss_legendre;
use stat_core::special::{erfc, ln_gamma};
use stat_core::{Result, StatError, StudentizedRange};

/// Which multiple-comparison procedure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComparisonMethod {
    /// Tukey–Kramer honestly significant differences.
    TukeyKramer,
    /// Games–Howell (unequal variances).
    GamesHowell,
    /// Pooled t tests with Holm-adjusted p-values.
    HolmT,
    /// Dunn's rank test with Holm-adjusted p-values.
    Dunn,
    /// Pooled t tests with Bonferroni-adjusted p-values.
    Bonferroni,
    /// Pooled t tests with Šidák-adjusted p-values.
    Sidak,
    /// Pooled t tests with Holm–Šidák step-down p-values.
    HolmSidak,
    /// Unadjusted pooled t tests (Fisher's least significant difference).
    FisherLsd,
    /// Newman–Keuls stepwise studentized range.
    NewmanKeuls,
    /// Tamhane's T2 (Welch t, Šidák-adjusted; unequal variances).
    TamhaneT2,
    /// Dunnett's T3 (Welch t, studentized maximum modulus; unequal variances).
    DunnettT3,
}

impl ComparisonMethod {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::TukeyKramer => "Tukey–Kramer",
            Self::GamesHowell => "Games–Howell",
            Self::HolmT => "Holm-adjusted t",
            Self::Dunn => "Dunn (rank-based, Holm-adjusted)",
            Self::Bonferroni => "Bonferroni t",
            Self::Sidak => "Šidák t",
            Self::HolmSidak => "Holm–Šidák t",
            Self::FisherLsd => "Fisher LSD (unadjusted)",
            Self::NewmanKeuls => "Newman–Keuls",
            Self::TamhaneT2 => "Tamhane T2",
            Self::DunnettT3 => "Dunnett T3",
        }
    }

    /// The methods that compare means (every method but Dunn's), in menu
    /// order.
    pub const MEANS: [Self; 10] = [
        Self::TukeyKramer,
        Self::GamesHowell,
        Self::HolmT,
        Self::Bonferroni,
        Self::Sidak,
        Self::HolmSidak,
        Self::FisherLsd,
        Self::NewmanKeuls,
        Self::TamhaneT2,
        Self::DunnettT3,
    ];

    /// Identifier in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::TukeyKramer => "tukey-kramer",
            Self::GamesHowell => "games-howell",
            Self::HolmT => "holm-t",
            Self::Dunn => "dunn",
            Self::Bonferroni => "bonferroni",
            Self::Sidak => "sidak",
            Self::HolmSidak => "holm-sidak",
            Self::FisherLsd => "fisher-lsd",
            Self::NewmanKeuls => "newman-keuls",
            Self::TamhaneT2 => "tamhane-t2",
            Self::DunnettT3 => "dunnett-t3",
        }
    }

    /// The method named `key` (see [`key`](Self::key)).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::MEANS.into_iter().chain([Self::Dunn]).find(|m| m.key() == key)
    }

    /// Symbol of the test statistic.
    #[must_use]
    pub fn statistic_symbol(self) -> &'static str {
        match self {
            Self::TukeyKramer | Self::GamesHowell | Self::NewmanKeuls => "q",
            Self::Dunn => "z",
            _ => "t",
        }
    }

    /// Whether the method allows unequal variances (Welch standard errors).
    #[must_use]
    pub fn unequal_variances(self) -> bool {
        matches!(self, Self::GamesHowell | Self::TamhaneT2 | Self::DunnettT3)
    }

    /// What the table's intervals are, e.g. "simultaneous 95% intervals".
    #[must_use]
    pub fn interval_note(self, level: f64) -> String {
        let pct = 100.0 * level;
        let pct = if (pct - pct.round()).abs() < 1e-9 { format!("{pct:.0}") } else { format!("{pct}") };
        match self {
            Self::Dunn => "differences of mean ranks".to_owned(),
            Self::NewmanKeuls => "stepwise; no simultaneous intervals".to_owned(),
            Self::FisherLsd => format!("unadjusted {pct}% intervals"),
            Self::HolmT => format!("Bonferroni {pct}% intervals"),
            Self::HolmSidak => format!("Šidák {pct}% intervals"),
            _ => format!("simultaneous {pct}% intervals"),
        }
    }
}

/// One pair of groups.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairwiseComparison {
    /// First group index.
    pub first: usize,
    /// Second group index.
    pub second: usize,
    /// `ȳ_first − ȳ_second` (mean ranks for Dunn).
    pub difference: f64,
    /// Standard error of the difference.
    pub std_error: f64,
    /// Test statistic (`q`, `t` or `z`).
    pub statistic: f64,
    /// Degrees of freedom (∞ for Dunn).
    pub df: f64,
    /// Adjusted p-value.
    pub p_value: f64,
    /// Simultaneous confidence interval for the difference (NaN for Dunn).
    pub confidence_interval: (f64, f64),
}

fn check_groups<G: AsRef<[f64]>>(groups: &[G], min: usize) -> Result<()> {
    if groups.len() < 2 {
        return Err(invalid("multiple comparisons need at least two groups"));
    }
    for g in groups {
        let g = g.as_ref();
        if g.len() < min {
            return Err(StatError::InsufficientData { required: min, found: g.len() });
        }
        crate::common::check_finite(g, "group values")?;
    }
    Ok(())
}

fn pairs(k: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..k).flat_map(move |i| (i + 1..k).map(move |j| (i, j)))
}

/// Holm's step-down adjustment of `p` (order preserved).
#[must_use]
pub fn holm_adjust(p: &[f64]) -> Vec<f64> {
    let m = p.len();
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&a, &b| p[a].total_cmp(&p[b]));
    let mut out = vec![0.0; m];
    let mut running: f64 = 0.0;
    for (rank, &i) in order.iter().enumerate() {
        running = running.max(((m - rank) as f64 * p[i]).min(1.0));
        out[i] = running;
    }
    out
}

fn pooled_mse<G: AsRef<[f64]>>(groups: &[G]) -> Result<(f64, f64)> {
    let n: usize = groups.iter().map(|g| g.as_ref().len()).sum();
    let df = (n - groups.len()) as f64;
    if df < 1.0 {
        return Err(invalid("no error degrees of freedom: every group has a single value"));
    }
    let ss: f64 = groups
        .iter()
        .map(|g| {
            let g = g.as_ref();
            let (m, _) = mean_var(g);
            g.iter().map(|v| (v - m).powi(2)).sum::<f64>()
        })
        .sum();
    let mse = ss / df;
    if mse == 0.0 {
        return Err(invalid("the groups have no within-group variation"));
    }
    Ok((mse, df))
}

/// Tukey–Kramer comparisons of all pairs at simultaneous confidence `level`.
///
/// # Errors
/// Fewer than two groups, an empty group, no error df, or zero variance.
pub fn tukey_kramer<G: AsRef<[f64]> + Sync>(groups: &[G], level: f64) -> Result<Vec<PairwiseComparison>> {
    check_groups(groups, 1)?;
    check_level(level)?;
    let (mse, df) = pooled_mse(groups)?;
    let k = groups.len();
    let dist = StudentizedRange::new(k, df)?;
    let crit = dist.quantile(level)? / std::f64::consts::SQRT_2;
    // Each p-value is a double integral; with hundreds of groups (tens of
    // thousands of pairs) they run in parallel.
    let all: Vec<(usize, usize)> = pairs(k).collect();
    all.into_par_iter()
        .map(|(i, j)| {
            let (a, b) = (groups[i].as_ref(), groups[j].as_ref());
            let diff = mean_var(a).0 - mean_var(b).0;
            let se = (mse * (1.0 / a.len() as f64 + 1.0 / b.len() as f64)).sqrt();
            let q = std::f64::consts::SQRT_2 * diff.abs() / se;
            Ok(PairwiseComparison {
                first: i,
                second: j,
                difference: diff,
                std_error: se,
                statistic: q,
                df,
                p_value: dist.sf(q)?,
                confidence_interval: (diff - crit * se, diff + crit * se),
            })
        })
        .collect()
}

/// Games–Howell comparisons (no equal-variance assumption).
///
/// # Errors
/// Fewer than two groups, a group with fewer than two values, or zero variance
/// in both groups of a pair.
pub fn games_howell<G: AsRef<[f64]>>(groups: &[G], level: f64) -> Result<Vec<PairwiseComparison>> {
    check_groups(groups, 2)?;
    check_level(level)?;
    let k = groups.len();
    pairs(k)
        .map(|(i, j)| {
            let (a, b) = (groups[i].as_ref(), groups[j].as_ref());
            let ((m1, v1), (m2, v2)) = (mean_var(a), mean_var(b));
            let (q1, q2) = (v1 / a.len() as f64, v2 / b.len() as f64);
            if q1 + q2 == 0.0 {
                return Err(invalid("two groups have no variation"));
            }
            let se = (q1 + q2).sqrt();
            let df = (q1 + q2).powi(2) / (q1 * q1 / (a.len() as f64 - 1.0) + q2 * q2 / (b.len() as f64 - 1.0));
            let dist = StudentizedRange::new(k, df)?;
            let crit = dist.quantile(level)? / std::f64::consts::SQRT_2;
            let diff = m1 - m2;
            let q = std::f64::consts::SQRT_2 * diff.abs() / se;
            Ok(PairwiseComparison {
                first: i,
                second: j,
                difference: diff,
                std_error: se,
                statistic: q,
                df,
                p_value: dist.sf(q)?,
                confidence_interval: (diff - crit * se, diff + crit * se),
            })
        })
        .collect()
}

/// Pooled-variance t tests of all pairs with Holm-adjusted p-values and
/// Bonferroni intervals.
///
/// # Errors
/// As [`tukey_kramer`].
pub fn holm_t<G: AsRef<[f64]>>(groups: &[G], level: f64) -> Result<Vec<PairwiseComparison>> {
    check_groups(groups, 1)?;
    check_level(level)?;
    let (mse, df) = pooled_mse(groups)?;
    let k = groups.len();
    let m = (k * (k - 1) / 2) as f64;
    let t = StudentsT::new(df)?;
    let crit = t.quantile(1.0 - (1.0 - level) / (2.0 * m))?;
    let mut out: Vec<PairwiseComparison> = pairs(k)
        .map(|(i, j)| {
            let (a, b) = (groups[i].as_ref(), groups[j].as_ref());
            let diff = mean_var(a).0 - mean_var(b).0;
            let se = (mse * (1.0 / a.len() as f64 + 1.0 / b.len() as f64)).sqrt();
            let stat = diff / se;
            Ok(PairwiseComparison {
                first: i,
                second: j,
                difference: diff,
                std_error: se,
                statistic: stat,
                df,
                p_value: t.two_sided_p_value(stat)?,
                confidence_interval: (diff - crit * se, diff + crit * se),
            })
        })
        .collect::<Result<_>>()?;
    let adjusted = holm_adjust(&out.iter().map(|c| c.p_value).collect::<Vec<_>>());
    for (c, p) in out.iter_mut().zip(adjusted) {
        c.p_value = p;
    }
    Ok(out)
}

/// Dunn's rank-based comparisons with Holm-adjusted p-values.
///
/// # Errors
/// Fewer than two groups, an empty group, or all values tied.
pub fn dunn<G: AsRef<[f64]>>(groups: &[G]) -> Result<Vec<PairwiseComparison>> {
    check_groups(groups, 1)?;
    let all: Vec<f64> = groups.iter().flat_map(|g| g.as_ref().iter().copied()).collect();
    let (ranks, ties) = ranks_with_ties(&all);
    let n = all.len() as f64;
    let base = n * (n + 1.0) / 12.0 - ties / (12.0 * (n - 1.0));
    if base <= 0.0 {
        return Err(invalid("all values are tied"));
    }
    let mut start = 0;
    let mut mean_ranks = Vec::with_capacity(groups.len());
    for g in groups {
        let len = g.as_ref().len();
        mean_ranks.push(ranks[start..start + len].iter().sum::<f64>() / len as f64);
        start += len;
    }
    let normal = Normal::standard();
    let mut out: Vec<PairwiseComparison> = pairs(groups.len())
        .map(|(i, j)| {
            let (ni, nj) = (groups[i].as_ref().len() as f64, groups[j].as_ref().len() as f64);
            let se = (base * (1.0 / ni + 1.0 / nj)).sqrt();
            let diff = mean_ranks[i] - mean_ranks[j];
            let z = diff / se;
            Ok(PairwiseComparison {
                first: i,
                second: j,
                difference: diff,
                std_error: se,
                statistic: z,
                df: f64::INFINITY,
                p_value: (2.0 * normal.sf(z.abs())?).min(1.0),
                confidence_interval: (f64::NAN, f64::NAN),
            })
        })
        .collect::<Result<_>>()?;
    let adjusted = holm_adjust(&out.iter().map(|c| c.p_value).collect::<Vec<_>>());
    for (c, p) in out.iter_mut().zip(adjusted) {
        c.p_value = p;
    }
    Ok(out)
}

/// Run `method` on `groups`.
///
/// # Errors
/// As the individual procedures.
pub fn compare_all_pairs<G: AsRef<[f64]> + Sync>(
    groups: &[G],
    method: ComparisonMethod,
    level: f64,
) -> Result<Vec<PairwiseComparison>> {
    match method {
        ComparisonMethod::TukeyKramer => tukey_kramer(groups, level),
        ComparisonMethod::GamesHowell => games_howell(groups, level),
        ComparisonMethod::HolmT => holm_t(groups, level),
        ComparisonMethod::Dunn => dunn(groups),
        ComparisonMethod::Bonferroni => pooled_t(groups, level, PooledAdjust::Bonferroni),
        ComparisonMethod::Sidak => pooled_t(groups, level, PooledAdjust::Sidak),
        ComparisonMethod::HolmSidak => pooled_t(groups, level, PooledAdjust::HolmSidak),
        ComparisonMethod::FisherLsd => pooled_t(groups, level, PooledAdjust::None),
        ComparisonMethod::NewmanKeuls => newman_keuls(groups),
        ComparisonMethod::TamhaneT2 => welch_pairs(groups, level, false),
        ComparisonMethod::DunnettT3 => welch_pairs(groups, level, true),
    }
}

/// Šidák's adjustment `1 − (1 − p)ᵐ`, accurate for small `p`.
fn sidak(p: f64, m: f64) -> f64 {
    -(m * (-p).ln_1p()).exp_m1()
}

/// The per-comparison error rate `1 − level^{1/m}` of Šidák's intervals.
fn sidak_alpha(level: f64, m: f64) -> f64 {
    -(level.ln() / m).exp_m1()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PooledAdjust {
    None,
    Bonferroni,
    Sidak,
    HolmSidak,
}

/// Pooled-variance t tests of all pairs with the chosen adjustment.
fn pooled_t<G: AsRef<[f64]>>(groups: &[G], level: f64, adjust: PooledAdjust) -> Result<Vec<PairwiseComparison>> {
    check_groups(groups, 1)?;
    check_level(level)?;
    let (mse, df) = pooled_mse(groups)?;
    let k = groups.len();
    let m = (k * (k - 1) / 2) as f64;
    let t = StudentsT::new(df)?;
    let alpha = 1.0 - level;
    let per_pair = match adjust {
        PooledAdjust::None => alpha,
        PooledAdjust::Bonferroni => alpha / m,
        PooledAdjust::Sidak | PooledAdjust::HolmSidak => sidak_alpha(level, m),
    };
    let crit = t.quantile(1.0 - per_pair / 2.0)?;
    let mut out: Vec<PairwiseComparison> = pairs(k)
        .map(|(i, j)| {
            let (a, b) = (groups[i].as_ref(), groups[j].as_ref());
            let diff = mean_var(a).0 - mean_var(b).0;
            let se = (mse * (1.0 / a.len() as f64 + 1.0 / b.len() as f64)).sqrt();
            let stat = diff / se;
            Ok(PairwiseComparison {
                first: i,
                second: j,
                difference: diff,
                std_error: se,
                statistic: stat,
                df,
                p_value: t.two_sided_p_value(stat)?,
                confidence_interval: (diff - crit * se, diff + crit * se),
            })
        })
        .collect::<Result<_>>()?;
    match adjust {
        PooledAdjust::None => {}
        PooledAdjust::Bonferroni => out.iter_mut().for_each(|c| c.p_value = (m * c.p_value).min(1.0)),
        PooledAdjust::Sidak => out.iter_mut().for_each(|c| c.p_value = sidak(c.p_value, m)),
        PooledAdjust::HolmSidak => {
            let mut order: Vec<usize> = (0..out.len()).collect();
            order.sort_by(|&a, &b| out[a].p_value.total_cmp(&out[b].p_value));
            let mut running: f64 = 0.0;
            for (rank, &i) in order.iter().enumerate() {
                running = running.max(sidak(out[i].p_value, m - rank as f64));
                out[i].p_value = running;
            }
        }
    }
    Ok(out)
}

/// Newman–Keuls stepwise comparisons (see the module notes).
fn newman_keuls<G: AsRef<[f64]>>(groups: &[G]) -> Result<Vec<PairwiseComparison>> {
    check_groups(groups, 1)?;
    let (mse, df) = pooled_mse(groups)?;
    let k = groups.len();
    let means: Vec<f64> = groups.iter().map(|g| mean_var(g.as_ref()).0).collect();
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by(|&a, &b| means[a].total_cmp(&means[b]));
    let mut rank = vec![0usize; k];
    for (r, &g) in order.iter().enumerate() {
        rank[g] = r;
    }
    let mut dists: Vec<Option<StudentizedRange>> = vec![None; k + 1];
    let mut out = Vec::with_capacity(k * (k - 1) / 2);
    let mut spans = Vec::with_capacity(k * (k - 1) / 2);
    for (i, j) in pairs(k) {
        let (a, b) = (groups[i].as_ref(), groups[j].as_ref());
        let diff = means[i] - means[j];
        let se = (mse * (1.0 / a.len() as f64 + 1.0 / b.len() as f64)).sqrt();
        let q = std::f64::consts::SQRT_2 * diff.abs() / se;
        let (lo, hi) = (rank[i].min(rank[j]), rank[i].max(rank[j]));
        let r = hi - lo + 1;
        let dist = match &dists[r] {
            Some(d) => d,
            None => dists[r].insert(StudentizedRange::new(r, df)?),
        };
        let p = dist.sf(q)?;
        spans.push((lo, hi));
        out.push(PairwiseComparison {
            first: i,
            second: j,
            difference: diff,
            std_error: se,
            statistic: q,
            df,
            p_value: p,
            confidence_interval: (f64::NAN, f64::NAN),
        });
    }
    let raw: Vec<f64> = out.iter().map(|c| c.p_value).collect();
    for (c, &(lo, hi)) in out.iter_mut().zip(&spans) {
        c.p_value =
            spans.iter().zip(&raw).filter(|((l, h), _)| *l <= lo && *h >= hi).map(|(_, &p)| p).fold(0.0_f64, f64::max);
    }
    Ok(out)
}

/// Tamhane T2 (`smm = false`) or Dunnett T3 (`smm = true`) comparisons.
fn welch_pairs<G: AsRef<[f64]>>(groups: &[G], level: f64, smm: bool) -> Result<Vec<PairwiseComparison>> {
    check_groups(groups, 2)?;
    check_level(level)?;
    let k = groups.len();
    let m = k * (k - 1) / 2;
    pairs(k)
        .map(|(i, j)| {
            let (a, b) = (groups[i].as_ref(), groups[j].as_ref());
            let ((m1, v1), (m2, v2)) = (mean_var(a), mean_var(b));
            let (q1, q2) = (v1 / a.len() as f64, v2 / b.len() as f64);
            if q1 + q2 == 0.0 {
                return Err(invalid("two groups have no variation"));
            }
            let se = (q1 + q2).sqrt();
            let df = (q1 + q2).powi(2) / (q1 * q1 / (a.len() as f64 - 1.0) + q2 * q2 / (b.len() as f64 - 1.0));
            let diff = m1 - m2;
            let stat = diff / se;
            let (p, crit) = if smm {
                let d = StudentizedMaximumModulus::new(m, df)?;
                (d.sf(stat.abs()), d.quantile(level)?)
            } else {
                let t = StudentsT::new(df)?;
                (sidak(t.two_sided_p_value(stat)?, m as f64), t.quantile(1.0 - sidak_alpha(level, m as f64) / 2.0)?)
            };
            Ok(PairwiseComparison {
                first: i,
                second: j,
                difference: diff,
                std_error: se,
                statistic: stat,
                df,
                p_value: p,
                confidence_interval: (diff - crit * se, diff + crit * se),
            })
        })
        .collect()
}

fn phi_upper(x: f64) -> f64 {
    0.5 * erfc(x * std::f64::consts::FRAC_1_SQRT_2)
}

/// The studentized maximum modulus distribution: `M = max |Zᵢ| / S` for `m`
/// independent standard normal `Zᵢ` and an independent `S = √(χ²_ν/ν)`
/// (Stoline & Ury 1979), used by Dunnett's T3. `P(M > c)` is integrated over
/// `u = ln s` by composite 16-point Gauss–Legendre quadrature across the
/// central `1 − 2·10⁻¹⁴` of `S` (as [`StudentizedRange`] does); for
/// `ν > 10⁶`, `S` is taken as 1.
#[derive(Debug, Clone)]
pub struct StudentizedMaximumModulus {
    m: f64,
    /// Quadrature `(s, weight · s · f_S(s))` over `u = ln s`, or `[(1, 1)]`
    /// for `ν > 10⁶`.
    nodes: Vec<(f64, f64)>,
}

impl StudentizedMaximumModulus {
    /// `m ≥ 1` comparisons and `ν > 0` degrees of freedom.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for `m = 0` or `ν ≤ 0`.
    pub fn new(m: usize, df: f64) -> Result<Self> {
        if m == 0 {
            return Err(invalid("the studentized maximum modulus needs at least one comparison"));
        }
        if df.is_nan() || df <= 0.0 {
            return Err(invalid(format!("degrees of freedom must be positive (got {df})")));
        }
        if df > 1e6 {
            return Ok(Self { m: m as f64, nodes: vec![(1.0, 1.0)] });
        }
        let chi = ChiSquared::new(df)?;
        let lo = (chi.quantile(1e-14)? / df).sqrt().max(1e-300);
        let hi = (chi.quantile(1.0 - 1e-14)? / df).sqrt();
        let ln_c = std::f64::consts::LN_2 + 0.5 * df * (0.5 * df).ln() - ln_gamma(0.5 * df);
        let (base, base_w) = gauss_legendre(16);
        let (a, b) = (lo.ln(), hi.ln());
        let panels = ((b - a).ceil() as usize).max(8);
        let half = 0.5 * (b - a) / panels as f64;
        let mut nodes = Vec::with_capacity(16 * panels);
        for p in 0..panels {
            let mid = a + half * (2 * p + 1) as f64;
            for (x, w) in base.iter().zip(&base_w) {
                let u = mid + half * x;
                let s = u.exp();
                let weight = half * w * (ln_c + df * u - 0.5 * df * s * s).exp();
                if weight > 1e-22 {
                    nodes.push((s, weight));
                }
            }
        }
        Ok(Self { m: m as f64, nodes })
    }

    /// `P(M > c)`.
    #[must_use]
    pub fn sf(&self, c: f64) -> f64 {
        if c <= 0.0 {
            return 1.0;
        }
        // 1 − (1 − 2Φ(−cs))ᵐ, kept accurate in the far tail.
        let total: f64 =
            self.nodes.iter().map(|&(s, w)| w * -(self.m * (-2.0 * phi_upper(c * s)).ln_1p()).exp_m1()).sum();
        total.clamp(0.0, 1.0)
    }

    /// The `p` quantile.
    ///
    /// # Errors
    /// `p` outside `(0, 1)`; failure to converge.
    pub fn quantile(&self, p: f64) -> Result<f64> {
        check_level(p)?;
        increasing_root(|c| Ok((1.0 - p) - self.sf(c)), 0.0, 4.0, "studentized maximum modulus quantile")
    }
}

/// The linear-trend contrast across ordered groups.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearTrend {
    /// Contrast `L = Σ cᵢ ȳᵢ` with `cᵢ = i − (k + 1)/2`.
    pub contrast: f64,
    /// Its standard error.
    pub std_error: f64,
    /// `t = L/SE`.
    pub t_statistic: f64,
    /// Error degrees of freedom `N − k`.
    pub df: f64,
    /// Two-sided p-value of `t`.
    pub p_value: f64,
    /// Confidence interval for `L`.
    pub confidence_interval: (f64, f64),
    /// Sum of squares of the contrast.
    pub ss_linear: f64,
    /// Share of the between-group sum of squares explained by the trend.
    pub share_of_between: f64,
    /// F for departure from a straight line (`k − 2` and `N − k` df).
    pub nonlinearity_f: f64,
    /// Its numerator degrees of freedom `k − 2`.
    pub nonlinearity_df: f64,
    /// Its p-value.
    pub nonlinearity_p: f64,
}

/// Test for a linear trend across `groups` taken in the order given.
///
/// # Errors
/// Fewer than three groups, an empty group, no error df, or zero variance.
pub fn linear_trend<G: AsRef<[f64]>>(groups: &[G], level: f64) -> Result<LinearTrend> {
    check_groups(groups, 1)?;
    check_level(level)?;
    let k = groups.len();
    if k < 3 {
        return Err(invalid("a trend across groups needs at least three groups"));
    }
    let (mse, df) = pooled_mse(groups)?;
    let centre = (k as f64 + 1.0) / 2.0;
    let n_total: usize = groups.iter().map(|g| g.as_ref().len()).sum();
    let grand: f64 = groups.iter().flat_map(|g| g.as_ref().iter()).sum::<f64>() / n_total as f64;
    let (mut l, mut w, mut ss_between) = (0.0, 0.0, 0.0);
    for (i, g) in groups.iter().enumerate() {
        let g = g.as_ref();
        let c = (i + 1) as f64 - centre;
        let mean = mean_var(g).0;
        l += c * mean;
        w += c * c / g.len() as f64;
        ss_between += g.len() as f64 * (mean - grand).powi(2);
    }
    let se = (mse * w).sqrt();
    let t_stat = l / se;
    let t = StudentsT::new(df)?;
    let ss_linear = l * l / w;
    let rest_df = (k - 2) as f64;
    let f = (ss_between - ss_linear).max(0.0) / rest_df / mse;
    let crit = t.quantile(0.5 + level / 2.0)?;
    Ok(LinearTrend {
        contrast: l,
        std_error: se,
        t_statistic: t_stat,
        df,
        p_value: t.two_sided_p_value(t_stat)?,
        confidence_interval: (l - crit * se, l + crit * se),
        ss_linear,
        share_of_between: if ss_between > 0.0 { ss_linear / ss_between } else { f64::NAN },
        nonlinearity_f: f,
        nonlinearity_df: rest_df,
        nonlinearity_p: FisherSnedecor::new(rest_df, df)?.sf(f)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    const A: [f64; 5] = [24.5, 23.5, 26.4, 27.1, 29.9];
    const B: [f64; 5] = [28.4, 34.2, 29.5, 32.2, 30.1];
    const C: [f64; 5] = [26.1, 28.3, 24.3, 26.2, 27.8];

    /// SciPy 1.18 `tukey_hsd` (its documentation example data).
    #[test]
    fn tukey() {
        let t = tukey_kramer(&[&A[..], &B[..], &C[..]], 0.95).unwrap();
        close(t[0].p_value, 0.014_448_326_736_400_73, 1e-6);
        close(t[1].p_value, 0.980_310_724_094_108_1, 1e-7);
        close(t[2].p_value, 0.020_331_136_739_714_76, 1e-6);
        close(t[0].confidence_interval.0, -8.249_159_003_581_287, 1e-7);
        close(t[0].confidence_interval.1, -0.950_840_996_418_715_4, 1e-6);
        let a2: Vec<f64> = A.iter().copied().chain([25.0]).collect();
        let u = tukey_kramer(&[&a2[..], &B[..], &C[..]], 0.95).unwrap();
        close(u[0].p_value, 0.006_041_359_094_246_301, 1e-6);
        close(u[1].p_value, 0.927_089_830_175_577, 1e-7);
        close(u[2].p_value, 0.015_856_959_274_100_135, 1e-6);
    }

    /// Games–Howell and Dunn evaluated from their definitions with SciPy 1.18
    /// (`studentized_range.sf`, `rankdata`).
    #[test]
    fn games_howell_and_dunn() {
        let a2: Vec<f64> = A.iter().copied().chain([25.0]).collect();
        let groups = [&a2[..], &B[..], &C[..]];
        let g = games_howell(&groups, 0.95).unwrap();
        for (c, (p, df)) in g.iter().zip([
            (0.019_000_737_828_153_37, 8.594_973_110_788_496),
            (0.914_595_224_247_917_2, 8.779_284_077_478_035),
            (0.024_702_449_289_267_57, 7.068_107_167_980_005),
        ]) {
            close(c.p_value, p, 1e-6);
            close(c.df, df, 1e-12);
        }
        let d = dunn(&groups).unwrap();
        close(d[0].statistic, -2.636_234_553_358_417_3, 1e-12);
        for (c, p) in d.iter().zip([0.025_149_528_562_049_12, 0.889_648_348_356_833_8, 0.033_590_446_224_520_1]) {
            close(c.p_value, p, 1e-9);
        }
        let h = holm_t(&groups, 0.95).unwrap();
        assert!(h[1].p_value >= h[0].p_value, "Holm keeps the step-down order");
        assert_eq!(holm_adjust(&[0.01, 0.04, 0.03]), [0.03, 0.06, 0.06]);
    }

    /// Four groups of `tests/data/pairwise_reference.R` (R 4.3.3: base
    /// `pairwise.t.test`, `t.test`, `ptukey` and `integrate` for the
    /// studentized maximum modulus).
    fn four() -> Vec<Vec<f64>> {
        vec![
            vec![24.5, 23.5, 26.4, 27.1, 29.9, 25.0],
            B.to_vec(),
            C.to_vec(),
            vec![30.0, 27.5, 31.2, 29.9, 28.8, 30.5, 32.1],
        ]
    }

    fn check_p(method: ComparisonMethod, expected: [f64; 6], tol: f64) {
        let got = compare_all_pairs(&four(), method, 0.95).unwrap();
        for (c, e) in got.iter().zip(expected) {
            close(c.p_value, e, tol);
        }
    }

    #[test]
    fn pooled_t_adjustments_match_r() {
        check_p(
            ComparisonMethod::FisherLsd,
            [
                0.000_604_786_922_078_869,
                0.691_058_342_783_435_4,
                0.001_704_007_976_771_58,
                0.002_174_900_222_078_341_4,
                0.447_387_337_578_789_23,
                0.006_581_553_135_589_819,
            ],
            1e-9,
        );
        check_p(
            ComparisonMethod::Bonferroni,
            [
                0.003_628_721_532_473_214_5,
                1.0,
                0.010_224_047_860_629_481,
                0.013_049_401_332_470_048,
                1.0,
                0.039_489_318_813_538_91,
            ],
            1e-9,
        );
        check_p(
            ComparisonMethod::Sidak,
            [
                0.003_623_239_446_374_882,
                0.999_130_521_532_797,
                0.010_180_592_043_098_18,
                0.012_978_653_886_369_429,
                0.971_520_983_184_438_6,
                0.038_845_239_959_076_18,
            ],
            1e-9,
        );
        check_p(
            ComparisonMethod::HolmSidak,
            [
                0.003_623_239_446_374_882,
                0.694_619_245_331_741,
                0.008_491_052_888_179_196,
                0.008_671_260_870_856_234,
                0.694_619_245_331_741,
                0.019_614_993_973_834_927,
            ],
            1e-9,
        );
        let b = compare_all_pairs(&four(), ComparisonMethod::Bonferroni, 0.95).unwrap();
        close(b[0].confidence_interval.0, -8.266_405_967_550_096, 1e-10);
        close(b[0].confidence_interval.1, -1.360_260_699_116_577, 1e-10);
        let s = compare_all_pairs(&four(), ComparisonMethod::HolmSidak, 0.95).unwrap();
        close(s[0].confidence_interval.0, -8.255_089_188_724_654, 1e-10);
        close(s[0].confidence_interval.1, -1.371_577_477_942_019, 1e-10);
    }

    #[test]
    fn newman_keuls_matches_r() {
        check_p(
            ComparisonMethod::NewmanKeuls,
            [
                0.003_105_996_051_174_897_4,
                0.691_058_342_783_437_4,
                0.004_627_199_963_847_084,
                0.005_875_830_279_819_86,
                0.447_387_337_578_803_2,
                0.006_581_553_135_621_032,
            ],
            1e-6,
        );
        let nk = compare_all_pairs(&four(), ComparisonMethod::NewmanKeuls, 0.95).unwrap();
        assert!(nk[0].confidence_interval.0.is_nan(), "no simultaneous intervals");
        // Coherence: with means a < c < d < b the extremes span every pair.
        let g = vec![vec![1.0, 2.0, 3.0], vec![1.5, 2.5, 3.5], vec![9.0, 10.0, 11.0], vec![2.0, 3.0, 4.0]];
        let nk = compare_all_pairs(&g, ComparisonMethod::NewmanKeuls, 0.95).unwrap();
        let widest = nk.iter().find(|c| c.first == 0 && c.second == 2).unwrap().p_value;
        assert!(nk.iter().all(|c| c.p_value >= widest - 1e-15 || c.first == 0 && c.second == 2));
    }

    #[test]
    fn tamhane_and_dunnett_t3_match_r() {
        let t2 = compare_all_pairs(&four(), ComparisonMethod::TamhaneT2, 0.95).unwrap();
        let t3 = compare_all_pairs(&four(), ComparisonMethod::DunnettT3, 0.95).unwrap();
        let t = [
            -3.455_910_492_703_438_8,
            -0.404_537_062_024_319_5,
            -3.595_098_533_304_777_4,
            3.461_493_007_683_128_7,
            0.743_743_333_417_505_2,
            -3.799_740_469_447_979_5,
        ];
        let df = [
            8.594_973_110_788_498,
            8.779_284_077_478_033,
            8.486_803_732_261_789,
            7.068_107_167_980_005,
            6.426_648_076_403_777,
            8.525_511_312_321_878,
        ];
        let p2 = [
            0.045_467_208_467_568_13,
            0.999_202_815_845_013_4,
            0.037_659_157_720_051_684,
            0.060_613_432_340_051_23,
            0.980_976_630_157_635_9,
            0.027_628_695_882_934_484,
        ];
        let p3 = [
            0.040_048_126_524_861_6,
            0.998_449_768_787_982_4,
            0.033_213_323_943_785_47,
            0.051_191_611_142_601_35,
            0.963_116_924_900_645_3,
            0.024_519_550_465_275_854,
        ];
        let u2 = [
            -0.087_731_466_021_934_6,
            3.473_488_069_180_022_7,
            -0.207_982_419_754_348_95,
            8.862_977_581_532_986,
            5.302_289_613_207_028,
            -0.363_421_089_453_807_24,
        ];
        let u3 = [
            -0.208_342_201_546_678_93,
            3.376_232_156_571_455,
            -0.305_083_952_338_331_5,
            8.702_674_017_209_315,
            5.117_960_307_413_447,
            -0.443_525_945_843_062_26,
        ];
        for i in 0..6 {
            close(t2[i].statistic, t[i], 1e-12);
            close(t2[i].df, df[i], 1e-12);
            close(t2[i].p_value, p2[i], 1e-9);
            close(t3[i].p_value, p3[i], 1e-7);
            close(t2[i].confidence_interval.1, u2[i], 1e-9);
            close(t3[i].confidence_interval.1, u3[i], 1e-7);
        }
        let smm = StudentizedMaximumModulus::new(3, 10.0).unwrap();
        close(smm.quantile(0.95).unwrap(), 2.828_855_230_224_838_6, 1e-8);
        assert!(StudentizedMaximumModulus::new(0, 10.0).is_err());
    }

    #[test]
    fn linear_trend_matches_r() {
        let t = linear_trend(&four(), 0.95).unwrap();
        close(t.contrast, 3.73, 1e-12);
        close(t.std_error, 1.728_705_746_764_684, 1e-12);
        close(t.t_statistic, 2.157_683_577_428_249_2, 1e-12);
        close(t.p_value, 0.043_963_832_256_600_88, 1e-9);
        close(t.share_of_between, 0.177_112_687_884_203_12, 1e-12);
        close(t.nonlinearity_f, 10.815_241_178_200_896, 1e-12);
        close(t.nonlinearity_p, 0.000_731_249_220_388_471_8, 1e-8);
        assert!(linear_trend(&four()[..2], 0.95).is_err());
    }

    #[test]
    fn method_keys_round_trip() {
        for m in ComparisonMethod::MEANS.into_iter().chain([ComparisonMethod::Dunn]) {
            assert_eq!(ComparisonMethod::from_key(m.key()), Some(m));
        }
    }
}
