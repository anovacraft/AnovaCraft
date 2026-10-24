//! Agreement of appraisers who classify parts into categories: an attribute
//! (categorical) measurement-system study.
//!
//! Each of `k` appraisers rates each of `n` parts `m` times (trials) into one
//! of `c` categories; optionally every part has a known standard (reference)
//! category. The study asks four questions (Fleiss, Levin & Paik,
//! *Statistical Methods for Rates and Proportions*, 3rd ed., Wiley 2003,
//! ch. 18; AIAG, *Measurement Systems Analysis*, 4th ed., 2010, ch. III §C):
//!
//! * **within each appraiser** — does an appraiser repeat their own rating?
//!   (needs `m ≥ 2`);
//! * **each appraiser against the standard**;
//! * **between appraisers** — do all appraisers give the same rating?
//!   (needs `k ≥ 2`);
//! * **all appraisers against the standard**.
//!
//! # Percent agreement
//! A part *matches* when every rating in the comparison is the same (and, for
//! the comparisons with a standard, equals the standard). The percentage of
//! matched parts `x/n` is given with the exact Clopper–Pearson interval
//! `[B(α/2; x, n − x + 1), B(1 − α/2; x + 1, n − x)]` (Clopper & Pearson
//! 1934, *Biometrika* 26).
//!
//! # Cohen's kappa (two sets of ratings of the same parts)
//! With cell proportions `pᵢⱼ` of a `c × c` table, margins `pᵢ.`, `p.ⱼ` and
//! agreement weights `wᵢⱼ` (`wᵢᵢ = 1`; `wᵢⱼ = 0` off the diagonal for the
//! unweighted kappa of Cohen 1960, *Educ. Psychol. Meas.* 20, 37–46;
//! `1 − |i − j|/(c − 1)` linear or `1 − (i − j)²/(c − 1)²` quadratic for the
//! weighted kappa of Cohen 1968 on ordered categories)
//!
//! ```text
//! p_o = ΣΣ wᵢⱼ pᵢⱼ      p_e = ΣΣ wᵢⱼ pᵢ. p.ⱼ      κ = (p_o − p_e) / (1 − p_e)
//! ```
//!
//! The large-sample variances are those of Fleiss, Cohen & Everitt (1969),
//! *Psychol. Bull.* 72, 323–327, with `w̄ᵢ. = Σⱼ p.ⱼ wᵢⱼ`, `w̄.ⱼ = Σᵢ pᵢ. wᵢⱼ`:
//!
//! ```text
//! Var(κ)  = [ΣΣ pᵢⱼ (wᵢⱼ − (w̄ᵢ. + w̄.ⱼ)(1 − κ))² − (κ − p_e(1 − κ))²] / (N (1 − p_e)²)
//! Var₀(κ) = [ΣΣ pᵢ. p.ⱼ (wᵢⱼ − (w̄ᵢ. + w̄.ⱼ))² − p_e²] / (N (1 − p_e)²)
//! ```
//!
//! `Var₀` holds under no agreement beyond chance and gives the test
//! `z = κ / √Var₀`; `Var` gives the interval `κ ± z₁₋α/₂ √Var`. A
//! category-specific kappa collapses the table to that category against all
//! others (Fleiss, Levin & Paik §18.1).
//!
//! # Fleiss' kappa (many ratings of each part)
//! With `xᵢⱼ` of the `m` ratings of part `i` in category `j`, `pⱼ` the share
//! of all `nm` ratings in category `j` and `qⱼ = 1 − pⱼ` (Fleiss 1971,
//! *Psychol. Bull.* 76, 378–382):
//!
//! ```text
//! κⱼ = 1 − Σᵢ xᵢⱼ (m − xᵢⱼ) / (n m (m − 1) pⱼ qⱼ)
//! κ  = 1 − (n m² − ΣᵢΣⱼ xᵢⱼ²) / (n m (m − 1) Σⱼ pⱼ qⱼ)
//! ```
//!
//! with the null standard errors of Fleiss, Nee & Landis (1979),
//! *Psychol. Bull.* 86, 974–977 (Fleiss, Levin & Paik eq. 18.53–18.54):
//! `se₀(κⱼ) = √(2 / (n m (m − 1)))` and
//! `se₀(κ) = √2 · √((Σ pⱼqⱼ)² − Σ pⱼqⱼ(qⱼ − pⱼ)) / (Σ pⱼqⱼ · √(n m (m − 1)))`.
//! Within an appraiser the `m` trials are the ratings; between appraisers all
//! `k m` ratings of a part are.
//!
//! # Ordinal ratings
//! When the categories are ordered (scored `1, …, c`), **Kendall's
//! coefficient of concordance** measures agreement of `J` sets of ratings of
//! the `n` parts (Kendall & Babington Smith 1939; Siegel & Castellan,
//! *Nonparametric Statistics for the Behavioral Sciences*, 2nd ed. 1988,
//! §9.4). Each set is ranked (ties get mid-ranks), `Rᵢ` is part `i`'s rank
//! sum and `Tⱼ = Σ(t³ − t)` over the tie groups of set `j`:
//!
//! ```text
//! W = 12 Σ(Rᵢ − R̄)² / (J² (n³ − n) − J Σⱼ Tⱼ)      χ² = J (n − 1) W   on n − 1 df
//! ```
//!
//! Agreement with the standard is measured by **Kendall's τ_b** between the
//! ratings and the standard (Kendall, *Rank Correlation Methods*, 4th ed.
//! 1970, ch. 3), with `C` concordant and `D` discordant pairs and `n₁`, `n₂`
//! the pairs tied in each variable: `τ_b = (C − D)/√((n₀ − n₁)(n₀ − n₂))`,
//! `n₀ = N(N − 1)/2`; the test is `z = (C − D)/√Var(C − D)` with Kendall's
//! tie-corrected variance
//!
//! ```text
//! Var = [v₀ − Σt(t−1)(2t+5) − Σu(u−1)(2u+5)] / 18
//!       + Σt(t−1) Σu(u−1) / (2N(N−1)) + Σt(t−1)(t−2) Σu(u−1)(u−2) / (9N(N−1)(N−2)),
//! v₀ = N(N−1)(2N+5).
//! ```
//!
//! # Failure modes
//! Ratings must be balanced (every appraiser rates every part `m` times);
//! category indices must be in range; at least two parts are needed. Kappa is
//! undefined (NaN) when chance agreement is complete (every rating in one
//! category); concordance is undefined when every set ties all parts.

