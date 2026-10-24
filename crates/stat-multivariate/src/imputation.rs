//! Missing-value imputation for a set of numeric columns.
//!
//! Data are row vectors (`data[i][j]`, row `i`, column `j`) with `NaN`
//! marking a missing value. Each method returns the completed matrix, in
//! which observed values are unchanged.
//!
//! # Missing-data patterns
//! [`missing_patterns`] groups rows by which columns they lack (Little &
//! Rubin 2019, *Statistical Analysis with Missing Data*, 3rd ed., §1.2), the
//! basis of the pattern table and of the EM algorithm's E-step.
//!
//! # Column centre
//! [`impute_center`] replaces each missing value by the mean or median of
//! the column's observed values: the baseline, which keeps the means but
//! shrinks variances and correlations.
//!
//! # Multivariate normal, EM algorithm
//! [`em_normal`] finds the maximum-likelihood mean `μ` and covariance `Σ`
//! of a multivariate normal distribution from incomplete rows by the EM
//! algorithm (Dempster, Laird & Rubin 1977, *JRSS B* 39; Schafer 1997,
//! *Analysis of Incomplete Multivariate Data*, ch. 5), assuming values are
//! missing at random. For a row with observed part `O` and missing part `M`
//! the E-step uses the conditional distribution
//!
//! ```text
//! E[x_M | x_O] = μ_M + Σ_MO Σ_OO⁻¹ (x_O − μ_O)
//! Var[x_M | x_O] = Σ_MM − Σ_MO Σ_OO⁻¹ Σ_OM
//! ```
//!
//! to accumulate the expected sufficient statistics `T₁ = Σ E[x]` and
//! `T₂ = Σ E[x xᵀ]` (the conditional covariance is added to `T₂`); the
//! M-step sets `μ = T₁/n` and `Σ = T₂/n − μ μᵀ`. Each iteration does not
//! decrease the observed-data log-likelihood
//!
//! ```text
//! ℓ(μ, Σ) = −½ Σᵢ [ pᵢ ln 2π + ln |Σ_OO| + (x_O − μ_O)ᵀ Σ_OO⁻¹ (x_O − μ_O) ],
//! ```
//!
//! which is recorded at each iteration. The algorithm starts from the
//! observed means and variances with zero covariances (Schafer 1997, §5.3.2)
//! and stops when no mean moves by more than `tolerance` standard
//! deviations and no covariance by more than `tolerance` times the product
//! of the two standard deviations. Missing values are imputed by their
//! conditional means at the estimates.
//!
//! # Low-rank SVD
//! [`svd_impute`] fits the matrix by column means plus a rank-`k` term and
//! iterates: fill the missing cells, recompute the column means and the
//! singular value decomposition of the centred (and, optionally, scaled)
//! matrix, refit the missing cells from the leading `k` singular triples,
//! until the relative change `‖Z_new − Z_old‖²_F / ‖Z_old‖²_F` falls below
//! `tolerance` (Troyanskaya et al. 2001, "Missing value estimation methods
//! for DNA microarrays", *Bioinformatics* 17).
//!
//! Without shrinkage this iteration can drift away (the kept components
//! chase the values they themselves filled in, and with several components
//! the imputed cells can grow far outside the data). By default the kept
//! singular values are therefore shrunk as in the regularized iterative PCA
//! of Josse & Husson (2012, "Handling missing values in exploratory
//! multivariate data analysis methods", *J. SFdS* 153(2); Josse, Pagès &
//! Husson 2009): with `n` rows, `p` columns, rank `k` and singular values
//! `σ₁ ≥ σ₂ ≥ …` of the centred matrix, the noise variance per cell is
//!
//! `σ̂² = n p / min(p, n − 1) · Σ_{s>k} σ_s² / n / ((n − 1) p − (n − 1) k − p k + k²)`,
//!
//! capped at `σ²_{k+1} / n`, and each kept singular value is replaced by
//! `σ_s − n σ̂² / σ_s` (the eigenvalue `λ_s = σ_s²/n` shrunk to `λ_s − σ̂²`).
//! When the discarded components hold no variation the fit is unchanged.
//! Alternatively, with a soft threshold `λ > 0` the singular values become
//! `max(σ − λ, 0)`, as in the soft-impute algorithm of Mazumder, Hastie &
//! Tibshirani (2010, *JMLR* 11); a threshold replaces the regularization.
//! [`choose_rank`] picks `k` by holding out a random share of the
//! observed cells and comparing their imputed values with the truth.

use stat_core::rng::Rng;
use stat_core::summation::sum_iter;
use stat_core::{Matrix, Result, StatError, svd};

use crate::spd::{cholesky, forward};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// One missing-data pattern: the columns a set of rows lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPattern {
    /// `missing[j]` is true when column `j` is missing in these rows.
    pub missing: Vec<bool>,
    /// The rows (indices into the data) with this pattern, ascending.
    pub rows: Vec<usize>,
}

