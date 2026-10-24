//! Combining analyses of multiply imputed data sets (Rubin's rules).
//!
//! After `m` completed data sets have each been analysed, every parameter
//! has `m` estimates `Q̂ₗ` and squared standard errors `Uₗ`. Rubin (1987,
//! *Multiple Imputation for Nonresponse in Surveys*, §3.1) combines them as
//!
//! ```text
//! Q̄ = (1/m) Σ Q̂ₗ                       pooled estimate
//! Ū = (1/m) Σ Uₗ                       within-imputation variance
//! B = Σ (Q̂ₗ − Q̄)² / (m − 1)            between-imputation variance
//! T = Ū + (1 + 1/m) B                  total variance
//! ```
//!
//! with the relative increase in variance `r = (1 + 1/m) B / Ū`, the share of
//! the variance due to the missing values `λ = (1 + 1/m) B / T`, and the
//! fraction of missing information `γ = (r + 2/(ν + 3)) / (r + 1)`. The
//! reference distribution is Student's t with the Barnard & Rubin (1999,
//! *Biometrika* 86) small-sample degrees of freedom
//!
//! ```text
//! ν_old = (m − 1) / λ²
//! ν_obs = (ν_com + 1)/(ν_com + 3) · ν_com · (1 − λ)
//! ν     = ν_old ν_obs / (ν_old + ν_obs)        (ν = ν_old when ν_com = ∞)
//! ```
//!
//! where `ν_com` is the degrees of freedom the analysis would have with no
//! missing values (`n − p` for a regression; infinite for large-sample
//! tests). As in van Buuren & Groothuis-Oudshoorn (2011, *J. Stat. Softw.*
//! 45(3)) `λ` is floored at 10⁻⁴ inside `ν_old`, so a parameter the missing
//! values do not affect gets a large but finite `ν`.
//!
//! # Several parameters at once
//! [`pool_wald`] is the D₁ statistic of Li, Raghunathan & Rubin (1991,
//! *JASA* 86): for `k` parameters with pooled estimate `Q̄`, average
//! covariance `Ū` and between-imputation covariance `B`,
//!
//! ```text
//! r₁ = (1 + 1/m) tr(B Ū⁻¹) / k,     D₁ = (Q̄ − Q₀)ᵀ Ū⁻¹ (Q̄ − Q₀) / (k (1 + r₁)),
//! ```
//!
//! referred to F(k, ν₁). With `a = k(m − 1)`, `ν₁ = 4 + (a − 4)(1 + (1 − 2/a)/r₁)²`
//! when `a > 4`, else `a (1 + 1/k)(1 + 1/r₁)² / 2`; with complete-data
//! degrees of freedom `ν_com` the small-sample form of Reiter (2007,
//! *Biometrika* 94) is used instead.
//!
//! [`pool_likelihood_ratio`] is the D₃ statistic of Meng & Rubin (1992,
//! *Biometrika* 79): with `dₗ` the likelihood-ratio statistic of data set `l`
//! at its own estimates and `d̃ₗ` the same statistic with both models' parameters
//! held at their pooled values,
//!
//! ```text
//! r₃ = (m + 1) / (k (m − 1)) · (d̄ − d̃̄),       D₃ = d̃̄ / (k (1 + r₃)),
//! ```
//!
//! referred to F(k, ν₃) with `ν₃` from the large-sample formula for `ν₁` (`r₃` in
//! place of `r₁`).
//!
//! # R² and correlations
//! [`pool_correlation`] pools correlations on Fisher's z scale
//! (`z = atanh r`, variance `1/(n − 3)`, `ν_com = ∞`) and transforms the
//! pooled value and its interval back; [`pool_r_squared`] does the same with
//! `√R²` (Harel 2009, *J. Appl. Stat.* 36).

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// One parameter pooled across imputations by Rubin's rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PooledEstimate {
    /// Number of imputations `m`.
    pub m: usize,
    /// Pooled estimate `Q̄`.
    pub estimate: f64,
    /// Within-imputation variance `Ū`.
    pub within: f64,
    /// Between-imputation variance `B`.
    pub between: f64,
    /// Total variance `T = Ū + (1 + 1/m) B`.
    pub total: f64,
    /// Standard error `√T`.
    pub std_error: f64,
    /// Degrees of freedom `ν` (Barnard–Rubin; infinite when both `B = 0`
    /// can be ignored and `ν_com` is infinite — see the module notes).
    pub df: f64,
    /// Relative increase in variance due to missing values, `r`.
    pub riv: f64,
    /// Share of the total variance due to missing values, `λ`.
    pub lambda: f64,
    /// Fraction of missing information, `γ`.
    pub fmi: f64,
    /// `t = (Q̄ − Q₀)/√T` for the null value given.
    pub t: f64,
    /// Two-sided p-value of `t` on `ν` degrees of freedom.
    pub p_value: f64,
    /// Confidence interval for `Q` at the level given.
    pub interval: (f64, f64),
}

