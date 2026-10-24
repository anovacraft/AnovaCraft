//! Repeated-measures (within-subject) analysis of variance.
//!
//! With `n` subjects each measured under `k` conditions (`yᵢⱼ`), the total sum
//! of squares splits into conditions `SS_C = nΣ(ȳ·ⱼ − ȳ)²`, subjects
//! `SS_S = kΣ(ȳᵢ· − ȳ)²` and error `SS_E` (the subject × condition
//! interaction). `F = (SS_C/(k−1)) / (SS_E/((n−1)(k−1)))`.
//!
//! The F test assumes sphericity: equal variances of every difference between
//! conditions. Mauchly's (1940) test checks it, and two estimates of the
//! departure `ε` multiply both degrees of freedom: Greenhouse–Geisser (1959)
//! `ε̂ = tr(E)² / (d · tr(E²))`, from the error sums of squares and products
//! `E` of `d` orthonormal contrasts among the conditions (`ε̂ = 1` under
//! sphericity and `1/d` at worst), and Huynh–Feldt (1976) with Lecoutre's
//! (1991) correction, `ε̃ = ((ν + 1) d ε̂ − 2) / (d (ν − d ε̂))` for `ν` error
//! degrees of freedom, used capped at 1 (Maxwell & Delaney, *Designing
//! Experiments and Analyzing Data*, 2nd ed., ch. 11–12).
//!
//! [`factorial_repeated_measures`] handles several within-subject factors
//! and between-subject factors by the multivariate formulation of the
//! univariate tests (O'Brien & Kaiser 1985, *Psychol. Bull.* 97): each
//! within-subject effect `W` turns every subject's cells into `d_W`
//! orthonormal contrast scores `Z = Y C_W`; a linear model of `Z` on the
//! between-subject factors (effect coding, Type III hypotheses) gives each
//! between term's hypothesis matrix `H` and the residual matrix `E`, and the
//! univariate test is `F = (tr H / (q d_W)) / (tr E / (ν d_W))` on
//! `(q d_W, ν d_W)` degrees of freedom, with `ν = n − p` and `q` the term's
//! columns. The empty within effect (`C` = the normalised sum) tests the
//! between-subject terms against subjects within groups.

use crate::common::{check_finite, invalid};
use stat_core::Result;
use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor};
use stat_core::eigen::{cholesky, symmetric_eigen};

/// Mauchly's test of sphericity and the two estimates of `ε` for one
/// within-subject effect with `d ≥ 2` contrast dimensions.
#[derive(Debug, Clone, PartialEq)]
pub struct Sphericity {
    /// Mauchly's `W = det(E) / (tr(E)/d)^d` (NaN when `E` is singular, e.g.
    /// with fewer error degrees of freedom than dimensions).
    pub mauchly_w: f64,
    /// `−ν ρ ln W` with `ρ = 1 − (2d² + d + 2)/(6dν)`.
    pub chi_square: f64,
    /// `d(d + 1)/2 − 1`.
    pub df: f64,
    /// Upper-tail p-value with Anderson's second-order term
    /// (*An Introduction to Multivariate Statistical Analysis*, 3rd ed.,
    /// §10.7.4).
    pub p_value: f64,
    /// Greenhouse–Geisser `ε̂`.
    pub greenhouse_geisser: f64,
    /// Huynh–Feldt `ε̃` (Lecoutre's correction) before capping at 1.
    pub huynh_feldt: f64,
}

impl Sphericity {
    /// `ε` used for the Huynh–Feldt test: `min(ε̃, 1)`.
    #[must_use]
    pub fn huynh_feldt_capped(&self) -> f64 {
        self.huynh_feldt.min(1.0)
    }
}

/// Sphericity statistics of an error sums-of-squares-and-products matrix `e`
/// (`d × d`, from orthonormal contrasts) on `nu` degrees of freedom; `None`
/// for `d < 2` or a matrix without positive trace.
#[must_use]
pub fn sphericity(e: &[Vec<f64>], nu: f64) -> Option<Sphericity> {
    let d = e.len();
    if d < 2 {
        return None;
    }
    let (values, _) = symmetric_eigen(e).ok()?;
    let trace: f64 = values.iter().sum();
    let trace_sq: f64 = values.iter().map(|v| v * v).sum();
    if !(trace > 0.0 && trace_sq > 0.0) {
        return None;
    }
    let df = d as f64;
    let gg = (trace * trace / (df * trace_sq)).clamp(1.0 / df, 1.0);
    let hf = ((nu + 1.0) * df * gg - 2.0) / (df * (nu - df * gg));
    let top = values[0];
    let (w, chi, p) = if values.iter().all(|&v| v > 1e-12 * top) && nu > 0.0 {
        let log_w = values.iter().map(|v| v.ln()).sum::<f64>() - df * (trace / df).ln();
        let rho = 1.0 - (2.0 * df * df + df + 2.0) / (6.0 * df * nu);
        let w2 = (df + 2.0) * (df - 1.0) * (df - 2.0) * (2.0 * df.powi(3) + 6.0 * df * df + 3.0 * df + 2.0)
            / (288.0 * (nu * df * rho).powi(2));
        let z = -nu * rho * log_w;
        let f = df * (df + 1.0) / 2.0 - 1.0;
        let tail = |k: f64| ChiSquared::new(k).and_then(|c| c.sf(z.max(0.0))).unwrap_or(f64::NAN);
        let (p1, p2) = (tail(f), tail(f + 4.0));
        (log_w.exp(), z, (p1 + w2 * (p2 - p1)).clamp(0.0, 1.0))
    } else {
        (f64::NAN, f64::NAN, f64::NAN)
    };
    Some(Sphericity {
        mauchly_w: w,
        chi_square: chi,
        df: df * (df + 1.0) / 2.0 - 1.0,
        p_value: p,
        greenhouse_geisser: gg,
        huynh_feldt: if hf.is_finite() && hf > 0.0 { hf } else { 1.0 },
    })
}