impl MissingPattern {
    /// How many columns the pattern lacks.
    #[must_use]
    pub fn n_missing(&self) -> usize {
        self.missing.iter().filter(|&&m| m).count()
    }
}

/// The missing-data patterns of a data matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPatterns {
    /// Distinct patterns, most frequent first (ties: fewer missing columns
    /// first, then first occurrence).
    pub patterns: Vec<MissingPattern>,
    /// Missing values in each column.
    pub missing_per_column: Vec<usize>,
    /// Number of rows.
    pub n_rows: usize,
}

impl MissingPatterns {
    /// Rows with no missing value.
    #[must_use]
    pub fn n_complete(&self) -> usize {
        self.patterns.iter().filter(|p| p.n_missing() == 0).map(|p| p.rows.len()).sum()
    }

    /// Total missing cells.
    #[must_use]
    pub fn n_missing_cells(&self) -> usize {
        self.missing_per_column.iter().sum()
    }

    /// Whether the pattern is monotone: the columns can be ordered so that a
    /// row missing one column also misses every later one (Little & Rubin
    /// 2019, §1.3). Ordering columns by how often they are missing and
    /// checking each pattern is a prefix in that order decides it.
    #[must_use]
    pub fn is_monotone(&self) -> bool {
        let p = self.missing_per_column.len();
        let mut order: Vec<usize> = (0..p).collect();
        order.sort_by_key(|&j| self.missing_per_column[j]);
        self.patterns.iter().all(|pat| {
            let seq: Vec<bool> = order.iter().map(|&j| pat.missing[j]).collect();
            seq.windows(2).all(|w| !w[0] || w[1])
        })
    }
}

/// Group the rows of `data` by their missing-data pattern (`NaN` is missing).
///
/// # Errors
/// [`StatError::DimensionMismatch`] when the rows differ in length.
pub fn missing_patterns(data: &[Vec<f64>]) -> Result<MissingPatterns> {
    let p = check_rows(data)?;
    let mut patterns: Vec<MissingPattern> = Vec::new();
    let mut index = std::collections::HashMap::new();
    let mut missing_per_column = vec![0; p];
    for (i, row) in data.iter().enumerate() {
        let missing: Vec<bool> = row.iter().map(|x| x.is_nan()).collect();
        for (j, &m) in missing.iter().enumerate() {
            missing_per_column[j] += usize::from(m);
        }
        let k = *index.entry(missing.clone()).or_insert_with(|| {
            patterns.push(MissingPattern { missing, rows: Vec::new() });
            patterns.len() - 1
        });
        patterns[k].rows.push(i);
    }
    patterns.sort_by(|a, b| {
        b.rows.len().cmp(&a.rows.len()).then(a.n_missing().cmp(&b.n_missing())).then(a.rows[0].cmp(&b.rows[0]))
    });
    Ok(MissingPatterns { patterns, missing_per_column, n_rows: data.len() })
}

fn check_rows(data: &[Vec<f64>]) -> Result<usize> {
    let p = data.first().map_or(0, Vec::len);
    if let Some(bad) = data.iter().find(|r| r.len() != p) {
        return Err(StatError::DimensionMismatch { context: "imputation rows", expected: p, found: bad.len() });
    }
    if data.iter().flatten().any(|x| x.is_infinite()) {
        return Err(StatError::NonFiniteInput { context: "imputation data" });
    }
    Ok(p)
}

/// The observed values of column `j`.
fn observed(data: &[Vec<f64>], j: usize) -> Vec<f64> {
    data.iter().map(|r| r[j]).filter(|x| !x.is_nan()).collect()
}

/// Which centre [`impute_center`] fills with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Center {
    /// The mean of the observed values.
    Mean,
    /// The median of the observed values.
    Median,
}

/// Fill each missing value with its column's observed mean or median.
/// Returns the completed data and the value used for each column.
///
/// # Errors
/// Ragged rows, infinite values, or a column with no observed value.
pub fn impute_center(data: &[Vec<f64>], center: Center) -> Result<(Vec<Vec<f64>>, Vec<f64>)> {
    let p = check_rows(data)?;
    let mut fills = Vec::with_capacity(p);
    for j in 0..p {
        let x = observed(data, j);
        if x.is_empty() {
            return Err(invalid(format!("column {} has no observed values", j + 1)));
        }
        fills.push(match center {
            Center::Mean => sum_iter(x.iter().copied()) / x.len() as f64,
            Center::Median => stat_core::descriptive::median(&x)?,
        });
    }
    let completed =
        data.iter().map(|r| r.iter().zip(&fills).map(|(&x, &f)| if x.is_nan() { f } else { x }).collect()).collect();
    Ok((completed, fills))
}

/// Settings of [`em_normal`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmOptions {
    /// Most iterations.
    pub max_iterations: usize,
    /// Largest change in any mean (in standard deviations) or covariance
    /// (relative to the product of standard deviations) that counts as
    /// converged.
    pub tolerance: f64,
}