use crate::attribute_capability::clopper_pearson;
use stat_core::distributions::{ContinuousDistribution, Normal};
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Two-sided normal p-value of `z` (NaN for NaN).
fn normal_p(z: f64) -> f64 {
    if z.is_nan() {
        return f64::NAN;
    }
    Normal::standard().sf(z.abs()).map_or(f64::NAN, |q| (2.0 * q).min(1.0))
}

/// Percentage of parts on which every rating in a comparison agrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PercentAgreement {
    /// Parts compared.
    pub inspected: usize,
    /// Parts on which all ratings agree.
    pub matched: usize,
    /// `100 · matched / inspected`.
    pub percent: f64,
    /// Lower Clopper–Pearson limit, in percent.
    pub lower: f64,
    /// Upper Clopper–Pearson limit, in percent.
    pub upper: f64,
}

impl PercentAgreement {
    /// The percentage `matched / inspected` with its exact interval at
    /// confidence `level`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for no parts, `matched > inspected` or
    /// a level outside (0, 1).
    pub fn new(matched: usize, inspected: usize, level: f64) -> Result<Self> {
        let (lo, hi) = clopper_pearson(matched as f64, inspected as f64, level)?;
        Ok(Self {
            inspected,
            matched,
            percent: 100.0 * matched as f64 / inspected as f64,
            lower: 100.0 * lo,
            upper: 100.0 * hi,
        })
    }
}

/// Agreement weights of a Cohen kappa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KappaWeights {
    /// Unweighted: only identical categories agree.
    None,
    /// `1 − |i − j| / (c − 1)` (Cicchetti & Allison 1971).
    Linear,
    /// `1 − (i − j)² / (c − 1)²` (Fleiss & Cohen 1973).
    Quadratic,
}

impl KappaWeights {
    /// Weight of categories `i` and `j` among `c`.
    #[must_use]
    pub fn weight(self, i: usize, j: usize, c: usize) -> f64 {
        if i == j {
            return 1.0;
        }
        let d = i.abs_diff(j) as f64 / (c.max(2) - 1) as f64;
        match self {
            Self::None => 0.0,
            Self::Linear => 1.0 - d,
            Self::Quadratic => 1.0 - d * d,
        }
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Unweighted",
            Self::Linear => "Linear weights",
            Self::Quadratic => "Quadratic weights",
        }
    }
}