/// Barnard–Rubin degrees of freedom for `m` imputations with
/// between-imputation variance `b`, total variance `t` and complete-data
/// degrees of freedom `df_complete` (`f64::INFINITY` for large-sample
/// analyses). `λ` is floored at 10⁻⁴.
#[must_use]
pub fn barnard_rubin_df(m: usize, b: f64, t: f64, df_complete: f64) -> f64 {
    let m = m as f64;
    let lambda = ((1.0 + 1.0 / m) * b / t).max(1e-4);
    let df_old = (m - 1.0) / (lambda * lambda);
    if df_complete.is_infinite() {
        return df_old;
    }
    let df_obs = (df_complete + 1.0) / (df_complete + 3.0) * df_complete * (1.0 - lambda);
    df_old * df_obs / (df_old + df_obs)
}

/// Pool one parameter's estimates `estimates[l]` and squared standard errors
/// `variances[l]` from `m ≥ 2` imputations by Rubin's rules, testing
/// `Q = null` and giving a two-sided interval at `level`.
///
/// # Errors
/// Fewer than two imputations, lengths that differ, non-finite values,
/// negative variances, a level outside (0, 1), `df_complete` not positive,
/// or a total variance of zero.
pub fn pool_scalar(
    estimates: &[f64],
    variances: &[f64],
    df_complete: f64,
    null: f64,
    level: f64,
) -> Result<PooledEstimate> {
    let m = estimates.len();
    if m < 2 {
        return Err(invalid("pooling needs at least two imputations"));
    }
    if variances.len() != m {
        return Err(StatError::DimensionMismatch { context: "pooled variances", expected: m, found: variances.len() });
    }
    if estimates.iter().chain(variances).any(|v| !v.is_finite()) || !null.is_finite() {
        return Err(StatError::NonFiniteInput { context: "pooled estimates" });
    }
    if variances.iter().any(|&u| u < 0.0) {
        return Err(invalid("a variance is negative"));
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid("the confidence level must lie between 0 and 1"));
    }
    if df_complete.is_nan() || df_complete <= 0.0 {
        return Err(invalid("the complete-data degrees of freedom must be positive"));
    }
    let mf = m as f64;
    let estimate = estimates.iter().sum::<f64>() / mf;
    let within = variances.iter().sum::<f64>() / mf;
    let between = estimates.iter().map(|q| (q - estimate).powi(2)).sum::<f64>() / (mf - 1.0);
    let total = within + (1.0 + 1.0 / mf) * between;
    if total.is_nan() || total <= 0.0 {
        return Err(invalid("the total variance is zero"));
    }
    let df = barnard_rubin_df(m, between, total, df_complete);
    let riv = if within > 0.0 { (1.0 + 1.0 / mf) * between / within } else { f64::INFINITY };
    let lambda = (1.0 + 1.0 / mf) * between / total;
    let fmi = if riv.is_finite() { (riv + 2.0 / (df + 3.0)) / (riv + 1.0) } else { 1.0 };
    let std_error = total.sqrt();
    let t = (estimate - null) / std_error;
    let (p_value, q) = if df.is_finite() {
        let dist = StudentsT::new(df)?;
        (dist.two_sided_p_value(t)?, dist.quantile(1.0 - (1.0 - level) / 2.0)?)
    } else {
        let z = Normal::new(0.0, 1.0)?;
        ((2.0 * z.sf(t.abs())?).min(1.0), z.quantile(1.0 - (1.0 - level) / 2.0)?)
    };
    Ok(PooledEstimate {
        m,
        estimate,
        within,
        between,
        total,
        std_error,
        df,
        riv,
        lambda,
        fmi,
        t,
        p_value,
        interval: (estimate - q * std_error, estimate + q * std_error),
    })
}

