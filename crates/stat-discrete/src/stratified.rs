//! Stratified analysis of contingency tables: a set of `K` tables (one per
//! stratum) cross-classifying the same row and column variables.
//!
//! # 2 × 2 × K tables
//! Write stratum `k`'s table as `[[a, b], [c, d]]` with `n = a + b + c + d`,
//! row totals `n₁ = a + b`, `n₂ = c + d` and column totals `m₁ = a + c`,
//! `m₂ = b + d`.
//!
//! * **Cochran–Mantel–Haenszel test** of conditional independence (Mantel &
//!   Haenszel 1959; Cochran 1954; Agresti, *Categorical Data Analysis*, 3rd ed., 2013):
//!   with `E(a) = n₁m₁/n` and the hypergeometric variance
//!   `Var(a) = n₁n₂m₁m₂ / (n²(n − 1))`,
//!   `CMH = (Σ(a − E(a)))² / Σ Var(a)` on 1 degree of freedom, and the
//!   continuity-corrected form `(max(0, |Σ(a − E(a))| − ½))² / Σ Var(a)`.
//!   Strata with `n < 2` carry no information and are skipped.
//! * **Mantel–Haenszel common odds ratio** `ψ̂ = Σ(ad/n) / Σ(bc/n)` with the
//!   Robins, Breslow & Greenland (1986) variance of `ln ψ̂`: with
//!   `P = (a + d)/n`, `Q = (b + c)/n`, `R = ad/n`, `S = bc/n`,
//!   `Var = ΣPR/(2R₊²) + Σ(PS + QR)/(2R₊S₊) + ΣQS/(2S₊²)`, and the interval
//!   `exp(ln ψ̂ ± z √Var)`.
//! * **Breslow–Day test** of a common odds ratio (Breslow & Day 1980, §4.4):
//!   in each stratum, `ã` solves `ã(n₂ − m₁ + ã) = ψ̂ (n₁ − ã)(m₁ − ã)` within
//!   the margins' bounds, `Var(ã) = (1/ã + 1/(n₁ − ã) + 1/(m₁ − ã) +
//!   1/(n₂ − m₁ + ã))⁻¹`, and `BD = Σ(a − ã)²/Var(ã)` on `K − 1` degrees of
//!   freedom (strata with a zero margin are left out). Tarone's (1985)
//!   correction subtracts `(Σ(a − ã))² / ΣVar(ã)`.
//!
//! # r × c × K tables: generalized CMH statistics
//! Landis, Heyman & Koch (1978), as presented by Agresti (2013). In stratum `h`, with
//! `nₕ` observations, row proportions `p = Rₕ/nₕ` and column proportions
//! `q = Cₕ/nₕ`, the cell counts `Nₕ` (vectorized by rows) have null mean
//! `mₕ = nₕ (p ⊗ q)` and covariance
//! `Vₕ = nₕ²/(nₕ − 1) (Dₚ − ppᵀ) ⊗ (D_q − qqᵀ)`. For a contrast matrix
//! `B = A_r ⊗ A_c`, `Q = gᵀ (B ΣVₕ Bᵀ)⁻¹ g` with `g = Σ B(Nₕ − mₕ)`:
//!
//! * **general association** (rows and columns both nominal):
//!   `A_r = [I, 0]`, `A_c = [I, 0]`, `(r − 1)(c − 1)` degrees of freedom;
//! * **row mean scores differ** (ordinal columns with scores `a`):
//!   `A_r = [I, 0]`, `A_c = aᵀ`, `r − 1` degrees of freedom;
//! * **nonzero correlation** (both ordinal, row scores `u`):
//!   `A_r = uᵀ`, `A_c = aᵀ`, 1 degree of freedom.
//!
//! For 2 × 2 × K tables all three equal the uncorrected CMH statistic.

use stat_core::distributions::{ChiSquared, ContinuousDistribution, Normal};
use stat_core::{Result, StatError};

/// A chi-square statistic with its degrees of freedom and upper-tail p-value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChiSquareTest {
    /// The statistic.
    pub statistic: f64,
    /// Degrees of freedom.
    pub df: usize,
    /// Upper-tail p-value.
    pub p_value: f64,
}

impl ChiSquareTest {
    fn new(statistic: f64, df: usize) -> Result<Self> {
        let statistic = statistic.max(0.0);
        let p_value = ChiSquared::new(df as f64)?.sf(statistic)?;
        Ok(Self { statistic, df, p_value })
    }
}

/// One stratum's 2 × 2 table `[[a, b], [c, d]]`.
pub type TwoByTwo = [[u64; 2]; 2];