/// A kappa statistic with its standard errors and test of no agreement
/// beyond chance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KappaEstimate {
    /// κ (NaN when undefined).
    pub kappa: f64,
    /// Large-sample standard error (NaN when not available, as for Fleiss' kappa).
    pub se: f64,
    /// Standard error under κ = 0.
    pub se0: f64,
    /// `κ / se₀`.
    pub z: f64,
    /// Two-sided p-value of `z`.
    pub p_value: f64,
    /// Lower confidence limit `κ − z₁₋α/₂ se` (NaN without `se`).
    pub lower: f64,
    /// Upper confidence limit.
    pub upper: f64,
}

impl KappaEstimate {
    fn new(kappa: f64, se: f64, se0: f64, level: f64) -> Self {
        let z = if se0 > 0.0 { kappa / se0 } else { f64::NAN };
        let q = Normal::standard().quantile(0.5 + level / 2.0).unwrap_or(f64::NAN);
        Self { kappa, se, se0, z, p_value: normal_p(z), lower: kappa - q * se, upper: kappa + q * se }
    }

    fn undefined() -> Self {
        let nan = f64::NAN;
        Self { kappa: nan, se: nan, se0: nan, z: nan, p_value: nan, lower: nan, upper: nan }
    }
}

/// Cohen's kappa of a `c × c` table of counts (rows: first rating, columns:
/// second rating), with the weights `weights`, standard errors of Fleiss,
/// Cohen & Everitt (1969) and an interval at confidence `level`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a non-square or empty table, negative
/// or non-finite counts, or a level outside (0, 1).
pub fn cohen_kappa(table: &[Vec<f64>], weights: KappaWeights, level: f64) -> Result<KappaEstimate> {
    let c = table.len();
    if c == 0 || table.iter().any(|r| r.len() != c) {
        return Err(invalid("kappa needs a square table of counts"));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if table.iter().flatten().any(|&x| !x.is_finite() || x < 0.0) {
        return Err(invalid("counts must be non-negative"));
    }
    let n: f64 = table.iter().flatten().sum();
    if n <= 0.0 {
        return Err(invalid("kappa needs at least one pair of ratings"));
    }
    let p: Vec<Vec<f64>> = table.iter().map(|r| r.iter().map(|x| x / n).collect()).collect();
    let row: Vec<f64> = p.iter().map(|r| r.iter().sum()).collect();
    let col: Vec<f64> = (0..c).map(|j| p.iter().map(|r| r[j]).sum()).collect();
    let w = |i: usize, j: usize| weights.weight(i, j, c);
    let (mut po, mut pe) = (0.0, 0.0);
    for i in 0..c {
        for j in 0..c {
            po += w(i, j) * p[i][j];
            pe += w(i, j) * row[i] * col[j];
        }
    }
    if 1.0 - pe <= 1e-12 {
        return Ok(KappaEstimate::undefined());
    }
    let kappa = (po - pe) / (1.0 - pe);
    let wr: Vec<f64> = (0..c).map(|i| (0..c).map(|j| col[j] * w(i, j)).sum()).collect();
    let wc: Vec<f64> = (0..c).map(|j| (0..c).map(|i| row[i] * w(i, j)).sum()).collect();
    let (mut a, mut a0) = (0.0, 0.0);
    for i in 0..c {
        for j in 0..c {
            a += p[i][j] * (w(i, j) - (wr[i] + wc[j]) * (1.0 - kappa)).powi(2);
            a0 += row[i] * col[j] * (w(i, j) - (wr[i] + wc[j])).powi(2);
        }
    }
    let d = n * (1.0 - pe).powi(2);
    let var = ((a - (kappa - pe * (1.0 - kappa)).powi(2)) / d).max(0.0);
    let var0 = ((a0 - pe * pe) / d).max(0.0);
    Ok(KappaEstimate::new(kappa, var.sqrt(), var0.sqrt(), level))
}

/// Cohen's kappa of category `k` against all others: the table collapsed to
/// 2 × 2 (unweighted).
///
/// # Errors
/// As [`cohen_kappa`]; also `k` out of range.
pub fn category_kappa(table: &[Vec<f64>], k: usize, level: f64) -> Result<KappaEstimate> {
    let c = table.len();
    if k >= c {
        return Err(invalid("no such category"));
    }
    let mut t = vec![vec![0.0; 2]; 2];
    for (i, r) in table.iter().enumerate() {
        for (j, &x) in r.iter().enumerate() {
            t[usize::from(i != k)][usize::from(j != k)] += x;
        }
    }
    cohen_kappa(&t, KappaWeights::None, level)
}

/// Fleiss' kappa overall and for each category.
#[derive(Debug, Clone, PartialEq)]
pub struct FleissKappa {
    /// Overall κ.
    pub overall: KappaEstimate,
    /// κⱼ of each category (NaN for a category nobody used).
    pub by_category: Vec<KappaEstimate>,
    /// Parts.
    pub subjects: usize,
    /// Ratings per part.
    pub raters: usize,
}

/// Fleiss' (1971) kappa of an `n × c` table: `counts[i][j]` of the `m`
/// ratings of part `i` fall in category `j` (every row must sum to the same
/// `m ≥ 2`), with the null standard errors of Fleiss, Nee & Landis (1979).
/// Standard errors under the null are used for the z tests; no interval is
/// given (`se`, `lower` and `upper` are NaN).
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two parts, rows with
/// different totals, fewer than two ratings per part, or non-integer counts.
pub fn fleiss_kappa(counts: &[Vec<f64>]) -> Result<FleissKappa> {
    let n = counts.len();
    if n < 2 {
        return Err(invalid("Fleiss' kappa needs at least two parts"));
    }
    let c = counts[0].len();
    if counts.iter().any(|r| r.len() != c) || c == 0 {
        return Err(invalid("every part needs a count for every category"));
    }
    if counts.iter().flatten().any(|&x| !x.is_finite() || x < 0.0 || x.fract() != 0.0) {
        return Err(invalid("counts must be non-negative whole numbers"));
    }
    let m: f64 = counts[0].iter().sum();
    if counts.iter().any(|r| (r.iter().sum::<f64>() - m).abs() > 0.0) {
        return Err(invalid("Fleiss' kappa needs the same number of ratings of every part"));
    }
    if m < 2.0 {
        return Err(invalid("Fleiss' kappa needs at least two ratings of every part"));
    }
    let nf = n as f64;
    let nm = nf * m;
    let base = nf * m * (m - 1.0);
    let p: Vec<f64> = (0..c).map(|j| counts.iter().map(|r| r[j]).sum::<f64>() / nm).collect();
    let pq: Vec<f64> = p.iter().map(|&pj| pj * (1.0 - pj)).collect();
    let se_cat = (2.0 / base).sqrt();
    let by_category = (0..c)
        .map(|j| {
            if pq[j] <= 0.0 {
                return KappaEstimate::undefined();
            }
            let dis: f64 = counts.iter().map(|r| r[j] * (m - r[j])).sum();
            let k = 1.0 - dis / (base * pq[j]);
            KappaEstimate::new(k, f64::NAN, se_cat, 0.95)
        })
        .collect();
    let spq: f64 = pq.iter().sum();
    let overall = if spq <= 0.0 {
        KappaEstimate::undefined()
    } else {
        let sq: f64 = counts.iter().flatten().map(|x| x * x).sum();
        let k = 1.0 - (nf * m * m - sq) / (base * spq);
        let s: f64 = pq.iter().zip(&p).map(|(&v, &pj)| v * ((1.0 - pj) - pj)).sum();
        let se0 = 2f64.sqrt() * (spq * spq - s).max(0.0).sqrt() / (spq * base.sqrt());
        KappaEstimate::new(k, f64::NAN, se0, 0.95)
    };
    Ok(FleissKappa { overall, by_category, subjects: n, raters: m as usize })
}

/// Kendall's coefficient of concordance with its chi-square test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Concordance {
    /// W, between 0 (no agreement) and 1 (identical rankings); NaN when undefined.
    pub w: f64,
    /// `J (n − 1) W`.
    pub chi_square: f64,
    /// `n − 1`.
    pub df: f64,
    /// Upper-tail chi-square p-value.
    pub p_value: f64,
    /// Sets of ratings (judges) `J`.
    pub judges: usize,
    /// Parts `n`.
    pub objects: usize,
}