/// A test of several parameters at once, combined across imputations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PooledTest {
    /// The F statistic (D₁ or D₃).
    pub statistic: f64,
    /// Numerator degrees of freedom `k` (parameters tested).
    pub df_num: usize,
    /// Denominator degrees of freedom `ν`.
    pub df_den: f64,
    /// Upper-tail p-value of F(k, ν).
    pub p_value: f64,
    /// Average relative increase in variance, `r₁` or `r₃`.
    pub riv: f64,
}

/// Denominator degrees of freedom of D₁ and D₃: Li, Raghunathan & Rubin
/// (1991) without complete-data degrees of freedom; with them, Reiter's
/// (2007) small-sample approximation (D₁ only).
fn test_df(k: usize, m: usize, r: f64, df_complete: f64) -> f64 {
    let (k, m) = (k as f64, m as f64);
    let t = k * (m - 1.0);
    if df_complete.is_finite() {
        // Reiter (2007), eq. (6), with a = r t/(t − 2) and ν* = (ν_com + 1)/(ν_com + 3) ν_com.
        let a = r * t / (t - 2.0);
        let v = (df_complete + 1.0) / (df_complete + 3.0) * df_complete;
        let c0 = 1.0 / (t - 4.0);
        let c1 = v - 2.0 * (1.0 + a);
        let c2 = v - 4.0 * (1.0 + a);
        let a2 = a * a;
        let z = 1.0 / c2
            + c0 * (a2 * c1 / ((1.0 + a).powi(2) * c2))
            + c0 * (8.0 * a2 * c1 / ((1.0 + a) * c2 * c2) + 4.0 * a2 / ((1.0 + a) * c2))
            + c0 * (4.0 * a2 / (c2 * c1) + 16.0 * a2 * c1 / c2.powi(3))
            + c0 * (8.0 * a2 / (c2 * c2));
        return 4.0 + 1.0 / z;
    }
    if t > 4.0 {
        4.0 + (t - 4.0) * (1.0 + (1.0 - 2.0 / t) / r).powi(2)
    } else {
        t * (1.0 + 1.0 / k) * (1.0 + 1.0 / r).powi(2) / 2.0
    }
}

fn f_test(statistic: f64, k: usize, df: f64, riv: f64) -> Result<PooledTest> {
    let p_value = if statistic <= 0.0 {
        1.0
    } else if df.is_finite() && df > 0.0 {
        FisherSnedecor::new(k as f64, df)?.sf(statistic)?
    } else {
        // F(k, ∞) is χ²(k)/k.
        stat_core::distributions::ChiSquared::new(k as f64)?.sf(statistic * k as f64)?
    };
    Ok(PooledTest { statistic, df_num: k, df_den: df, p_value, riv })
}