/// Mantel–Haenszel analysis of a set of 2 × 2 tables.
#[derive(Debug, Clone, PartialEq)]
pub struct MantelHaenszel {
    /// Strata with at least two observations.
    pub strata_used: usize,
    /// Cochran–Mantel–Haenszel statistic without continuity correction.
    pub cmh: ChiSquareTest,
    /// With the continuity correction of ½.
    pub cmh_corrected: ChiSquareTest,
    /// Mantel–Haenszel common odds ratio (0 or ∞ when one sum is zero).
    pub odds_ratio: f64,
    /// Robins–Breslow–Greenland standard error of `ln ψ̂` (`None` when the
    /// estimate is 0 or ∞).
    pub log_odds_ratio_se: Option<f64>,
    /// Confidence limits of the common odds ratio (`None` with the SE).
    pub interval: Option<(f64, f64)>,
    /// Confidence level of the interval.
    pub confidence: f64,
    /// Odds ratio of the table pooled over strata (ignoring them).
    pub crude_odds_ratio: f64,
    /// Sample odds ratio `ad/(bc)` of each stratum (NaN when `ad = bc = 0`).
    pub stratum_odds_ratios: Vec<f64>,
    /// Breslow–Day test of homogeneous odds ratios (`None` with fewer than
    /// two informative strata or no finite positive common odds ratio).
    pub breslow_day: Option<ChiSquareTest>,
    /// Breslow–Day with Tarone's correction.
    pub breslow_day_tarone: Option<ChiSquareTest>,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn ratio(num: f64, den: f64) -> f64 {
    if num == 0.0 && den == 0.0 { f64::NAN } else { num / den }
}

/// Mantel–Haenszel analysis of 2 × 2 × K tables (see the module notes).
///
/// # Errors
/// No strata, a confidence level outside (0, 1), or no stratum with both
/// margins varying (the CMH variance is zero).
pub fn mantel_haenszel(strata: &[TwoByTwo], confidence: f64) -> Result<MantelHaenszel> {
    if strata.is_empty() {
        return Err(invalid("the analysis needs at least one stratum"));
    }
    if !(confidence > 0.0 && confidence < 1.0) {
        return Err(invalid("the confidence level must be between 0 and 1"));
    }
    let (mut dev, mut var, mut used) = (0.0, 0.0, 0usize);
    let (mut r_sum, mut s_sum) = (0.0, 0.0);
    let (mut pr, mut ps_qr, mut qs) = (0.0, 0.0, 0.0);
    let mut pooled = [[0u64; 2]; 2];
    let mut stratum_odds_ratios = Vec::with_capacity(strata.len());
    for t in strata {
        let [[a, b], [c, d]] = t.map(|r| r.map(|x| x as f64));
        for i in 0..2 {
            for j in 0..2 {
                pooled[i][j] += t[i][j];
            }
        }
        stratum_odds_ratios.push(ratio(a * d, b * c));
        let n = a + b + c + d;
        if n < 2.0 {
            continue;
        }
        used += 1;
        let (n1, n2, m1, m2) = (a + b, c + d, a + c, b + d);
        dev += a - n1 * m1 / n;
        var += n1 * n2 * m1 * m2 / (n * n * (n - 1.0));
        let (p, q, r, s) = ((a + d) / n, (b + c) / n, a * d / n, b * c / n);
        r_sum += r;
        s_sum += s;
        pr += p * r;
        ps_qr += p * s + q * r;
        qs += q * s;
    }
    if var <= 0.0 {
        return Err(invalid("no stratum has both of its row and column totals varying; the CMH test is undefined"));
    }
    let cmh = ChiSquareTest::new(dev * dev / var, 1)?;
    let corrected = (dev.abs() - 0.5).max(0.0);
    let cmh_corrected = ChiSquareTest::new(corrected * corrected / var, 1)?;
    let odds_ratio = ratio(r_sum, s_sum);
    let (log_odds_ratio_se, interval) = if r_sum > 0.0 && s_sum > 0.0 {
        let v = pr / (2.0 * r_sum * r_sum) + ps_qr / (2.0 * r_sum * s_sum) + qs / (2.0 * s_sum * s_sum);
        let se = v.sqrt();
        let z = Normal::standard().quantile(0.5 + confidence / 2.0)?;
        let l = odds_ratio.ln();
        (Some(se), Some(((l - z * se).exp(), (l + z * se).exp())))
    } else {
        (None, None)
    };
    let [[pa, pb], [pc, pd]] = pooled.map(|r| r.map(|x| x as f64));
    let crude_odds_ratio = ratio(pa * pd, pb * pc);
    let (breslow_day, breslow_day_tarone) = if odds_ratio.is_finite() && odds_ratio > 0.0 {
        breslow_day(strata, odds_ratio)?.map_or((None, None), |(a, b)| (Some(a), Some(b)))
    } else {
        (None, None)
    };
    Ok(MantelHaenszel {
        strata_used: used,
        cmh,
        cmh_corrected,
        odds_ratio,
        log_odds_ratio_se,
        interval,
        confidence,
        crude_odds_ratio,
        stratum_odds_ratios,
        breslow_day,
        breslow_day_tarone,
    })
}

/// Expected `a` of a 2 × 2 table with margins `n₁, n₂, m₁` and odds ratio `psi`.
fn fitted_cell(n1: f64, n2: f64, m1: f64, psi: f64) -> f64 {
    let (lo, hi) = ((m1 - n2).max(0.0), n1.min(m1));
    let qa = 1.0 - psi;
    let qb = n2 - m1 + psi * (n1 + m1);
    let qc = -psi * n1 * m1;
    if qa.abs() < 1e-12 {
        return (-qc / qb).clamp(lo, hi);
    }
    let disc = (qb * qb - 4.0 * qa * qc).max(0.0).sqrt();
    let q = -0.5 * (qb + qb.signum() * disc);
    let roots = [q / qa, if q == 0.0 { f64::NAN } else { qc / q }];
    let tol = 1e-9 * (1.0 + hi);
    roots
        .into_iter()
        .filter(|r| r.is_finite() && *r >= lo - tol && *r <= hi + tol)
        .map(|r| r.clamp(lo, hi))
        .next()
        .unwrap_or(f64::NAN)
}

/// Breslow–Day and Tarone-corrected statistics, `None` with fewer than two
/// strata whose margins are all positive.
fn breslow_day(strata: &[TwoByTwo], psi: f64) -> Result<Option<(ChiSquareTest, ChiSquareTest)>> {
    let (mut bd, mut dev, mut var, mut k) = (0.0, 0.0, 0.0, 0usize);
    for t in strata {
        let [[a, b], [c, d]] = t.map(|r| r.map(|x| x as f64));
        let (n1, n2, m1, m2) = (a + b, c + d, a + c, b + d);
        if n1 == 0.0 || n2 == 0.0 || m1 == 0.0 || m2 == 0.0 {
            continue;
        }
        let e = fitted_cell(n1, n2, m1, psi);
        let v = 1.0 / (1.0 / e + 1.0 / (n1 - e) + 1.0 / (m1 - e) + 1.0 / (n2 - m1 + e));
        if !(v.is_finite() && v > 0.0) {
            continue;
        }
        k += 1;
        bd += (a - e) * (a - e) / v;
        dev += a - e;
        var += v;
    }
    if k < 2 {
        return Ok(None);
    }
    let plain = ChiSquareTest::new(bd, k - 1)?;
    let tarone = ChiSquareTest::new(bd - dev * dev / var, k - 1)?;
    Ok(Some((plain, tarone)))
}

/// Generalized Cochran–Mantel–Haenszel statistics of an r × c × K table.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneralizedCmh {
    /// Nonzero correlation (1 df; row and column scores).
    pub correlation: Option<ChiSquareTest>,
    /// Row mean scores differ (`r − 1` df; column scores).
    pub row_mean_scores: Option<ChiSquareTest>,
    /// General association (`(r − 1)(c − 1)` df).
    pub general_association: Option<ChiSquareTest>,
    /// Strata with at least two observations.
    pub strata_used: usize,
    /// Rows and columns with a positive total over all strata (the others
    /// are dropped, with their scores).
    pub dimensions: (usize, usize),
}