/// Mid-ranks of `x` (1-based) and the tie sum `Σ(t³ − t)`.
fn mid_ranks(x: &[f64]) -> (Vec<f64>, f64) {
    let n = x.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
    let mut ranks = vec![0.0; n];
    let mut ties = 0.0;
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && x[order[j + 1]] == x[order[i]] {
            j += 1;
        }
        let t = (j - i + 1) as f64;
        let r = (i + j) as f64 / 2.0 + 1.0;
        for &o in &order[i..=j] {
            ranks[o] = r;
        }
        ties += t * t * t - t;
        i = j + 1;
    }
    (ranks, ties)
}

/// Kendall's W of `scores[j][i]`: judge `j`'s score of object `i` (higher
/// scores rank higher; ties get mid-ranks and the tie correction).
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two judges or objects,
/// ragged rows or non-finite scores.
pub fn kendall_w(scores: &[Vec<f64>]) -> Result<Concordance> {
    let k = scores.len();
    let n = scores.first().map_or(0, Vec::len);
    if k < 2 || n < 2 {
        return Err(invalid("concordance needs at least two sets of ratings of at least two parts"));
    }
    if scores.iter().any(|s| s.len() != n || s.iter().any(|v| !v.is_finite())) {
        return Err(invalid("every set of ratings must score every part"));
    }
    let mut sums = vec![0.0; n];
    let mut ties = 0.0;
    for s in scores {
        let (r, t) = mid_ranks(s);
        sums.iter_mut().zip(&r).for_each(|(a, b)| *a += b);
        ties += t;
    }
    let (kf, nf) = (k as f64, n as f64);
    let mean = sums.iter().sum::<f64>() / nf;
    let ss: f64 = sums.iter().map(|r| (r - mean).powi(2)).sum();
    let denom = kf * kf * (nf * nf * nf - nf) - kf * ties;
    let w = if denom > 1e-9 { (12.0 * ss / denom).min(1.0) } else { f64::NAN };
    let chi = kf * (nf - 1.0) * w;
    let df = nf - 1.0;
    let p_value = if chi.is_nan() {
        f64::NAN
    } else {
        stat_core::ChiSquared::new(df).and_then(|d| d.sf(chi)).unwrap_or(f64::NAN)
    };
    Ok(Concordance { w, chi_square: chi, df, p_value, judges: k, objects: n })
}