/// Lower Cholesky factor of a symmetric positive-definite matrix.
fn cholesky(a: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    let k = a.len();
    let mut l = vec![vec![0.0; k]; k];
    for i in 0..k {
        for j in 0..=i {
            let s = a[i][j] - (0..j).map(|q| l[i][q] * l[j][q]).sum::<f64>();
            if i == j {
                if s.is_nan() || s <= 0.0 {
                    return Err(invalid("the average covariance matrix is not positive definite"));
                }
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    Ok(l)
}

/// `A⁻¹ b` from the Cholesky factor of `A`.
fn solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let k = b.len();
    let mut z = vec![0.0; k];
    for i in 0..k {
        z[i] = (b[i] - (0..i).map(|q| l[i][q] * z[q]).sum::<f64>()) / l[i][i];
    }
    let mut x = vec![0.0; k];
    for i in (0..k).rev() {
        x[i] = (z[i] - (i + 1..k).map(|q| l[q][i] * x[q]).sum::<f64>()) / l[i][i];
    }
    x
}

/// The D₁ (pooled Wald) test that `k` parameters equal `null` jointly.
///
/// `estimates[l]` holds the `k` estimates of imputation `l` and
/// `covariances[l]` their `k × k` covariance matrix. `df_complete` is the
/// complete-data residual degrees of freedom (`f64::INFINITY` for none).
///
/// # Errors
/// Fewer than two imputations, inconsistent dimensions, non-finite values,
/// or an average covariance matrix that is not positive definite.
pub fn pool_wald(
    estimates: &[Vec<f64>],
    covariances: &[Vec<Vec<f64>>],
    null: &[f64],
    df_complete: f64,
) -> Result<PooledTest> {
    let m = estimates.len();
    if m < 2 {
        return Err(invalid("pooling needs at least two imputations"));
    }
    let k = null.len();
    if k == 0 {
        return Err(invalid("test at least one parameter"));
    }
    if covariances.len() != m
        || estimates.iter().any(|q| q.len() != k)
        || covariances.iter().any(|u| u.len() != k || u.iter().any(|r| r.len() != k))
    {
        return Err(StatError::DimensionMismatch { context: "pooled test", expected: k, found: 0 });
    }
    if estimates.iter().flatten().chain(covariances.iter().flatten().flatten()).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "pooled test" });
    }
    let mf = m as f64;
    let qbar: Vec<f64> = (0..k).map(|j| estimates.iter().map(|q| q[j]).sum::<f64>() / mf).collect();
    let ubar: Vec<Vec<f64>> =
        (0..k).map(|a| (0..k).map(|b| covariances.iter().map(|u| u[a][b]).sum::<f64>() / mf).collect()).collect();
    let l = cholesky(&ubar)?;
    // tr(B Ū⁻¹) = Σₗ (Q̂ₗ − Q̄)ᵀ Ū⁻¹ (Q̂ₗ − Q̄) / (m − 1).
    let trace = estimates
        .iter()
        .map(|q| {
            let d: Vec<f64> = q.iter().zip(&qbar).map(|(a, b)| a - b).collect();
            d.iter().zip(solve(&l, &d)).map(|(a, b)| a * b).sum::<f64>()
        })
        .sum::<f64>()
        / (mf - 1.0);
    let r = (1.0 + 1.0 / mf) * trace / k as f64;
    let d: Vec<f64> = qbar.iter().zip(null).map(|(a, b)| a - b).collect();
    let quad: f64 = d.iter().zip(solve(&l, &d)).map(|(a, b)| a * b).sum();
    let statistic = quad / (k as f64 * (1.0 + r));
    f_test(statistic, k, test_df(k, m, r, df_complete), r)
}