/// Orthonormal (normalised Helmert) contrasts among `l` levels: an
/// `l × (l − 1)` matrix whose columns are orthogonal to the constant and to
/// each other.
#[must_use]
pub fn orthonormal_contrasts(l: usize) -> Vec<Vec<f64>> {
    let m = l.saturating_sub(1);
    let mut c = vec![vec![0.0; m]; l];
    for j in 0..m {
        let norm = (((j + 1) * (j + 2)) as f64).sqrt();
        for row in c.iter_mut().take(j + 1) {
            row[j] = 1.0 / norm;
        }
        c[j + 1][j] = -((j + 1) as f64) / norm;
    }
    c
}

/// Result of a repeated-measures ANOVA.
#[derive(Debug, Clone, PartialEq)]
pub struct RepeatedMeasuresAnova {
    /// Number of subjects.
    pub n_subjects: usize,
    /// Mean of each condition.
    pub condition_means: Vec<f64>,
    /// Conditions sum of squares.
    pub ss_conditions: f64,
    /// Subjects sum of squares.
    pub ss_subjects: f64,
    /// Error (subject × condition) sum of squares.
    pub ss_error: f64,
    /// `k − 1`.
    pub df_conditions: f64,
    /// `n − 1`.
    pub df_subjects: f64,
    /// `(n − 1)(k − 1)`.
    pub df_error: f64,
    /// F statistic.
    pub f_statistic: f64,
    /// p-value assuming sphericity.
    pub p_value: f64,
    /// Greenhouse–Geisser `ε̂`.
    pub greenhouse_geisser: f64,
    /// p-value with Greenhouse–Geisser corrected degrees of freedom.
    pub p_value_gg: f64,
    /// Mauchly's test and both estimates of `ε` (`None` for two conditions,
    /// where sphericity holds trivially).
    pub sphericity: Option<Sphericity>,
    /// p-value with Huynh–Feldt corrected degrees of freedom (`ε̃` capped at 1).
    pub p_value_hf: f64,
}

/// Repeated-measures ANOVA of `data` (one row per subject, one column per
/// condition; no missing values).
///
/// # Errors
/// Fewer than two subjects or conditions, ragged rows, non-finite values, or
/// no residual variation.
pub fn repeated_measures_anova<R: AsRef<[f64]>>(data: &[R]) -> Result<RepeatedMeasuresAnova> {
    let n = data.len();
    let k = data.first().map_or(0, |r| r.as_ref().len());
    if n < 2 || k < 2 {
        return Err(invalid("repeated measures need at least two subjects and two conditions"));
    }
    if data.iter().any(|r| r.as_ref().len() != k) {
        return Err(invalid("every subject needs one value per condition"));
    }
    for r in data {
        check_finite(r.as_ref(), "repeated measures")?;
    }
    let (nf, kf) = (n as f64, k as f64);
    let grand = data.iter().flat_map(|r| r.as_ref().iter()).sum::<f64>() / (nf * kf);
    let col_means: Vec<f64> = (0..k).map(|j| data.iter().map(|r| r.as_ref()[j]).sum::<f64>() / nf).collect();
    let row_means: Vec<f64> = data.iter().map(|r| r.as_ref().iter().sum::<f64>() / kf).collect();
    let ss_c = nf * col_means.iter().map(|m| (m - grand).powi(2)).sum::<f64>();
    let ss_s = kf * row_means.iter().map(|m| (m - grand).powi(2)).sum::<f64>();
    let mut ss_e = 0.0;
    for (i, r) in data.iter().enumerate() {
        for (j, &y) in r.as_ref().iter().enumerate() {
            ss_e += (y - row_means[i] - col_means[j] + grand).powi(2);
        }
    }
    let (df_c, df_e) = (kf - 1.0, (nf - 1.0) * (kf - 1.0));
    if ss_e <= 0.0 {
        return Err(invalid("no residual variation: every subject changes identically"));
    }
    let f = (ss_c / df_c) / (ss_e / df_e);
    let p = FisherSnedecor::new(df_c, df_e)?.sf(f)?;
    // Error sums of squares and products of orthonormal contrasts among the
    // conditions.
    let c = orthonormal_contrasts(k);
    let z: Vec<Vec<f64>> = data
        .iter()
        .map(|r| (0..k - 1).map(|j| (0..k).map(|i| (r.as_ref()[i] - col_means[i]) * c[i][j]).sum()).collect())
        .collect();
    let e: Vec<Vec<f64>> =
        (0..k - 1).map(|a| (0..k - 1).map(|b| z.iter().map(|zi| zi[a] * zi[b]).sum()).collect()).collect();
    let sph = sphericity(&e, nf - 1.0);
    let eps = sph.as_ref().map_or(1.0, |s| s.greenhouse_geisser);
    let eps_hf = sph.as_ref().map_or(1.0, Sphericity::huynh_feldt_capped);
    let p_gg = FisherSnedecor::new(eps * df_c, eps * df_e)?.sf(f)?;
    let p_hf = FisherSnedecor::new(eps_hf * df_c, eps_hf * df_e)?.sf(f)?;
    Ok(RepeatedMeasuresAnova {
        n_subjects: n,
        condition_means: col_means,
        ss_conditions: ss_c,
        ss_subjects: ss_s,
        ss_error: ss_e,
        df_conditions: df_c,
        df_subjects: nf - 1.0,
        df_error: df_e,
        f_statistic: f,
        p_value: p,
        greenhouse_geisser: eps,
        p_value_gg: p_gg,
        sphericity: sph,
        p_value_hf: p_hf,
    })
}