impl Default for EmOptions {
    fn default() -> Self {
        Self { max_iterations: 1000, tolerance: 1e-8 }
    }
}

/// Maximum-likelihood multivariate normal estimates from incomplete data,
/// and the data completed by conditional means.
#[derive(Debug, Clone, PartialEq)]
pub struct EmImputation {
    /// Mean vector `μ`.
    pub mean: Vec<f64>,
    /// Covariance matrix `Σ` (maximum likelihood, divisor `n`).
    pub covariance: Vec<Vec<f64>>,
    /// Observed-data log-likelihood at the start of each iteration, and at
    /// the final estimates (so one more entry than iterations).
    pub log_likelihood: Vec<f64>,
    /// Largest scaled parameter change of each iteration.
    pub change: Vec<f64>,
    /// Iterations performed.
    pub iterations: usize,
    /// Whether the change fell below the tolerance.
    pub converged: bool,
    /// The data with each missing value replaced by its conditional mean.
    pub completed: Vec<Vec<f64>>,
    /// Rows with at least one observed value (rows with none add nothing to
    /// the likelihood; they are imputed by `μ`).
    pub n_used: usize,
}

/// Per-pattern pieces of the conditional distribution.
struct Conditional {
    obs: Vec<usize>,
    mis: Vec<usize>,
    /// Cholesky factor of `Σ_OO`.
    l: Vec<Vec<f64>>,
    /// `B = Σ_MO Σ_OO⁻¹` (`|M| × |O|`).
    b: Vec<Vec<f64>>,
    /// `Σ_MM − B Σ_OM`.
    c: Vec<Vec<f64>>,
}

fn conditional(missing: &[bool], sigma: &[Vec<f64>]) -> Result<Conditional> {
    let obs: Vec<usize> = (0..missing.len()).filter(|&j| !missing[j]).collect();
    let mis: Vec<usize> = (0..missing.len()).filter(|&j| missing[j]).collect();
    let soo: Vec<Vec<f64>> = obs.iter().map(|&a| obs.iter().map(|&b| sigma[a][b]).collect()).collect();
    let l = cholesky(&soo).ok_or_else(|| {
        invalid("the covariance matrix became singular: a column is constant or an exact combination of others")
    })?;
    // Row m of B solves Σ_OO bᵀ = Σ_Om, i.e. L Lᵀ bᵀ = s.
    let b: Vec<Vec<f64>> = mis
        .iter()
        .map(|&m| {
            let s: Vec<f64> = obs.iter().map(|&o| sigma[o][m]).collect();
            backward(&l, &forward(&l, &s))
        })
        .collect();
    let c = mis
        .iter()
        .enumerate()
        .map(|(a, &ma)| {
            mis.iter()
                .map(|&mb| sigma[ma][mb] - obs.iter().enumerate().map(|(k, &o)| b[a][k] * sigma[o][mb]).sum::<f64>())
                .collect()
        })
        .collect();
    Ok(Conditional { obs, mis, l, b, c })
}

/// Solve `Lᵀ x = z` (back substitution).
fn backward(l: &[Vec<f64>], z: &[f64]) -> Vec<f64> {
    let n = z.len();
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|k| l[k][i] * x[k]).sum();
        x[i] = (z[i] - s) / l[i][i];
    }
    x
}