/// Generalized CMH statistics (see the module notes). Each stratum is an
/// `r × c` grid of counts; `row_scores` and `column_scores` give one score
/// per row and column (for example `1, 2, …`). A statistic whose covariance
/// matrix is singular is `None`.
///
/// # Errors
/// No strata, tables of different shapes, scores of the wrong length or not
/// finite, or fewer than two rows or columns with a positive total.
pub fn generalized_cmh(strata: &[Vec<Vec<u64>>], row_scores: &[f64], column_scores: &[f64]) -> Result<GeneralizedCmh> {
    let r = strata.first().map_or(0, Vec::len);
    let c = strata.first().and_then(|t| t.first()).map_or(0, Vec::len);
    if strata.is_empty() || r == 0 || c == 0 {
        return Err(invalid("the analysis needs at least one stratum"));
    }
    if strata.iter().any(|t| t.len() != r || t.iter().any(|row| row.len() != c)) {
        return Err(invalid("every stratum's table must have the same rows and columns"));
    }
    if row_scores.len() != r || column_scores.len() != c {
        return Err(invalid("there must be one score per row and per column"));
    }
    if row_scores.iter().chain(column_scores).any(|s| !s.is_finite()) {
        return Err(invalid("scores must be finite numbers"));
    }
    // Strata with fewer than two observations carry no information; drop them
    // before choosing the non-empty rows and columns, or a level seen only in
    // such a stratum leaves an all-zero margin that makes the covariance
    // singular.
    let used: Vec<&Vec<Vec<u64>>> = strata.iter().filter(|t| t.iter().flatten().sum::<u64>() >= 2).collect();
    let keep_r: Vec<usize> = (0..r).filter(|&i| used.iter().any(|t| t[i].iter().any(|&x| x > 0))).collect();
    let keep_c: Vec<usize> = (0..c).filter(|&j| used.iter().any(|t| t.iter().any(|row| row[j] > 0))).collect();
    let (r, c) = (keep_r.len(), keep_c.len());
    if r < 2 || c < 2 {
        return Err(invalid("the tables need at least two non-empty rows and two non-empty columns"));
    }
    let tables: Vec<Vec<Vec<f64>>> = used
        .iter()
        .map(|t| keep_r.iter().map(|&i| keep_c.iter().map(|&j| t[i][j] as f64).collect()).collect())
        .collect();
    let u: Vec<f64> = keep_r.iter().map(|&i| row_scores[i]).collect();
    let a: Vec<f64> = keep_c.iter().map(|&j| column_scores[j]).collect();
    let first = |k: usize| -> Vec<Vec<f64>> {
        (0..k - 1).map(|i| (0..k).map(|j| f64::from(u8::from(i == j))).collect()).collect()
    };
    let stat = |ar: &[Vec<f64>], ac: &[Vec<f64>]| -> Result<Option<ChiSquareTest>> {
        let df = ar.len() * ac.len();
        Ok(match contrast_statistic(&tables, ar, ac) {
            Some(q) => Some(ChiSquareTest::new(q, df)?),
            None => None,
        })
    };
    Ok(GeneralizedCmh {
        correlation: stat(std::slice::from_ref(&u), std::slice::from_ref(&a))?,
        row_mean_scores: stat(&first(r), std::slice::from_ref(&a))?,
        general_association: stat(&first(r), &first(c))?,
        strata_used: tables.len(),
        dimensions: (r, c),
    })
}