// ───────────────────────────── Factorial designs ─────────────────────────────

/// A between-subject factor: its name, number of levels and each subject's
/// level.
#[derive(Debug, Clone, PartialEq)]
pub struct BetweenFactor {
    /// Factor name.
    pub name: String,
    /// Number of levels.
    pub n_levels: usize,
    /// Level of each subject (`< n_levels`).
    pub codes: Vec<usize>,
}

/// One row of a factorial repeated-measures ANOVA table.
#[derive(Debug, Clone, PartialEq)]
pub struct RepeatedEffect {
    /// Name, factors joined by " × " (between-subject factors first).
    pub name: String,
    /// Between-subject factors in the effect (indices).
    pub between: Vec<usize>,
    /// Within-subject factors in the effect (indices).
    pub within: Vec<usize>,
    /// Effect sum of squares.
    pub ss: f64,
    /// Effect degrees of freedom.
    pub df: f64,
    /// Sum of squares of the effect's error term.
    pub ss_error: f64,
    /// Degrees of freedom of the error term.
    pub df_error: f64,
    /// Name of the error term (e.g. "Subjects", "Time × Subjects").
    pub error_name: String,
    /// F statistic.
    pub f_statistic: f64,
    /// p-value assuming sphericity.
    pub p_value: f64,
    /// Sphericity statistics of the within-subject part (`None` when it has a
    /// single contrast dimension or no within-subject factor).
    pub sphericity: Option<Sphericity>,
    /// p-value with Greenhouse–Geisser corrected degrees of freedom (equal to
    /// `p_value` without a sphericity correction).
    pub p_value_gg: f64,
    /// p-value with Huynh–Feldt corrected degrees of freedom (`ε̃` capped at 1).
    pub p_value_hf: f64,
    /// Partial η² = SS / (SS + SS_error).
    pub partial_eta_squared: f64,
    /// Generalized η² (Olejnik & Algina 2003, every factor manipulated):
    /// SS / (SS + the sum of every error term's SS).
    pub generalized_eta_squared: f64,
}

/// Result of a factorial repeated-measures ANOVA.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorialRepeatedMeasures {
    /// Subjects analysed.
    pub n_subjects: usize,
    /// Residual degrees of freedom of the between-subject model, `ν = n − p`.
    pub residual_df: f64,
    /// Effects: between-subject terms first, then each within-subject effect
    /// followed by its interactions with the between-subject terms.
    pub effects: Vec<RepeatedEffect>,
}

/// Non-empty subsets of `0..m` ordered by size, then lexicographically (the
/// order R lists model terms in).
fn subsets(m: usize) -> Vec<Vec<usize>> {
    let mut all: Vec<Vec<usize>> =
        (1..(1usize << m)).map(|mask| (0..m).filter(|&i| (mask >> i) & 1 == 1).collect()).collect();
    all.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    all
}

/// Inverse of a symmetric positive-definite matrix by Cholesky factorization.
fn spd_inverse(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let l = cholesky(a)?;
    // L⁻¹ (lower triangular), then A⁻¹ = L⁻ᵀ L⁻¹.
    let mut li = vec![vec![0.0; n]; n];
    for j in 0..n {
        for i in j..n {
            let s: f64 = (j..i).map(|k| l[i][k] * li[k][j]).sum();
            li[i][j] = (if i == j { 1.0 } else { 0.0 } - s) / l[i][i];
        }
    }
    Some((0..n).map(|i| (0..n).map(|j| (i.max(j)..n).map(|k| li[k][i] * li[k][j]).sum()).collect()).collect())
}