/// Estimate a multivariate normal mean and covariance from incomplete rows
/// by the EM algorithm and impute each missing value by its conditional
/// mean (see the [module documentation](self)).
///
/// # Errors
/// Ragged rows or infinite values; fewer than one column; a column with
/// fewer than two observed values or no spread; a covariance matrix that
/// becomes singular (a column that is an exact linear combination of
/// others). Not converging within `max_iterations` is reported through
/// [`EmImputation::converged`], not as an error.
pub fn em_normal(data: &[Vec<f64>], options: &EmOptions) -> Result<EmImputation> {
    let p = check_rows(data)?;
    if p == 0 {
        return Err(invalid("choose at least one column"));
    }
    let pats = missing_patterns(data)?;
    let used: Vec<&MissingPattern> = pats.patterns.iter().filter(|pt| pt.n_missing() < p).collect();
    let n_used: usize = used.iter().map(|pt| pt.rows.len()).sum();
    let n = n_used as f64;
    // Start: observed means and variances, no covariance.
    let mut mean = Vec::with_capacity(p);
    let mut sigma = vec![vec![0.0; p]; p];
    for j in 0..p {
        let x = observed(data, j);
        if x.len() < 2 {
            return Err(invalid(format!("column {} needs at least two observed values", j + 1)));
        }
        let m = sum_iter(x.iter().copied()) / x.len() as f64;
        let v = sum_iter(x.iter().map(|v| (v - m) * (v - m))) / x.len() as f64;
        if v <= 0.0 {
            return Err(invalid(format!("column {} has the same value in every observed row", j + 1)));
        }
        mean.push(m);
        sigma[j][j] = v;
    }
    let mut log_likelihood = Vec::new();
    let mut change = Vec::new();
    let mut converged = false;
    let mut iterations = 0;
    while iterations < options.max_iterations {
        let mut t1 = vec![0.0; p];
        let mut t2 = vec![vec![0.0; p]; p];
        let mut ll = 0.0;
        for pat in &used {
            let cond = conditional(&pat.missing, &sigma)?;
            let log_det: f64 = 2.0 * (0..cond.obs.len()).map(|i| cond.l[i][i].ln()).sum::<f64>();
            for &r in &pat.rows {
                let x = expected_row(&data[r], &mean, &cond);
                let d: Vec<f64> = cond.obs.iter().map(|&o| data[r][o] - mean[o]).collect();
                let z = forward(&cond.l, &d);
                ll -= 0.5
                    * (cond.obs.len() as f64 * (2.0 * std::f64::consts::PI).ln()
                        + log_det
                        + z.iter().map(|v| v * v).sum::<f64>());
                for a in 0..p {
                    t1[a] += x[a];
                    for b in a..p {
                        t2[a][b] += x[a] * x[b];
                    }
                }
                for (ia, &ma) in cond.mis.iter().enumerate() {
                    for (ib, &mb) in cond.mis.iter().enumerate() {
                        if ma <= mb {
                            t2[ma][mb] += cond.c[ia][ib];
                        }
                    }
                }
            }
        }
        log_likelihood.push(ll);
        let new_mean: Vec<f64> = t1.iter().map(|t| t / n).collect();
        let mut new_sigma = vec![vec![0.0; p]; p];
        for a in 0..p {
            for b in a..p {
                let s = t2[a][b] / n - new_mean[a] * new_mean[b];
                new_sigma[a][b] = s;
                new_sigma[b][a] = s;
            }
        }
        let sd: Vec<f64> = (0..p).map(|j| sigma[j][j].sqrt()).collect();
        let mut delta = 0.0_f64;
        for a in 0..p {
            delta = delta.max((new_mean[a] - mean[a]).abs() / sd[a]);
            for b in a..p {
                delta = delta.max((new_sigma[a][b] - sigma[a][b]).abs() / (sd[a] * sd[b]));
            }
        }
        mean = new_mean;
        sigma = new_sigma;
        iterations += 1;
        change.push(delta);
        if delta <= options.tolerance {
            converged = true;
            break;
        }
    }
    // Final log-likelihood and conditional means at the estimates.
    let mut completed = data.to_vec();
    let mut ll = 0.0;
    for pat in &pats.patterns {
        if pat.n_missing() == p {
            for &r in &pat.rows {
                completed[r].clone_from(&mean);
            }
            continue;
        }
        let cond = conditional(&pat.missing, &sigma)?;
        let log_det: f64 = 2.0 * (0..cond.obs.len()).map(|i| cond.l[i][i].ln()).sum::<f64>();
        for &r in &pat.rows {
            completed[r] = expected_row(&data[r], &mean, &cond);
            let d: Vec<f64> = cond.obs.iter().map(|&o| data[r][o] - mean[o]).collect();
            let z = forward(&cond.l, &d);
            ll -= 0.5
                * (cond.obs.len() as f64 * (2.0 * std::f64::consts::PI).ln()
                    + log_det
                    + z.iter().map(|v| v * v).sum::<f64>());
        }
    }
    log_likelihood.push(ll);
    Ok(EmImputation { mean, covariance: sigma, log_likelihood, change, iterations, converged, completed, n_used })
}

/// The row with its missing values replaced by their conditional means.
fn expected_row(row: &[f64], mean: &[f64], cond: &Conditional) -> Vec<f64> {
    let mut x = row.to_vec();
    let d: Vec<f64> = cond.obs.iter().map(|&o| row[o] - mean[o]).collect();
    for (a, &m) in cond.mis.iter().enumerate() {
        x[m] = mean[m] + cond.b[a].iter().zip(&d).map(|(b, d)| b * d).sum::<f64>();
    }
    x
}

/// Settings of [`svd_impute`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvdOptions {
    /// Number of singular triples kept (`k`, at least 1 and below the
    /// number of columns).
    pub rank: usize,
    /// Soft-threshold `λ` subtracted from the kept singular values (0 keeps
    /// them as they are). In the units of the centred (scaled) data. Above 0
    /// it replaces `regularize`.
    pub shrinkage: f64,
    /// Shrink the kept singular values by the estimated noise variance
    /// (regularized iterative PCA, see the [module documentation](self)),
    /// which keeps the iteration from drifting. On by default.
    pub regularize: bool,
    /// Divide each column by its observed standard deviation first, so
    /// columns on large scales do not dominate.
    pub standardize: bool,
    /// Most iterations.
    pub max_iterations: usize,
    /// Relative change `‖Z_new − Z_old‖²_F / ‖Z_old‖²_F` that counts as converged.
    pub tolerance: f64,
}