/// Kendall's τ_b with its tie-corrected normal test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KendallTau {
    /// τ_b (NaN when either variable is constant).
    pub tau_b: f64,
    /// `(C − D) / √Var(C − D)`.
    pub z: f64,
    /// Two-sided p-value.
    pub p_value: f64,
    /// Pairs of values.
    pub n: usize,
}

/// Kendall's τ_b of paired values `x`, `y`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for different lengths, fewer than three
/// pairs or non-finite values.
pub fn kendall_tau_b(x: &[f64], y: &[f64]) -> Result<KendallTau> {
    let n = x.len();
    if y.len() != n {
        return Err(invalid("τ_b needs paired values"));
    }
    if n < 3 {
        return Err(invalid("τ_b needs at least three pairs"));
    }
    if x.iter().chain(y).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "Kendall's tau" });
    }
    let mut s = 0.0;
    for i in 0..n {
        for j in i + 1..n {
            let a = (x[i] - x[j]).signum() * f64::from(x[i] != x[j]);
            let b = (y[i] - y[j]).signum() * f64::from(y[i] != y[j]);
            s += a * b;
        }
    }
    let ties = |v: &[f64]| {
        let mut sorted = v.to_vec();
        sorted.sort_by(f64::total_cmp);
        let mut groups = Vec::new();
        let mut i = 0;
        while i < n {
            let mut j = i;
            while j + 1 < n && sorted[j + 1] == sorted[i] {
                j += 1;
            }
            groups.push((j - i + 1) as f64);
            i = j + 1;
        }
        let pairs: f64 = groups.iter().map(|t| t * (t - 1.0) / 2.0).sum();
        let v0: f64 = groups.iter().map(|t| t * (t - 1.0) * (2.0 * t + 5.0)).sum();
        let v1: f64 = groups.iter().map(|t| t * (t - 1.0)).sum();
        let v2: f64 = groups.iter().map(|t| t * (t - 1.0) * (t - 2.0)).sum();
        (pairs, v0, v1, v2)
    };
    let (tx, vx, x1, x2) = ties(x);
    let (ty, vy, y1, y2) = ties(y);
    let nf = n as f64;
    let n0 = nf * (nf - 1.0) / 2.0;
    let denom = ((n0 - tx) * (n0 - ty)).sqrt();
    let tau_b = if denom > 0.0 { s / denom } else { f64::NAN };
    let var = (nf * (nf - 1.0) * (2.0 * nf + 5.0) - vx - vy) / 18.0
        + x1 * y1 / (2.0 * nf * (nf - 1.0))
        + x2 * y2 / (9.0 * nf * (nf - 1.0) * (nf - 2.0));
    let z = if var > 0.0 && denom > 0.0 { s / var.sqrt() } else { f64::NAN };
    Ok(KendallTau { tau_b, z, p_value: normal_p(z), n })
}

// ---------------------------------------------------------------------------
// The study
// ---------------------------------------------------------------------------