/// Kronecker product of the per-factor matrices: contrasts for the factors in
/// `effect`, the normalised constant for the others (`k × d` overall).
fn within_contrasts(levels: &[usize], effect: &[usize]) -> Vec<Vec<f64>> {
    let mut c: Vec<Vec<f64>> = vec![vec![1.0]];
    for (f, &l) in levels.iter().enumerate() {
        let factor: Vec<Vec<f64>> =
            if effect.contains(&f) { orthonormal_contrasts(l) } else { vec![vec![1.0 / (l as f64).sqrt()]; l] };
        let mut next = Vec::with_capacity(c.len() * l);
        for row in &c {
            for frow in &factor {
                next.push(row.iter().flat_map(|&a| frow.iter().map(move |&b| a * b)).collect::<Vec<f64>>());
            }
        }
        c = next;
    }
    c
}

/// One within-subject effect's error matrix and between-term hypotheses.
struct Stratum {
    within: Vec<usize>,
    error: Vec<Vec<f64>>,
    /// `(between term, or None for the intercept; SS; columns of the term)`.
    hypotheses: Vec<(Option<usize>, f64, f64)>,
}

/// Factorial repeated-measures ANOVA. `data` has one row per subject and one
/// column per within-subject cell, cells ordered with the last within factor
/// varying fastest (`within_levels` gives each within factor's number of
/// levels; their product is the row length, 1 with no within factor).
/// `between` holds the between-subject factors; every combination of their
/// levels needs at least one subject. Between-subject terms are tested by
/// Type III hypotheses under effect coding, which equal the sequential
/// (Type I/II) ones when every between-subject cell has the same number of
/// subjects.
///
/// # Errors
/// Mismatched names or row lengths, non-finite values, factors with fewer
/// than two levels, no factor, an empty between-subject cell, no residual
/// degrees of freedom, or no residual variation.
pub fn factorial_repeated_measures<R: AsRef<[f64]>>(
    data: &[R],
    within_names: &[String],
    within_levels: &[usize],
    between: &[BetweenFactor],
) -> Result<FactorialRepeatedMeasures> {
    let n = data.len();
    if within_names.len() != within_levels.len() {
        return Err(invalid("one name per within-subject factor"));
    }
    if within_levels.iter().any(|&l| l < 2) || between.iter().any(|b| b.n_levels < 2) {
        return Err(invalid("every factor needs at least two levels"));
    }
    if within_levels.is_empty() && between.is_empty() {
        return Err(invalid("choose at least one factor"));
    }
    let k: usize = within_levels.iter().product();
    if data.iter().any(|r| r.as_ref().len() != k) {
        return Err(invalid("every subject needs one value per within-subject cell"));
    }
    for r in data {
        check_finite(r.as_ref(), "repeated measures")?;
    }
    if between.iter().any(|b| b.codes.len() != n || b.codes.iter().any(|&c| c >= b.n_levels)) {
        return Err(invalid("every subject needs a level of each between-subject factor"));
    }
    // Between-subject design: intercept, then effect-coded terms.
    let b_terms = subsets(between.len());
    let mut x: Vec<Vec<f64>> = vec![vec![1.0]; n];
    let mut term_cols: Vec<std::ops::Range<usize>> = Vec::new();
    for term in &b_terms {
        let start = x.first().map_or(1, Vec::len);
        for (i, row) in x.iter_mut().enumerate() {
            let mut cols = vec![1.0];
            for &f in term {
                let bf = &between[f];
                let code = bf.codes[i];
                let last = bf.n_levels - 1;
                let coding: Vec<f64> = (0..last)
                    .map(|j| {
                        if code == j {
                            1.0
                        } else if code == last {
                            -1.0
                        } else {
                            0.0
                        }
                    })
                    .collect();
                cols = cols.iter().flat_map(|&a| coding.iter().map(move |&b| a * b)).collect();
            }
            row.extend(cols);
        }
        term_cols.push(start..x.first().map_or(1, Vec::len));
    }
    let p = x.first().map_or(1, Vec::len);
    if n <= p {
        return Err(invalid("too few subjects: no residual degrees of freedom"));
    }
    let nu = (n - p) as f64;
    let xtx: Vec<Vec<f64>> = (0..p).map(|a| (0..p).map(|b| x.iter().map(|r| r[a] * r[b]).sum()).collect()).collect();
    let xtx_inv = spd_inverse(&xtx)
        .ok_or_else(|| invalid("every combination of the between-subject factors needs at least one subject"))?;
    // Within-subject effects, the empty effect (between-subject terms) first.
    let mut w_effects: Vec<Vec<usize>> = vec![Vec::new()];
    w_effects.extend(subsets(within_levels.len()));
    let mut strata = Vec::new();
    for w in &w_effects {
        let c = within_contrasts(within_levels, w);
        let d = c.first().map_or(0, Vec::len);
        let z: Vec<Vec<f64>> = data
            .iter()
            .map(|r| (0..d).map(|j| r.as_ref().iter().zip(&c).map(|(y, cr)| y * cr[j]).sum()).collect())
            .collect();
        // B = (XᵀX)⁻¹ Xᵀ Z and the residual sums of squares and products.
        let xtz: Vec<Vec<f64>> =
            (0..p).map(|a| (0..d).map(|j| x.iter().zip(&z).map(|(xr, zr)| xr[a] * zr[j]).sum()).collect()).collect();
        let beta: Vec<Vec<f64>> =
            (0..p).map(|a| (0..d).map(|j| (0..p).map(|b| xtx_inv[a][b] * xtz[b][j]).sum()).collect()).collect();
        let resid: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..d).map(|j| z[i][j] - (0..p).map(|a| x[i][a] * beta[a][j]).sum::<f64>()).collect())
            .collect();
        let error: Vec<Vec<f64>> =
            (0..d).map(|a| (0..d).map(|b| resid.iter().map(|r| r[a] * r[b]).sum()).collect()).collect();
        let mut terms: Vec<(Option<usize>, std::ops::Range<usize>)> = Vec::new();
        if !w.is_empty() {
            terms.push((None, 0..1));
        }
        terms.extend(term_cols.iter().cloned().enumerate().map(|(t, r)| (Some(t), r)));
        let mut hypotheses = Vec::new();
        for (t, cols) in terms {
            let q = cols.len();
            let lb: Vec<&Vec<f64>> = cols.clone().map(|a| &beta[a]).collect();
            let m: Vec<Vec<f64>> = cols.clone().map(|a| cols.clone().map(|b| xtx_inv[a][b]).collect()).collect();
            let m_inv = spd_inverse(&m).ok_or_else(|| invalid("a between-subject term cannot be estimated"))?;
            // tr H = Σⱼ (LB)ⱼᵀ M⁻¹ (LB)ⱼ.
            let ss: f64 = (0..d)
                .map(|j| (0..q).map(|a| (0..q).map(|b| lb[a][j] * m_inv[a][b] * lb[b][j]).sum::<f64>()).sum::<f64>())
                .sum();
            hypotheses.push((t, ss, q as f64));
        }
        strata.push(Stratum { within: w.clone(), error, hypotheses });
    }
    let trace = |m: &[Vec<f64>]| (0..m.len()).map(|a| m[a][a]).sum::<f64>();
    let total_error: f64 = strata.iter().map(|s| trace(&s.error)).sum();
    let join = |b: &[usize], w: &[usize]| -> String {
        b.iter()
            .map(|&i| between[i].name.as_str())
            .chain(w.iter().map(|&i| within_names[i].as_str()))
            .collect::<Vec<_>>()
            .join(" × ")
    };
    let mut effects = Vec::new();
    for st in &strata {
        let d = st.error.len() as f64;
        let ss_error = trace(&st.error);
        if ss_error <= 0.0 {
            return Err(invalid("no residual variation"));
        }
        let df_error = nu * d;
        let sph = sphericity(&st.error, nu);
        let error_name = if st.within.is_empty() {
            if between.is_empty() { "Subjects".to_owned() } else { "Subjects within groups".to_owned() }
        } else {
            format!("{} × Subjects", join(&[], &st.within))
        };
        for &(t, ss, q) in &st.hypotheses {
            let b: Vec<usize> = t.map(|t| b_terms[t].clone()).unwrap_or_default();
            let df = q * d;
            let f = (ss / df) / (ss_error / df_error);
            let tail =
                |e: f64| FisherSnedecor::new(e * df, e * df_error).and_then(|dist| dist.sf(f)).unwrap_or(f64::NAN);
            let p = tail(1.0);
            let (p_gg, p_hf) = match &sph {
                Some(s) => (tail(s.greenhouse_geisser), tail(s.huynh_feldt_capped())),
                None => (p, p),
            };
            effects.push(RepeatedEffect {
                name: join(&b, &st.within),
                between: b,
                within: st.within.clone(),
                ss,
                df,
                ss_error,
                df_error,
                error_name: error_name.clone(),
                f_statistic: f,
                p_value: p,
                sphericity: sph.clone(),
                p_value_gg: p_gg,
                p_value_hf: p_hf,
                partial_eta_squared: ss / (ss + ss_error),
                generalized_eta_squared: ss / (ss + total_error),
            });
        }
    }
    Ok(FactorialRepeatedMeasures { n_subjects: n, residual_df: nu, effects })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::means::tests::close;

    /// Sums of squares, F and ε̂ evaluated with NumPy/SciPy 1.18 from the
    /// definitions above.
    #[test]
    fn rm_anova() {
        let y = [[8.0, 7.2, 6.1], [9.1, 8.5, 7.0], [7.5, 7.9, 6.8], [8.8, 8.0, 7.9], [9.5, 8.1, 7.2], [7.9, 7.0, 7.1]];
        let r = repeated_measures_anova(&y).unwrap();
        close(r.f_statistic, 14.853_633_037_114_585, 1e-12);
        close(r.p_value, 0.001_013_094_761_724_269_6, 1e-9);
        close(r.greenhouse_geisser, 0.942_911_331_676_431_2, 1e-12);
        close(r.p_value_gg, 0.001_336_717_901_066_123_3, 1e-9);
        assert_eq!((r.df_conditions, r.df_error), (2.0, 10.0));
        // Two conditions: the F test is the squared paired t, and ε̂ = 1.
        let pairs: Vec<[f64; 2]> = y.iter().map(|r| [r[0], r[2]]).collect();
        let two = repeated_measures_anova(&pairs).unwrap();
        let a: Vec<f64> = y.iter().map(|r| r[0]).collect();
        let b: Vec<f64> = y.iter().map(|r| r[2]).collect();
        let t = crate::means::paired_t(&a, &b, 0.0, 0.95, crate::Alternative::TwoSided).unwrap();
        close(two.f_statistic, t.statistic * t.statistic, 1e-12);
        close(two.greenhouse_geisser, 1.0, 1e-12);
    }

    /// Mauchly's test and the Huynh–Feldt estimate: R 4.x with car 3.x
    /// (`tests/data/repeated_reference.R`): W = 0.9394548920925,
    /// p = 0.8825754942765, HF ε̃ = 1.4955692905636 (capped at 1 for the test).
    #[test]
    fn sphericity_one_way() {
        let y = [[8.0, 7.2, 6.1], [9.1, 8.5, 7.0], [7.5, 7.9, 6.8], [8.8, 8.0, 7.9], [9.5, 8.1, 7.2], [7.9, 7.0, 7.1]];
        let r = repeated_measures_anova(&y).unwrap();
        let s = r.sphericity.as_ref().unwrap();
        close(s.mauchly_w, 0.939_454_892_092_5, 1e-10);
        close(s.p_value, 0.882_575_494_276_5, 1e-9);
        close(s.huynh_feldt, 1.495_569_290_563_6, 1e-10);
        close(r.p_value_hf, r.p_value, 1e-12);
        assert_eq!(s.df, 2.0);
        // Five conditions: GG 0.694000981145772, HF 1.03470659901185, W
        // 0.1555921611679; the χ² and p use Anderson's second-order term with
        // d = 4 (R's car and mauchly.test write 3p for 3d there: p = 0.13851).
        let y5 = [
            [31.0, 29.0, 25.0, 27.0, 24.0],
            [28.0, 27.0, 26.0, 22.0, 20.0],
            [35.0, 33.0, 30.0, 31.0, 25.0],
            [30.0, 31.0, 26.0, 25.0, 25.0],
            [26.0, 25.0, 25.0, 20.0, 19.0],
            [33.0, 29.0, 30.0, 27.0, 21.0],
            [29.0, 26.0, 27.0, 24.0, 23.0],
            [32.0, 33.0, 27.0, 28.0, 22.0],
            [27.0, 24.0, 23.0, 22.0, 21.0],
            [34.0, 30.0, 31.0, 26.0, 24.0],
        ];
        let r = repeated_measures_anova(&y5).unwrap();
        let s = r.sphericity.as_ref().unwrap();
        close(r.f_statistic, 41.718, 1e-6);
        close(s.greenhouse_geisser, 0.694_000_981_145_772, 1e-10);
        close(s.huynh_feldt, 1.034_706_599_011_85, 1e-10);
        close(s.mauchly_w, 0.155_592_161_167_9, 1e-10);
        close(s.chi_square, 13.798_834_762_323_7, 1e-9);
        close(s.p_value, 0.138_401_081_061_339, 1e-8);
        close(r.p_value_gg, 1.189_815_679_911_13e-9, 1e-7);
        assert_eq!(s.df, 9.0);
    }

    /// O'Brien & Kaiser (1985) data (car's `OBrienKaiser`): treatment (3) ×
    /// gender (2) between subjects, unequal cells; phase (3) × hour (5)
    /// within. R `car::Anova(mlm, idata, idesign = ~phase*hour, type = 3)`
    /// (`tests/data/repeated_reference.R`).
    #[test]
    fn factorial_obrien_kaiser() {
        let raw: [(&str, &str, [f64; 15]); 16] = [
            ("control", "M", [1., 2., 4., 2., 1., 3., 2., 5., 3., 2., 2., 3., 2., 4., 4.]),
            ("control", "M", [4., 4., 5., 3., 4., 2., 2., 3., 5., 3., 4., 5., 6., 4., 1.]),
            ("control", "M", [5., 6., 5., 7., 7., 4., 5., 7., 5., 4., 7., 6., 9., 7., 6.]),
            ("control", "F", [5., 4., 7., 5., 4., 2., 2., 3., 5., 3., 4., 4., 5., 3., 4.]),
            ("control", "F", [3., 4., 6., 4., 3., 6., 7., 8., 6., 3., 4., 3., 6., 4., 3.]),
            ("A", "M", [7., 8., 7., 9., 9., 9., 9., 10., 8., 9., 9., 10., 11., 9., 6.]),
            ("A", "M", [5., 5., 6., 4., 5., 7., 7., 8., 10., 8., 8., 9., 11., 9., 8.]),
            ("A", "F", [2., 3., 5., 3., 2., 2., 4., 8., 6., 5., 6., 6., 7., 5., 6.]),
            ("A", "F", [3., 3., 4., 6., 4., 4., 5., 6., 4., 1., 5., 4., 7., 5., 4.]),
            ("B", "M", [4., 4., 5., 3., 4., 6., 7., 6., 8., 8., 8., 8., 9., 7., 8.]),
            ("B", "M", [3., 3., 4., 2., 3., 5., 4., 7., 5., 4., 5., 6., 8., 6., 5.]),
            ("B", "M", [6., 7., 8., 6., 3., 9., 10., 11., 9., 6., 8., 7., 10., 8., 7.]),
            ("B", "F", [5., 5., 6., 8., 6., 4., 6., 6., 8., 6., 7., 7., 8., 10., 8.]),
            ("B", "F", [2., 2., 3., 1., 2., 5., 6., 7., 5., 2., 6., 7., 8., 6., 3.]),
            ("B", "F", [2., 2., 3., 4., 4., 6., 6., 7., 9., 7., 7., 7., 8., 6., 7.]),
            ("B", "F", [4., 5., 7., 5., 4., 7., 7., 8., 6., 7., 7., 8., 10., 8., 7.]),
        ];
        let data: Vec<Vec<f64>> = raw.iter().map(|r| r.2.to_vec()).collect();
        let treatment = BetweenFactor {
            name: "treatment".into(),
            n_levels: 3,
            codes: raw.iter().map(|r| ["control", "A", "B"].iter().position(|t| *t == r.0).unwrap()).collect(),
        };
        let gender = BetweenFactor {
            name: "gender".into(),
            n_levels: 2,
            codes: raw.iter().map(|r| usize::from(r.1 == "F")).collect(),
        };
        let names = ["phase".to_owned(), "hour".to_owned()];
        let r = factorial_repeated_measures(&data, &names, &[3, 5], &[treatment, gender]).unwrap();
        assert_eq!(r.residual_df, 10.0);
        // (name, SS, df, error SS, error df, F, p)
        let expected: [(&str, f64, f64, f64, f64, f64, f64); 15] = [
            ("treatment", 179.730_332_522, 2.0, 228.055_555_555_56, 10.0, 3.940_49, 0.054_706_9),
            ("gender", 83.448_275_862, 1.0, 228.055_555_555_56, 10.0, 3.659_12, 0.084_800_3),
            ("treatment × gender", 130.241_281_427, 2.0, 228.055_555_555_56, 10.0, 2.855_47, 0.104_469_2),
            ("phase", 129.511_494_253, 2.0, 80.277_777_777_78, 20.0, 16.132_92, 6.7316e-5),
            ("treatment × phase", 77.885_239_254, 4.0, 80.277_777_777_78, 20.0, 4.850_98, 0.006_722_7),
            ("gender × phase", 2.270_114_943, 2.0, 80.277_777_777_78, 20.0, 0.282_78, 0.756_647_3),
            ("treatment × gender × phase", 10.221_005_677, 4.0, 80.277_777_777_78, 20.0, 0.636_602_43, 0.642_369_5),
            ("hour", 104.285_440_613, 4.0, 62.5, 40.0, 16.685_67, 4.0266e-8),
            ("treatment × hour", 1.166_666_667, 8.0, 62.5, 40.0, 0.093_33, 0.999_244_6),
            ("gender × hour", 2.814_176_245, 4.0, 62.5, 40.0, 0.450_27, 0.771_559_1),
            ("treatment × gender × hour", 7.755_474_453, 8.0, 62.5, 40.0, 0.620_44, 0.755_484_4),
            ("phase × hour", 11.346_743_295, 8.0, 96.166_666_666_67, 80.0, 1.179_90, 0.321_586_6),
            ("treatment × phase × hour", 6.641_119_221, 16.0, 96.166_666_666_67, 80.0, 0.345_29, 0.990_124_6),
            ("gender × phase × hour", 8.955_938_697, 8.0, 96.166_666_666_67, 80.0, 0.931_29, 0.495_611_9),
            ("treatment × gender × phase × hour", 14.154_501_217, 16.0, 96.166_666_666_67, 80.0, 0.735_94, 0.749_561_6),
        ];
        assert_eq!(r.effects.len(), expected.len());
        for (e, x) in r.effects.iter().zip(&expected) {
            assert_eq!(e.name, x.0);
            close(e.ss, x.1, 1e-9);
            assert_eq!((e.df, e.df_error), (x.2, x.4));
            close(e.ss_error, x.3, 1e-11);
            close(e.f_statistic, x.5, 1e-4);
            close(e.p_value, x.6, 1e-4);
        }
        let by = |n: &str| r.effects.iter().find(|e| e.name == n).unwrap();
        // Sphericity: (effect, W, GG ε, HF ε, p GG, p HF).
        let checks = [
            (
                "phase",
                0.749_272_638_019_2,
                0.799_534_759_051_138,
                0.927_859_403_950_418,
                2.813_681_244_158_41e-4,
                1.124_742_900_546_32e-4,
            ),
            (
                "treatment × phase",
                0.749_272_638_019_2,
                0.799_534_759_051_138,
                0.927_859_403_950_418,
                1.269_090_436_015_18e-2,
                8.438_775_501_929_79e-3,
            ),
            (
                "hour",
                0.066_066_271_643_6,
                0.460_281_502_256_993,
                0.559_280_181_294_145,
                9.762_880_671_457_42e-5,
                2.300_914_306_289_11e-5,
            ),
            (
                "treatment × gender × hour",
                0.066_066_271_643_6,
                0.460_281_502_256_993,
                0.559_280_181_294_145,
                6.413_624_618_251_30e-1,
                6.692_976_144_664_16e-1,
            ),
            (
                "phase × hour",
                0.004_779_921_354_1,
                0.449_501_257_731_810,
                0.733_060_776_233_725,
                3.345_211_798_539_33e-1,
                3.296_590_033_126_64e-1,
            ),
            (
                "treatment × gender × phase × hour",
                0.004_779_921_354_1,
                0.449_501_257_731_810,
                0.733_060_776_233_725,
                6.463_449_039_921_86e-1,
                7.080_121_616_190_40e-1,
            ),
        ];
        for (name, w, gg, hf, p_gg, p_hf) in checks {
            let e = by(name);
            let s = e.sphericity.as_ref().unwrap();
            close(s.mauchly_w, w, 1e-9);
            close(s.greenhouse_geisser, gg, 1e-10);
            close(s.huynh_feldt, hf, 1e-10);
            close(e.p_value_gg, p_gg, 1e-8);
            close(e.p_value_hf, p_hf, 1e-8);
        }
        // Mauchly p-values: d = 2 has no second-order term, so R's 0.27282 is
        // exact; d = 4 and 8 use Anderson's formula (computed in the R script).
        close(by("phase").sphericity.as_ref().unwrap().p_value, 0.272_822_026_080_209, 1e-8);
        let hour = by("hour").sphericity.clone().unwrap();
        close(hour.chi_square, 22.868_899_118_170_66, 1e-9);
        close(hour.p_value, 0.007_462_920_131_994_96, 1e-8);
        close(by("phase × hour").sphericity.as_ref().unwrap().p_value, 0.447_690_946_616_64, 1e-8);
        assert!(by("treatment").sphericity.is_none());
        assert_eq!(by("gender").error_name, "Subjects within groups");
        assert_eq!(by("phase × hour").error_name, "phase × hour × Subjects");
        // Generalized η²: SS / (SS + every error SS).
        let total = 228.055_555_555_56 + 80.277_777_777_78 + 62.5 + 96.166_666_666_67;
        close(by("hour").generalized_eta_squared, 104.285_440_613 / (104.285_440_613 + total), 1e-9);
    }

    /// With one within factor and no between factor the factorial routine is
    /// the one-way analysis; with no within factor it is a one-way ANOVA.
    #[test]
    fn factorial_special_cases() {
        let y = [[8.0, 7.2, 6.1], [9.1, 8.5, 7.0], [7.5, 7.9, 6.8], [8.8, 8.0, 7.9], [9.5, 8.1, 7.2], [7.9, 7.0, 7.1]];
        let one = repeated_measures_anova(&y).unwrap();
        let f = factorial_repeated_measures(&y, &["C".to_owned()], &[3], &[]).unwrap();
        assert_eq!(f.effects.len(), 1);
        close(f.effects[0].f_statistic, one.f_statistic, 1e-12);
        close(f.effects[0].p_value_gg, one.p_value_gg, 1e-10);
        close(f.effects[0].ss_error, one.ss_error, 1e-12);
        // Between only: three groups of two (means 1.5, 4.5, 8.5):
        // SS_between = 49.3333…, SS_within = 1.5, F = 49.3333….
        let v = [[1.0], [2.0], [4.0], [5.0], [9.0], [8.0]];
        let g = BetweenFactor { name: "G".into(), n_levels: 3, codes: vec![0, 0, 1, 1, 2, 2] };
        let a = factorial_repeated_measures(&v, &[], &[], &[g]).unwrap();
        close(a.effects[0].ss, 49.333_333_333_333_33, 1e-12);
        close(a.effects[0].ss_error, 1.5, 1e-12);
        close(a.effects[0].f_statistic, 49.333_333_333_333_33, 1e-12);
        assert!(factorial_repeated_measures(&v, &[], &[], &[]).is_err());
        let empty = BetweenFactor { name: "G".into(), n_levels: 4, codes: vec![0, 0, 1, 1, 2, 2] };
        assert!(factorial_repeated_measures(&v, &[], &[], &[empty]).is_err());
    }
}