impl Default for SvdOptions {
    fn default() -> Self {
        Self { rank: 2, shrinkage: 0.0, regularize: true, standardize: true, max_iterations: 1000, tolerance: 1e-10 }
    }
}

/// Data completed by a low-rank fit.
#[derive(Debug, Clone, PartialEq)]
pub struct SvdImputation {
    /// The data with missing values replaced by the low-rank fit.
    pub completed: Vec<Vec<f64>>,
    /// Relative change of each iteration.
    pub change: Vec<f64>,
    /// Iterations performed.
    pub iterations: usize,
    /// Whether the change fell below the tolerance.
    pub converged: bool,
    /// Singular values of the final centred (scaled) completed matrix.
    pub singular_values: Vec<f64>,
    /// Noise variance per cell `σ̂²` of the last iteration when the fit was
    /// regularized (in the units of the centred, scaled data).
    pub noise_variance: Option<f64>,
}

/// Impute missing values by an iterated rank-`k` fit (see the
/// [module documentation](self)).
///
/// # Errors
/// Ragged rows or infinite values; fewer than two columns; a rank of 0 or
/// not below the number of columns (or rows); a negative shrinkage; a
/// column with fewer than two observed values or no spread.
pub fn svd_impute(data: &[Vec<f64>], options: &SvdOptions) -> Result<SvdImputation> {
    let p = check_rows(data)?;
    let n = data.len();
    if p < 2 {
        return Err(invalid("low-rank imputation needs at least two columns"));
    }
    if options.rank == 0 || options.rank >= p || options.rank >= n {
        return Err(invalid(format!(
            "the rank must be at least 1 and below the number of columns ({p}) and rows ({n})"
        )));
    }
    if !(options.shrinkage >= 0.0 && options.shrinkage.is_finite()) {
        return Err(invalid("the shrinkage must be zero or positive"));
    }
    let mut scale = vec![1.0; p];
    let mut z = vec![vec![0.0; p]; n];
    let missing: Vec<(usize, usize)> =
        (0..n).flat_map(|i| (0..p).map(move |j| (i, j))).filter(|&(i, j)| data[i][j].is_nan()).collect();
    for j in 0..p {
        let x = observed(data, j);
        if x.len() < 2 {
            return Err(invalid(format!("column {} needs at least two observed values", j + 1)));
        }
        let m = sum_iter(x.iter().copied()) / x.len() as f64;
        let sd = (sum_iter(x.iter().map(|v| (v - m) * (v - m))) / (x.len() - 1) as f64).sqrt();
        if sd <= 0.0 {
            return Err(invalid(format!("column {} has the same value in every observed row", j + 1)));
        }
        if options.standardize {
            scale[j] = sd;
        }
        for i in 0..n {
            // Missing cells start at the observed mean.
            z[i][j] = if data[i][j].is_nan() { m } else { data[i][j] } / scale[j];
        }
    }
    let mut change = Vec::new();
    let mut converged = false;
    let mut iterations = 0;
    let mut singular_values = Vec::new();
    let mut noise_variance = None;
    let regularize = options.regularize && options.shrinkage == 0.0;
    let k = options.rank;
    while iterations < options.max_iterations {
        let means: Vec<f64> = (0..p).map(|j| sum_iter(z.iter().map(|r| r[j])) / n as f64).collect();
        let centred: Vec<Vec<f64>> = z.iter().map(|r| r.iter().zip(&means).map(|(x, m)| x - m).collect()).collect();
        let s = svd(&Matrix::from_rows(&centred)?)?;
        singular_values.clone_from(&s.singular_values);
        let d: Vec<f64> = if regularize {
            let sigma2 = noise_variance_per_cell(&s.singular_values, n, p, k);
            noise_variance = Some(sigma2);
            let nf = n as f64;
            s.singular_values
                .iter()
                .take(k)
                .map(|&v| if v > 0.0 { (v - nf * sigma2 / v).max(0.0) } else { 0.0 })
                .collect()
        } else {
            s.singular_values.iter().take(k).map(|&v| (v - options.shrinkage).max(0.0)).collect()
        };
        let mut num = 0.0;
        let den = sum_iter(z.iter().flatten().map(|v| v * v));
        for &(i, j) in &missing {
            let fit = means[j] + (0..d.len()).map(|k| s.u.col(k)[i] * d[k] * s.v.col(k)[j]).sum::<f64>();
            num += (fit - z[i][j]).powi(2);
            z[i][j] = fit;
        }
        iterations += 1;
        let rel = if den > 0.0 { num / den } else { 0.0 };
        change.push(rel);
        if rel <= options.tolerance {
            converged = true;
            break;
        }
    }
    let completed = (0..n)
        .map(|i| (0..p).map(|j| if data[i][j].is_nan() { z[i][j] * scale[j] } else { data[i][j] }).collect())
        .collect();
    Ok(SvdImputation { completed, change, iterations, converged, singular_values, noise_variance })
}