/// Ratings of an attribute agreement study, indexed by appraiser, part and
/// trial; values are category indices (in category order).
#[derive(Debug, Clone, PartialEq)]
pub struct AgreementRatings {
    /// Category labels, in order (the order matters for ordinal ratings).
    pub categories: Vec<String>,
    /// Appraiser labels.
    pub appraisers: Vec<String>,
    /// Part labels.
    pub parts: Vec<String>,
    /// `ratings[a][p][t]`: appraiser `a`'s rating of part `p` on trial `t`;
    /// every appraiser rates every part the same number of times.
    pub ratings: Vec<Vec<Vec<usize>>>,
    /// The standard (reference) category of each part, if known.
    pub standard: Option<Vec<usize>>,
}

impl AgreementRatings {
    /// Trials per appraiser and part.
    #[must_use]
    pub fn trials(&self) -> usize {
        self.ratings.first().and_then(|a| a.first()).map_or(0, Vec::len)
    }
}

/// Settings of [`attribute_agreement`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgreementOptions {
    /// Confidence level of the intervals.
    pub confidence: f64,
    /// Categories are ordered: add weighted kappas, Kendall's W and τ_b.
    pub ordinal: bool,
}

impl Default for AgreementOptions {
    fn default() -> Self {
        Self { confidence: 0.95, ordinal: false }
    }
}

/// Repeatability of one appraiser over the trials.
#[derive(Debug, Clone, PartialEq)]
pub struct WithinAppraiser {
    /// Parts whose trials all agree.
    pub agreement: PercentAgreement,
    /// Fleiss' kappa with the trials as the ratings.
    pub kappa: FleissKappa,
    /// Kendall's W with the trials as the judges (ordinal ratings).
    pub concordance: Option<Concordance>,
}

/// One appraiser (or all appraisers pooled) against the standard.
#[derive(Debug, Clone, PartialEq)]
pub struct VersusStandard {
    /// Parts whose ratings all equal the standard.
    pub agreement: PercentAgreement,
    /// Parts rated the same on every trial (and by every appraiser, when
    /// pooled) but not as the standard.
    pub consistently_misrated: usize,
    /// Parts given different ratings.
    pub mixed: usize,
    /// Counts of ratings (rows: standard, columns: rating), every trial
    /// counted.
    pub table: Vec<Vec<f64>>,
    /// Cohen's kappa of the ratings against the standard.
    pub kappa: KappaEstimate,
    /// Kappa of each category against the others.
    pub by_category: Vec<KappaEstimate>,
    /// Linear and quadratic weighted kappas (ordinal ratings).
    pub weighted: Option<(KappaEstimate, KappaEstimate)>,
    /// Kendall's τ_b of ratings with the standard (ordinal ratings).
    pub tau: Option<KendallTau>,
}

/// Cohen's kappa of two appraisers, ratings paired by part and trial.
#[derive(Debug, Clone, PartialEq)]
pub struct PairwiseKappa {
    /// First appraiser index.
    pub first: usize,
    /// Second appraiser index.
    pub second: usize,
    /// Unweighted kappa.
    pub kappa: KappaEstimate,
}

/// Agreement among all appraisers.
#[derive(Debug, Clone, PartialEq)]
pub struct BetweenAppraisers {
    /// Parts on which every rating by every appraiser agrees.
    pub agreement: PercentAgreement,
    /// Fleiss' kappa with all `k m` ratings of a part.
    pub kappa: FleissKappa,
    /// Kendall's W with each appraiser's trial as a judge (ordinal ratings).
    pub concordance: Option<Concordance>,
    /// Cohen's kappa of every pair of appraisers.
    pub pairwise: Vec<PairwiseKappa>,
}

/// A computed attribute agreement study.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeAgreement {
    /// Each appraiser's repeatability (empty with a single trial).
    pub within: Vec<WithinAppraiser>,
    /// Each appraiser against the standard (empty without a standard).
    pub versus_standard: Vec<VersusStandard>,
    /// Agreement between appraisers (`None` with a single appraiser).
    pub between: Option<BetweenAppraisers>,
    /// All ratings against the standard (`None` without a standard).
    pub all_versus_standard: Option<VersusStandard>,
    /// Trials per appraiser and part.
    pub trials: usize,
    /// The settings used.
    pub options: AgreementOptions,
}

fn category_counts(ratings: &[&Vec<usize>], c: usize) -> Vec<f64> {
    let mut counts = vec![0.0; c];
    for r in ratings {
        for &v in r.iter() {
            counts[v] += 1.0;
        }
    }
    counts
}