/// `A S Aᵀ` for `S = diag(p) − ppᵀ`.
fn contrast_covariance(a: &[Vec<f64>], p: &[f64]) -> Vec<Vec<f64>> {
    let ap: Vec<f64> = a.iter().map(|row| row.iter().zip(p).map(|(x, y)| x * y).sum()).collect();
    a.iter()
        .enumerate()
        .map(|(i, ri)| {
            a.iter()
                .enumerate()
                .map(|(j, rj)| ri.iter().zip(rj).zip(p).map(|((x, y), w)| x * y * w).sum::<f64>() - ap[i] * ap[j])
                .collect()
        })
        .collect()
}

/// `gᵀ V⁻¹ g` for the contrasts `A_r ⊗ A_c`, or `None` when `V` is singular.
fn contrast_statistic(tables: &[Vec<Vec<f64>>], ar: &[Vec<f64>], ac: &[Vec<f64>]) -> Option<f64> {
    let (dr, dc) = (ar.len(), ac.len());
    let dim = dr * dc;
    let mut g = vec![0.0; dim];
    let mut v = vec![vec![0.0; dim]; dim];
    for t in tables {
        let r = t.len();
        let c = t[0].len();
        let rows: Vec<f64> = t.iter().map(|row| row.iter().sum()).collect();
        let cols: Vec<f64> = (0..c).map(|j| t.iter().map(|row| row[j]).sum()).collect();
        let n: f64 = rows.iter().sum();
        // Deviations from the null means.
        let dev: Vec<Vec<f64>> = (0..r).map(|i| (0..c).map(|j| t[i][j] - rows[i] * cols[j] / n).collect()).collect();
        for (x, arx) in ar.iter().enumerate() {
            for (y, acy) in ac.iter().enumerate() {
                let mut s = 0.0;
                for i in 0..r {
                    for j in 0..c {
                        s += arx[i] * dev[i][j] * acy[j];
                    }
                }
                g[x * dc + y] += s;
            }
        }
        let p: Vec<f64> = rows.iter().map(|x| x / n).collect();
        let q: Vec<f64> = cols.iter().map(|x| x / n).collect();
        let sr = contrast_covariance(ar, &p);
        let sc = contrast_covariance(ac, &q);
        let f = n * n / (n - 1.0);
        for x1 in 0..dr {
            for y1 in 0..dc {
                for x2 in 0..dr {
                    for y2 in 0..dc {
                        v[x1 * dc + y1][x2 * dc + y2] += f * sr[x1][x2] * sc[y1][y2];
                    }
                }
            }
        }
    }
    quadratic_form(&v, &g)
}