/// Noise variance per cell of a rank-`k` fit of an `n × p` centred matrix
/// with singular values `sv` (Josse & Husson 2012): the discarded variation
/// over its degrees of freedom, capped at the first discarded eigenvalue
/// `σ²_{k+1}/n` so no kept singular value shrinks below zero.
fn noise_variance_per_cell(sv: &[f64], n: usize, p: usize, k: usize) -> f64 {
    let (nf, pf, kf) = (n as f64, p as f64, k as f64);
    let cap = sv.get(k).map_or(0.0, |s| s * s / nf);
    let df = (nf - 1.0) * pf - (nf - 1.0) * kf - pf * kf + kf * kf;
    if df <= 0.0 {
        return cap;
    }
    let rest = sum_iter(sv.iter().skip(k).map(|s| s * s / nf));
    (nf * pf / pf.min(nf - 1.0) * rest / df).min(cap)
}

/// The held-out comparison behind a chosen rank.
#[derive(Debug, Clone, PartialEq)]
pub struct RankChoice {
    /// Ranks tried (1, 2, …).
    pub ranks: Vec<usize>,
    /// Root mean square error of the held-out cells at each rank, in
    /// standard deviations of their columns.
    pub rmse: Vec<f64>,
    /// The rank with the smallest error.
    pub best: usize,
    /// Number of observed cells held out.
    pub n_held_out: usize,
}