fn versus(
    data: &AgreementRatings,
    standard: &[usize],
    appraisers: &[usize],
    options: &AgreementOptions,
) -> Result<VersusStandard> {
    let c = data.categories.len();
    let n = data.parts.len();
    let level = options.confidence;
    let mut table = vec![vec![0.0; c]; c];
    let (mut matched, mut wrong, mut mixed) = (0, 0, 0);
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for (p, &s) in standard.iter().enumerate().take(n) {
        let all: Vec<usize> = appraisers.iter().flat_map(|&a| data.ratings[a][p].iter().copied()).collect();
        for &r in &all {
            table[s][r] += 1.0;
            xs.push(r as f64);
            ys.push(s as f64);
        }
        if all.iter().all(|&r| r == all[0]) {
            if all[0] == s {
                matched += 1;
            } else {
                wrong += 1;
            }
        } else {
            mixed += 1;
        }
    }
    let kappa = cohen_kappa(&table, KappaWeights::None, level)?;
    let by_category = (0..c).map(|k| category_kappa(&table, k, level)).collect::<Result<Vec<_>>>()?;
    let (weighted, tau) = if options.ordinal {
        (
            Some((
                cohen_kappa(&table, KappaWeights::Linear, level)?,
                cohen_kappa(&table, KappaWeights::Quadratic, level)?,
            )),
            kendall_tau_b(&xs, &ys).ok(),
        )
    } else {
        (None, None)
    };
    Ok(VersusStandard {
        agreement: PercentAgreement::new(matched, n, level)?,
        consistently_misrated: wrong,
        mixed,
        table,
        kappa,
        by_category,
        weighted,
        tau,
    })
}