/// `gᵀ V⁻¹ g` by Cholesky factorization; `None` when `V` is not positive
/// definite (relative to its largest diagonal entry).
fn quadratic_form(v: &[Vec<f64>], g: &[f64]) -> Option<f64> {
    let n = g.len();
    let scale = (0..n).map(|i| v[i][i].abs()).fold(0.0, f64::max);
    if scale <= 0.0 {
        return None;
    }
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let s: f64 = v[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                if s <= 1e-10 * scale {
                    return None;
                }
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    // Solve L y = g; the form is |y|².
    let mut y = vec![0.0; n];
    for i in 0..n {
        y[i] = (g[i] - (0..i).map(|k| l[i][k] * y[k]).sum::<f64>()) / l[i][i];
    }
    Some(y.iter().map(|x| x * x).sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_cell_reproduces_the_odds_ratio() {
        let (n1, n2, m1) = (10.0, 12.0, 9.0);
        for psi in [0.2, 1.0, 3.5] {
            let a = fitted_cell(n1, n2, m1, psi);
            let or = a * (n2 - m1 + a) / ((n1 - a) * (m1 - a));
            assert!((or - psi).abs() < 1e-9 * psi, "{psi}: {or}");
        }
    }

    #[test]
    fn general_association_of_2x2_strata_equals_cmh() {
        let strata: Vec<TwoByTwo> = vec![[[3, 5], [2, 8]], [[6, 1], [4, 4]], [[2, 2], [1, 6]]];
        let mh = mantel_haenszel(&strata, 0.95).unwrap();
        let general: Vec<Vec<Vec<u64>>> = strata.iter().map(|t| t.iter().map(|r| r.to_vec()).collect()).collect();
        let g = generalized_cmh(&general, &[1.0, 2.0], &[1.0, 2.0]).unwrap();
        for s in [g.correlation, g.row_mean_scores, g.general_association] {
            let s = s.unwrap();
            assert!((s.statistic - mh.cmh.statistic).abs() < 1e-10);
            assert_eq!(s.df, 1);
        }
    }

    #[test]
    fn invalid_inputs_are_errors() {
        assert!(mantel_haenszel(&[], 0.95).is_err());
        assert!(mantel_haenszel(&[[[1, 2], [3, 4]]], 1.5).is_err());
        assert!(mantel_haenszel(&[[[3, 0], [4, 0]]], 0.95).is_err(), "a constant column");
        assert!(generalized_cmh(&[vec![vec![1, 2], vec![3, 4]]], &[1.0], &[1.0, 2.0]).is_err());
        assert!(generalized_cmh(&[vec![vec![1, 2], vec![3, 4]], vec![vec![1, 2]]], &[1.0, 2.0], &[1.0, 2.0]).is_err());
    }

    #[test]
    fn zero_sum_gives_infinite_odds_ratio_without_interval() {
        let mh = mantel_haenszel(&[[[3, 0], [1, 4]], [[2, 0], [2, 3]]], 0.95).unwrap();
        assert!(mh.odds_ratio.is_infinite());
        assert!(mh.interval.is_none() && mh.breslow_day.is_none());
    }

    /// A row level seen only in a one-observation stratum used to stay in the
    /// table with an all-zero margin, leaving two of the three statistics
    /// undefined. With that stratum dropped first, the remaining 2 × 2
    /// stratum [[8, 2], [2, 8]] gives (n − 1)/n × Pearson χ² = 19/20 × 7.2.
    #[test]
    fn small_strata_dropped_before_choosing_levels() {
        let g = generalized_cmh(
            &[vec![vec![8, 2], vec![2, 8], vec![0, 0]], vec![vec![0, 0], vec![0, 0], vec![1, 0]]],
            &[1.0, 2.0, 3.0],
            &[1.0, 2.0],
        )
        .unwrap();
        assert_eq!((g.dimensions, g.strata_used), ((2, 2), 1));
        for t in [g.correlation, g.row_mean_scores, g.general_association] {
            let t = t.unwrap();
            assert!((t.statistic - 6.84).abs() < 1e-12, "{}", t.statistic);
            assert_eq!(t.df, 1);
        }
    }
}