/// The D₃ (pooled likelihood-ratio) test of a model against a model with `k`
/// fewer parameters. `own[l]` is the likelihood-ratio statistic `2(ℓ₁ − ℓ₀)`
/// of imputation `l` at its own estimates; `at_pooled[l]` the same statistic
/// with each model's parameters held at their pooled values.
/// The denominator degrees of freedom are those of Li, Raghunathan & Rubin
/// (1991) in `r₃`, as Meng & Rubin give them.
///
/// # Errors
/// Fewer than two imputations, lengths that differ, non-finite values or
/// `k = 0`.
pub fn pool_likelihood_ratio(own: &[f64], at_pooled: &[f64], k: usize) -> Result<PooledTest> {
    let m = own.len();
    if m < 2 {
        return Err(invalid("pooling needs at least two imputations"));
    }
    if at_pooled.len() != m {
        return Err(StatError::DimensionMismatch { context: "pooled test", expected: m, found: at_pooled.len() });
    }
    if k == 0 {
        return Err(invalid("test at least one parameter"));
    }
    if own.iter().chain(at_pooled).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "pooled test" });
    }
    let mf = m as f64;
    let dbar = own.iter().sum::<f64>() / mf;
    let dtilde = at_pooled.iter().sum::<f64>() / mf;
    let r = (mf + 1.0) / (k as f64 * (mf - 1.0)) * (dbar - dtilde);
    let statistic = dtilde / (k as f64 * (1.0 + r));
    f_test(statistic, k, test_df(k, m, r, f64::INFINITY), r)
}

/// A correlation (or `√R²`) pooled on Fisher's z scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PooledCorrelation {
    /// The pooled value, `tanh z̄` (or `tanh² z̄` for R²).
    pub estimate: f64,
    /// Confidence interval, transformed back.
    pub interval: (f64, f64),
    /// The pooling on the z scale (its `t` and p-value test ρ = 0).
    pub z: PooledEstimate,
}

/// Pool correlations `r[l]`, each from `n[l]` rows (`n > 3`), on Fisher's z
/// scale with variance `1/(n − 3)` and no complete-data degrees of freedom;
/// the interval is `z̄ ± z_{1−α/2} √T` transformed back.
///
/// # Errors
/// As [`pool_scalar`]; a correlation outside (−1, 1) or `n ≤ 3`.
pub fn pool_correlation(r: &[f64], n: &[usize], level: f64) -> Result<PooledCorrelation> {
    if r.len() != n.len() {
        return Err(StatError::DimensionMismatch { context: "pooled correlations", expected: r.len(), found: n.len() });
    }
    if r.iter().any(|v| v.is_nan() || v.abs() >= 1.0) {
        return Err(invalid("a correlation must lie strictly between −1 and 1"));
    }
    if n.iter().any(|&n| n <= 3) {
        return Err(invalid("each correlation needs more than three rows"));
    }
    let z: Vec<f64> = r.iter().map(|v| v.atanh()).collect();
    let u: Vec<f64> = n.iter().map(|&n| 1.0 / (n as f64 - 3.0)).collect();
    let pooled = pool_scalar(&z, &u, f64::INFINITY, 0.0, level)?;
    // A normal interval on the z scale, as van Buuren (2018, §5.3) uses.
    let q = Normal::new(0.0, 1.0)?.quantile(1.0 - (1.0 - level) / 2.0)?;
    let (lo, hi) = (pooled.estimate - q * pooled.std_error, pooled.estimate + q * pooled.std_error);
    Ok(PooledCorrelation { estimate: pooled.estimate.tanh(), interval: (lo.tanh(), hi.tanh()), z: pooled })
}

/// Pool coefficients of determination `r_squared[l]`, each from `n[l]` rows,
/// as `√R²` on Fisher's z scale (Harel 2009), squaring the pooled value and
/// interval ends back (an interval end below zero becomes 0).
///
/// # Errors
/// As [`pool_correlation`]; an R² outside [0, 1).
pub fn pool_r_squared(r_squared: &[f64], n: &[usize], level: f64) -> Result<PooledCorrelation> {
    if r_squared.iter().any(|v| !(0.0..1.0).contains(v)) {
        return Err(invalid("R² must lie in [0, 1)"));
    }
    let r: Vec<f64> = r_squared.iter().map(|v| v.sqrt()).collect();
    let c = pool_correlation(&r, n, level)?;
    let sq = |v: f64| if v < 0.0 { 0.0 } else { v * v };
    Ok(PooledCorrelation { estimate: sq(c.estimate), interval: (sq(c.interval.0), sq(c.interval.1)), z: c.z })
}

#[cfg(test)]
mod tests;