/// Run an attribute agreement study (see the module documentation for the
/// statistics).
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two categories or parts, no
/// appraisers, unbalanced ratings (every appraiser must rate every part the
/// same number of times), a category index out of range, a standard of the
/// wrong length, a study with nothing to compare (one appraiser, one trial
/// and no standard) or a level outside (0, 1).
pub fn attribute_agreement(data: &AgreementRatings, options: &AgreementOptions) -> Result<AttributeAgreement> {
    let (c, n, k) = (data.categories.len(), data.parts.len(), data.appraisers.len());
    let level = options.confidence;
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    if c < 2 {
        return Err(invalid("the ratings need at least two categories"));
    }
    if n < 2 {
        return Err(invalid("the study needs at least two parts"));
    }
    if k == 0 || data.ratings.len() != k {
        return Err(invalid("the study needs at least one appraiser"));
    }
    let m = data.trials();
    if m == 0 || data.ratings.iter().any(|a| a.len() != n || a.iter().any(|t| t.len() != m)) {
        return Err(invalid("the study must be balanced: every appraiser rates every part the same number of times"));
    }
    if data.ratings.iter().flatten().flatten().any(|&v| v >= c) {
        return Err(invalid("a rating is not one of the categories"));
    }
    if let Some(s) = &data.standard {
        if s.len() != n || s.iter().any(|&v| v >= c) {
            return Err(invalid("the standard needs one category per part"));
        }
    }
    if k == 1 && m == 1 && data.standard.is_none() {
        return Err(invalid("nothing to compare: give a second trial, a second appraiser or the standard ratings"));
    }
    let concord = |sets: Vec<Vec<f64>>| if options.ordinal { kendall_w(&sets).ok() } else { None };
    let mut within = Vec::new();
    if m >= 2 {
        for a in 0..k {
            let parts = &data.ratings[a];
            let matched = parts.iter().filter(|t| t.iter().all(|&v| v == t[0])).count();
            let counts: Vec<Vec<f64>> = parts.iter().map(|t| category_counts(&[t], c)).collect();
            let sets = (0..m).map(|t| parts.iter().map(|r| r[t] as f64).collect()).collect();
            within.push(WithinAppraiser {
                agreement: PercentAgreement::new(matched, n, level)?,
                kappa: fleiss_kappa(&counts)?,
                concordance: concord(sets),
            });
        }
    }
    let mut versus_standard = Vec::new();
    let mut all_versus_standard = None;
    if let Some(s) = &data.standard {
        for a in 0..k {
            versus_standard.push(versus(data, s, &[a], options)?);
        }
        if k >= 2 {
            all_versus_standard = Some(versus(data, s, &(0..k).collect::<Vec<_>>(), options)?);
        }
    }
    let between = if k >= 2 {
        let mut matched = 0;
        let mut counts = Vec::with_capacity(n);
        for p in 0..n {
            let all: Vec<&Vec<usize>> = (0..k).map(|a| &data.ratings[a][p]).collect();
            let first = all[0][0];
            if all.iter().all(|t| t.iter().all(|&v| v == first)) {
                matched += 1;
            }
            counts.push(category_counts(&all, c));
        }
        let sets = (0..k)
            .flat_map(|a| (0..m).map(move |t| (a, t)))
            .map(|(a, t)| (0..n).map(|p| data.ratings[a][p][t] as f64).collect())
            .collect();
        let mut pairwise = Vec::new();
        for a in 0..k {
            for b in a + 1..k {
                let mut table = vec![vec![0.0; c]; c];
                for p in 0..n {
                    for t in 0..m {
                        table[data.ratings[a][p][t]][data.ratings[b][p][t]] += 1.0;
                    }
                }
                pairwise.push(PairwiseKappa {
                    first: a,
                    second: b,
                    kappa: cohen_kappa(&table, KappaWeights::None, level)?,
                });
            }
        }
        Some(BetweenAppraisers {
            agreement: PercentAgreement::new(matched, n, level)?,
            kappa: fleiss_kappa(&counts)?,
            concordance: concord(sets),
            pairwise,
        })
    } else {
        None
    };
    Ok(AttributeAgreement { within, versus_standard, between, all_versus_standard, trials: m, options: *options })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} vs {b}");
    }

    /// Cohen (1960), Table 1 (proportions of 200 judgments): p_o = .70,
    /// p_c = .41, κ = .29/.59 = .492.
    #[test]
    fn cohen_1960_example() {
        let t = vec![vec![88.0, 14.0, 18.0], vec![10.0, 40.0, 10.0], vec![2.0, 6.0, 12.0]];
        let k = cohen_kappa(&t, KappaWeights::None, 0.95).unwrap();
        close(k.kappa, 0.29 / 0.59, 1e-12);
    }

    #[test]
    fn perfect_and_undefined_kappa() {
        let t = vec![vec![5.0, 0.0], vec![0.0, 7.0]];
        let k = cohen_kappa(&t, KappaWeights::None, 0.95).unwrap();
        close(k.kappa, 1.0, 1e-12);
        let t = vec![vec![5.0, 0.0], vec![0.0, 0.0]];
        assert!(cohen_kappa(&t, KappaWeights::None, 0.95).unwrap().kappa.is_nan());
        assert!(cohen_kappa(&[vec![1.0]], KappaWeights::None, 1.5).is_err());
        assert!(cohen_kappa(&[vec![1.0, 2.0]], KappaWeights::None, 0.95).is_err());
    }

    #[test]
    fn weights() {
        close(KappaWeights::Linear.weight(0, 2, 5), 0.5, 1e-15);
        close(KappaWeights::Quadratic.weight(0, 2, 5), 0.75, 1e-15);
        close(KappaWeights::None.weight(1, 1, 5), 1.0, 0.0);
    }

    #[test]
    fn concordance_without_ties_by_hand() {
        // Three judges rank four objects identically: W = 1.
        let s = vec![vec![1.0, 2.0, 3.0, 4.0]; 3];
        close(kendall_w(&s).unwrap().w, 1.0, 1e-12);
        // Two judges in exactly opposite orders: W = 0.
        let s = vec![vec![1.0, 2.0, 3.0], vec![3.0, 2.0, 1.0]];
        close(kendall_w(&s).unwrap().w, 0.0, 1e-12);
        assert!(kendall_w(&[vec![1.0, 2.0]]).is_err());
        assert!(kendall_w(&[vec![1.0, 1.0], vec![2.0, 2.0]]).unwrap().w.is_nan());
    }

    #[test]
    fn unbalanced_or_empty_studies_are_refused() {
        let base = AgreementRatings {
            categories: vec!["F".into(), "P".into()],
            appraisers: vec!["A".into()],
            parts: vec!["1".into(), "2".into()],
            ratings: vec![vec![vec![0, 0], vec![1, 1]]],
            standard: None,
        };
        let o = AgreementOptions::default();
        let r = attribute_agreement(&base, &o).unwrap();
        assert_eq!(r.within.len(), 1);
        assert!(r.between.is_none() && r.versus_standard.is_empty());
        close(r.within[0].agreement.percent, 100.0, 1e-12);
        let mut bad = base.clone();
        bad.ratings[0][1].pop();
        assert!(attribute_agreement(&bad, &o).is_err());
        let mut bad = base.clone();
        bad.ratings[0][1][0] = 2;
        assert!(attribute_agreement(&bad, &o).is_err());
        let mut bad = base.clone();
        bad.ratings = vec![vec![vec![0], vec![1]]];
        assert!(attribute_agreement(&bad, &o).is_err(), "nothing to compare");
        let mut bad = base;
        bad.standard = Some(vec![0]);
        assert!(attribute_agreement(&bad, &o).is_err());
    }
}