/// Choose the rank of [`svd_impute`] by hiding a random `share` of the
/// observed cells (always leaving each column at least three observed
/// values and each row at least one), imputing
/// them at ranks `1..=max_rank` with `options`, and taking the rank whose
/// imputed values are closest to the hidden ones (root mean square error in
/// column standard deviations). Reproducible from `seed`.
///
/// # Errors
/// As [`svd_impute`]; also a `share` outside (0, 0.5] or too few observed
/// cells to hold any out.
pub fn choose_rank(
    data: &[Vec<f64>],
    max_rank: usize,
    share: f64,
    seed: u64,
    options: &SvdOptions,
) -> Result<RankChoice> {
    let p = check_rows(data)?;
    if !(share > 0.0 && share <= 0.5) {
        return Err(invalid("the held-out share must be in (0, 0.5]"));
    }
    let n = data.len();
    let max_rank = max_rank.min(p.saturating_sub(1)).min(n.saturating_sub(1));
    if max_rank == 0 {
        return Err(invalid("low-rank imputation needs at least two columns and two rows"));
    }
    let mut cells: Vec<(usize, usize)> =
        (0..n).flat_map(|i| (0..p).map(move |j| (i, j))).filter(|&(i, j)| !data[i][j].is_nan()).collect();
    let mut rng = Rng::new(seed);
    rng.shuffle(&mut cells);
    let target = ((cells.len() as f64) * share).round() as usize;
    let mut left: Vec<usize> = (0..p).map(|j| observed(data, j).len()).collect();
    let mut row_left: Vec<usize> = data.iter().map(|r| r.iter().filter(|x| !x.is_nan()).count()).collect();
    let mut held = Vec::new();
    for (i, j) in cells {
        if held.len() >= target {
            break;
        }
        // Keep at least three values per column and one per row.
        if left[j] > 3 && row_left[i] > 1 {
            left[j] -= 1;
            row_left[i] -= 1;
            held.push((i, j));
        }
    }
    if held.is_empty() {
        return Err(invalid("too few observed values to hold any out for choosing the rank"));
    }
    let mut masked = data.to_vec();
    for &(i, j) in &held {
        masked[i][j] = f64::NAN;
    }
    let sd: Vec<f64> = (0..p)
        .map(|j| {
            let x = observed(&masked, j);
            let m = sum_iter(x.iter().copied()) / x.len() as f64;
            (sum_iter(x.iter().map(|v| (v - m) * (v - m))) / (x.len() - 1) as f64).sqrt()
        })
        .collect();
    let mut ranks = Vec::new();
    let mut rmse = Vec::new();
    for k in 1..=max_rank {
        let fit = svd_impute(&masked, &SvdOptions { rank: k, ..*options })?;
        let sse: f64 = held.iter().map(|&(i, j)| ((fit.completed[i][j] - data[i][j]) / sd[j]).powi(2)).sum();
        ranks.push(k);
        rmse.push((sse / held.len() as f64).sqrt());
    }
    let best = ranks[rmse.iter().enumerate().fold(0, |b, (i, &e)| if e < rmse[b] { i } else { b })];
    Ok(RankChoice { ranks, rmse, best, n_held_out: held.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAN: f64 = f64::NAN;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    #[test]
    fn patterns_are_grouped_and_ordered() {
        let data = vec![
            vec![1.0, 2.0, NAN],
            vec![1.0, 2.0, 3.0],
            vec![NAN, 2.0, NAN],
            vec![1.0, 2.0, NAN],
            vec![4.0, 5.0, 6.0],
        ];
        let p = missing_patterns(&data).unwrap();
        assert_eq!(p.patterns.len(), 3);
        assert_eq!(p.patterns[0].rows, vec![1, 4], "ties: fewer missing first");
        assert_eq!(p.patterns[1].rows, vec![0, 3]);
        assert_eq!(p.patterns[2].missing, vec![true, false, true]);
        assert_eq!(p.missing_per_column, vec![1, 0, 3]);
        assert_eq!((p.n_complete(), p.n_missing_cells()), (2, 4));
        assert!(p.is_monotone());
        let not = missing_patterns(&[vec![NAN, 1.0], vec![1.0, NAN], vec![1.0, 1.0]]).unwrap();
        assert!(!not.is_monotone());
    }

    #[test]
    fn centre_fills_mean_or_median() {
        let data = vec![vec![1.0, NAN], vec![2.0, 10.0], vec![NAN, 20.0], vec![9.0, 60.0]];
        let (c, fills) = impute_center(&data, Center::Mean).unwrap();
        assert_eq!(fills, vec![4.0, 30.0]);
        assert_eq!(c[0], vec![1.0, 30.0]);
        assert_eq!(c[2], vec![4.0, 20.0]);
        let (_, fills) = impute_center(&data, Center::Median).unwrap();
        assert_eq!(fills, vec![2.0, 20.0]);
        assert!(impute_center(&[vec![NAN], vec![NAN]], Center::Mean).is_err());
    }

    /// Bivariate data, `y` missing in the last rows (a monotone pattern).
    fn monotone() -> Vec<Vec<f64>> {
        let x = [8.0, 6.0, 11.0, 22.0, 14.0, 17.0, 18.0, 24.0, 19.0, 23.0, 26.0, 40.0, 4.0, 4.0, 5.0, 6.0, 8.0, 10.0];
        let y = [59.0, 58.0, 56.0, 53.0, 50.0, 45.0, 43.0, 42.0, 39.0, 38.0, 30.0, 27.0];
        x.iter().enumerate().map(|(i, &x)| vec![x, y.get(i).copied().unwrap_or(NAN)]).collect()
    }

    #[test]
    fn em_matches_the_closed_form_for_a_monotone_bivariate_pattern() {
        // Anderson (1957); Little & Rubin (2019), §7.2: with x complete and y
        // observed on the first m rows, the ML estimates are
        //   μx = x̄ (all rows), σxx = s_xx (all rows, divisor n),
        //   β = s_xy / s_xx (complete rows), α = ȳ_c − β x̄_c,
        //   μy = α + β μx, σyy = s_yy·x + β² σxx with s_yy·x the residual
        //   variance of the complete-row regression (divisor m),
        //   σxy = β σxx.
        let data = monotone();
        let fit = em_normal(&data, &EmOptions { max_iterations: 10_000, tolerance: 1e-13 }).unwrap();
        assert!(fit.converged);
        let n = data.len() as f64;
        let xs: Vec<f64> = data.iter().map(|r| r[0]).collect();
        let complete: Vec<&Vec<f64>> = data.iter().filter(|r| !r[1].is_nan()).collect();
        let m = complete.len() as f64;
        let mx = xs.iter().sum::<f64>() / n;
        let sxx = xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>() / n;
        let xc = complete.iter().map(|r| r[0]).sum::<f64>() / m;
        let yc = complete.iter().map(|r| r[1]).sum::<f64>() / m;
        let cxx = complete.iter().map(|r| (r[0] - xc).powi(2)).sum::<f64>() / m;
        let cxy = complete.iter().map(|r| (r[0] - xc) * (r[1] - yc)).sum::<f64>() / m;
        let cyy = complete.iter().map(|r| (r[1] - yc).powi(2)).sum::<f64>() / m;
        let beta = cxy / cxx;
        let my = yc + beta * (mx - xc);
        let syy = (cyy - beta * cxy) + beta * beta * sxx;
        let sxy = beta * sxx;
        assert!(close(fit.mean[0], mx, 1e-10) && close(fit.mean[1], my, 1e-9), "{:?}", fit.mean);
        assert!(close(fit.covariance[0][0], sxx, 1e-9));
        assert!(close(fit.covariance[0][1], sxy, 1e-9));
        assert!(close(fit.covariance[1][1], syy, 1e-9));
        // Imputed y is the regression prediction.
        for r in 12..18 {
            assert!(close(fit.completed[r][1], my + beta * (data[r][0] - mx), 1e-9));
        }
    }

    #[test]
    fn em_log_likelihood_never_decreases() {
        let data = vec![
            vec![1.0, 2.0, NAN],
            vec![2.0, NAN, 5.0],
            vec![3.0, 5.0, 4.0],
            vec![NAN, 4.0, 6.0],
            vec![5.0, 9.0, 12.0],
            vec![6.0, 7.5, NAN],
            vec![4.0, 3.0, 9.5],
            vec![2.5, 6.5, 2.0],
            vec![7.0, 8.0, 3.0],
            vec![3.5, NAN, 8.0],
            vec![1.5, 4.0, 5.5],
            vec![NAN, NAN, NAN],
        ];
        let fit = em_normal(&data, &EmOptions::default()).unwrap();
        assert!(fit.converged);
        assert_eq!(fit.n_used, 11);
        assert_eq!(fit.log_likelihood.len(), fit.iterations + 1);
        for w in fit.log_likelihood.windows(2) {
            assert!(w[1] >= w[0] - 1e-9 * w[0].abs(), "{w:?}");
        }
        assert_eq!(fit.completed[11], fit.mean, "an empty row takes the mean");
        assert_eq!(fit.completed[2], data[2], "complete rows unchanged");
    }

    #[test]
    fn em_without_missing_values_is_the_sample_estimate() {
        let data = vec![vec![1.0, 2.0], vec![2.0, 3.5], vec![4.0, 3.0], vec![5.0, 7.0]];
        let fit = em_normal(&data, &EmOptions::default()).unwrap();
        assert!(fit.converged && fit.iterations <= 2);
        assert!(close(fit.mean[0], 3.0, 1e-14) && close(fit.mean[1], 3.875, 1e-14));
        assert!(close(fit.covariance[0][0], 2.5, 1e-12));
        let cxy = ((-2.0) * (2.0 - 3.875) - (3.5 - 3.875) + 1.0 * (3.0 - 3.875) + 2.0 * (7.0 - 3.875)) / 4.0;
        assert!(close(fit.covariance[0][1], cxy, 1e-12));
    }

    #[test]
    fn em_refuses_constant_and_collinear_columns() {
        assert!(em_normal(&[vec![1.0, 2.0], vec![1.0, 3.0], vec![1.0, NAN]], &EmOptions::default()).is_err());
        let collinear: Vec<Vec<f64>> = (0..8)
            .map(|i| vec![f64::from(i), 2.0 * f64::from(i), if i == 3 { NAN } else { f64::from(i * i) }])
            .collect();
        assert!(em_normal(&collinear, &EmOptions::default()).is_err());
    }

    /// Column means plus an exact rank-2 term, with some cells removed.
    fn low_rank(n: usize, p: usize) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let mut rng = Rng::new(7);
        let a: Vec<[f64; 2]> = (0..n).map(|_| [rng.normal(), rng.normal()]).collect();
        let b: Vec<[f64; 2]> = (0..p).map(|_| [rng.normal(), rng.normal()]).collect();
        let full: Vec<Vec<f64>> =
            (0..n).map(|i| (0..p).map(|j| 10.0 * j as f64 + a[i][0] * b[j][0] + a[i][1] * b[j][1]).collect()).collect();
        let mut holes = full.clone();
        for (i, row) in holes.iter_mut().enumerate() {
            for (j, x) in row.iter_mut().enumerate() {
                if (i * 7 + j * 3) % 11 == 0 {
                    *x = NAN;
                }
            }
        }
        (full, holes)
    }

    #[test]
    fn svd_recovers_an_exact_low_rank_matrix() {
        let (full, holes) = low_rank(40, 6);
        let fit = svd_impute(
            &holes,
            &SvdOptions { rank: 2, tolerance: 1e-24, max_iterations: 20_000, ..SvdOptions::default() },
        )
        .unwrap();
        let worst =
            full.iter().flatten().zip(fit.completed.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-6, "largest error {worst}");
        assert!(fit.singular_values[2] < 1e-6 * fit.singular_values[0]);
        // Observed values untouched.
        assert_eq!(fit.completed[1][0], holes[1][0]);
    }

    #[test]
    fn rank_choice_finds_the_true_rank() {
        let (_, holes) = low_rank(60, 6);
        let choice = choose_rank(&holes, 4, 0.1, 11, &SvdOptions::default()).unwrap();
        assert_eq!(choice.best, 2, "{choice:?}");
        assert_eq!(choice.ranks, vec![1, 2, 3, 4]);
        assert!(choice.n_held_out > 0);
    }

    #[test]
    fn shrinkage_pulls_the_fit_towards_the_means() {
        let (_, holes) = low_rank(40, 6);
        let hard = svd_impute(&holes, &SvdOptions::default()).unwrap();
        let soft = svd_impute(&holes, &SvdOptions { shrinkage: 1e6, ..SvdOptions::default() }).unwrap();
        let (mean, _) = impute_center(&holes, Center::Mean).unwrap();
        // A threshold above every singular value leaves only the column means,
        // which then match the observed means (the fixed point of the update).
        for (s, m) in soft.completed.iter().flatten().zip(mean.iter().flatten()) {
            assert!(close(*s, *m, 1e-6));
        }
        assert_ne!(hard.completed, soft.completed);
    }

    #[test]
    fn svd_rejects_bad_ranks() {
        let (_, holes) = low_rank(10, 3);
        assert!(svd_impute(&holes, &SvdOptions { rank: 3, ..SvdOptions::default() }).is_err());
        assert!(svd_impute(&holes, &SvdOptions { rank: 0, ..SvdOptions::default() }).is_err());
    }
}
