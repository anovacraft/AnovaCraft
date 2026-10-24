//! Multivariate monitoring from a principal-components model of a reference
//! period: Hotelling's T² on the retained component scores and the squared
//! prediction error (SPE, also written Q) of the part of each observation the
//! model does not explain (Jackson & Mudholkar 1979; Nomikos & MacGregor
//! 1995, "Multivariate SPC charts for monitoring batch processes",
//! *Technometrics* 37; Kourti & MacGregor 1996, "Multivariate SPC methods for
//! process and product monitoring", *J. Quality Technology* 28).
//!
//! The first `m` observations (rows of `p` variables) are the **reference**
//! sample. Its columns are centred on their means and, for a correlation
//! model, divided by their standard deviations, giving standardized rows `z`
//! for every observation (the same reference means and scales apply to later,
//! **monitored**, observations). A principal component analysis of the
//! reference rows ([`stat_multivariate::principal_components`]) gives
//! eigenvalues `λ₁ ≥ … ≥ λ_p` and unit loadings `P` (`p × A` for the `A`
//! retained components). For each observation:
//!
//! * scores `t = Pᵀz` and `T² = Σₐ tₐ²/λₐ`. Reference points have the
//!   scaled-beta upper limit `((m − 1)²/m) · B_α(A/2, (m − A − 1)/2)` and
//!   monitored points `A(m + 1)(m − 1)/(m(m − A)) · F_α(A, m − A)` (Tracy,
//!   Young & Mason 1992), as for a T² chart of `A` variables;
//! * residual `e = z − P t` and `SPE = Σⱼ eⱼ²`. With
//!   `θₖ = Σ_{j>A} λⱼᵏ` (k = 1, 2, 3) and `h₀ = 1 − 2θ₁θ₃/(3θ₂²)`, the
//!   **Jackson–Mudholkar** upper limit is
//!   `Q_α = θ₁ [z_α √(2θ₂h₀²)/θ₁ + 1 + θ₂h₀(h₀ − 1)/θ₁²]^{1/h₀}` with `z_α`
//!   the upper-α standard normal point (Jackson & Mudholkar, "Control
//!   procedures for residuals associated with principal component analysis",
//!   *Technometrics* 21, 1979); the **scaled χ²** limit is
//!   `g χ²_α(h)` with `g = θ₂/θ₁` and `h = θ₁²/θ₂` (Box 1954; Nomikos &
//!   MacGregor 1995). When the bracket of the first is not positive the
//!   second is used.
//!
//! **Contributions** split each statistic over the variables. For T²,
//! variable `j` contributes `cⱼ = zⱼ Σₐ (tₐ/λₐ) pⱼₐ`, and the `cⱼ` sum to T²
//! (a variable can contribute negatively); for SPE it contributes `eⱼ²`
//! (Miller, Swanson & Heckler 1998, "Contribution plots: a missing link in
//! multivariate quality control", *Applied Mathematics and Computer Science*
//! 8; Westerhuis, Gurden & Smilde 2000, *Chemometrics and Intelligent
//! Laboratory Systems* 51).
//!
//! **Choosing `A` by cross-validation** ([`pca_cross_validation`]) follows
//! the element-wise "EKF" scheme of Bro, Kjeldahl, Smilde & Kiers (2008),
//! "Cross-validation of component models: a critical look at current
//! methods", *Analytical and Bioanalytical Chemistry* 390: the reference rows
//! are split into `k` folds by venetian blinds (row `i` in fold `i mod k`);
//! each fold's rows are left out in turn and the model refitted on the rest;
//! for every left-out row and variable `j`, the scores are estimated from the
//! other variables by least squares, `t̂ = (P₋ⱼᵀP₋ⱼ)⁻¹P₋ⱼᵀz₋ⱼ`, and `zⱼ` is
//! predicted as `pⱼᵀt̂`. The prediction error sum of squares (PRESS) over all
//! rows and variables is computed for `A = 1, 2, …` and the `A` with the
//! smallest PRESS is chosen. Leaving out the predicted variable is what makes
//! PRESS rise again once components fit noise (naive row-wise PRESS can only
//! fall as `A` grows).
//!
//! **Change point** ([`mean_change_point`]): for a sequence of `n` vectors
//! `y₁, …, y_n` with common covariance, the likelihood-ratio statistic for a
//! single shift of the mean after observation `τ` is
//! `Λ_τ = −n ln(1 − τ(n − τ)/n² · dᵀ S⁻¹ d)`, where `d` is the difference of
//! the means before and after `τ` and `S` the covariance of all `n` vectors
//! about their mean (divisor `n`); the estimate is the `τ` that maximizes
//! `Λ_τ` (Sullivan & Woodall 2000, "Change-point detection of mean vector or
//! covariance matrix shifts using multivariate individual observations",
//! *IIE Transactions* 32; Srivastava & Worsley 1986). Its p-value is from a
//! permutation test of the maximum (the vectors are exchangeable when there
//! is no shift, and `S` does not change under permutation).
//!
//! # Failure modes
//! Fewer than two variables, observations of unequal length, non-finite
//! values, a reference too small (`m ≤ A + 1`) for the limits, a constant
//! variable in a correlation model, `A` outside `1 … min(p, m − 1)`,
//! `α ∉ (0, 1)` and fewer than two folds are errors. With `A = p` every SPE
//! is zero and there is no SPE limit (`NaN`).

use stat_core::discrete::beta_quantile;
use stat_core::eigen::cholesky;
use stat_core::rng::Rng;
use stat_core::{ChiSquared, ContinuousDistribution, FisherSnedecor, Matrix, Normal, Result, StatError};
use stat_multivariate::{Pca, principal_components};

use crate::multivariate::{ChartPhase, MultivariatePoint};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// How many principal components the model keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentChoice {
    /// A fixed number.
    Fixed(usize),
    /// The number with the smallest cross-validated PRESS, from this many
    /// venetian-blind folds of the reference rows.
    CrossValidation {
        /// Number of folds (at least 2).
        folds: usize,
    },
}

/// Which approximation sets the SPE limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeLimitMethod {
    /// Jackson & Mudholkar (1979), from the residual eigenvalues.
    #[default]
    JacksonMudholkar,
    /// Box's scaled χ² `g χ²(h)` with moments from the residual eigenvalues.
    ScaledChiSquare,
}

impl SpeLimitMethod {
    /// Both methods, in menu order.
    pub const ALL: [Self; 2] = [Self::JacksonMudholkar, Self::ScaledChiSquare];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::JacksonMudholkar => "Jackson–Mudholkar",
            Self::ScaledChiSquare => "Scaled χ² (Box)",
        }
    }

    /// Stable key for saved files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::JacksonMudholkar => "jackson-mudholkar",
            Self::ScaledChiSquare => "scaled-chi-square",
        }
    }

    /// The method with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Settings of a principal-components monitoring chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PcaChartOptions {
    /// Standardize the variables (a correlation model) rather than only
    /// centre them (a covariance model).
    pub standardize: bool,
    /// Number of components retained.
    pub components: ComponentChoice,
    /// False-alarm probability of each limit.
    pub alpha: f64,
    /// Approximation for the SPE limit.
    pub spe_method: SpeLimitMethod,
}

impl Default for PcaChartOptions {
    fn default() -> Self {
        Self {
            standardize: true,
            components: ComponentChoice::CrossValidation { folds: 7 },
            alpha: 0.0027,
            spe_method: SpeLimitMethod::JacksonMudholkar,
        }
    }
}

/// A fitted principal-components monitoring chart.
#[derive(Debug, Clone, PartialEq)]
pub struct PcaChart {
    /// Number of variables `p`.
    pub variables: usize,
    /// Observations in the reference sample `m`.
    pub reference_count: usize,
    /// Settings used.
    pub options: PcaChartOptions,
    /// Retained components `A`.
    pub components: usize,
    /// Reference means of the variables.
    pub means: Vec<f64>,
    /// Reference scale divisors (standard deviations, or 1 for a covariance
    /// model).
    pub scales: Vec<f64>,
    /// All `p` reference eigenvalues, decreasing (zeros beyond the rank).
    pub eigenvalues: Vec<f64>,
    /// Retained loadings, `loadings[j][a]` (`p × A`).
    pub loadings: Vec<Vec<f64>>,
    /// Cross-validated PRESS for `A = 1, 2, …` (empty when `A` was fixed).
    pub press: Vec<f64>,
    /// Standardized observations `z`.
    pub standardized: Vec<Vec<f64>>,
    /// Scores `t` of every observation (`n × A`).
    pub scores: Vec<Vec<f64>>,
    /// Residuals `e = z − P t` (`n × p`).
    pub residuals: Vec<Vec<f64>>,
    /// T² of every observation with its limit.
    pub t2: Vec<MultivariatePoint>,
    /// SPE of every observation with its limit.
    pub spe: Vec<MultivariatePoint>,
    /// T² limit of the reference observations.
    pub t2_reference_limit: f64,
    /// T² limit of monitored observations (`NaN` when all are reference).
    pub t2_monitoring_limit: f64,
    /// SPE limit (`NaN` when `A = p`).
    pub spe_limit: f64,
    /// The SPE method actually used (the scaled χ² when Jackson–Mudholkar's
    /// bracket is not positive).
    pub spe_method_used: SpeLimitMethod,
}

impl PcaChart {
    /// Contributions of each variable to observation `i`'s T²; they sum to
    /// the T² (see the module notes).
    ///
    /// # Errors
    /// An index out of range.
    pub fn t2_contributions(&self, i: usize) -> Result<Vec<f64>> {
        let z = self.standardized.get(i).ok_or_else(|| invalid(format!("no observation {i}")))?;
        let t = &self.scores[i];
        Ok((0..self.variables)
            .map(|j| z[j] * (0..self.components).map(|a| t[a] / self.eigenvalues[a] * self.loadings[j][a]).sum::<f64>())
            .collect())
    }

    /// Contributions `eⱼ²` of each variable to observation `i`'s SPE.
    ///
    /// # Errors
    /// An index out of range.
    pub fn spe_contributions(&self, i: usize) -> Result<Vec<f64>> {
        let e = self.residuals.get(i).ok_or_else(|| invalid(format!("no observation {i}")))?;
        Ok(e.iter().map(|v| v * v).collect())
    }

    /// Proportion of the total variance of the reference sample explained by
    /// the retained components.
    #[must_use]
    pub fn explained(&self) -> f64 {
        let total: f64 = self.eigenvalues.iter().sum();
        self.eigenvalues[..self.components].iter().sum::<f64>() / total
    }
}

fn check_alpha(alpha: f64) -> Result<()> {
    if alpha > 0.0 && alpha < 1.0 { Ok(()) } else { Err(invalid("the false-alarm probability must lie in (0, 1)")) }
}

fn checked_shape(rows: &[Vec<f64>]) -> Result<usize> {
    let p = rows.first().map_or(0, Vec::len);
    if p < 2 {
        return Err(invalid("a principal-components chart needs at least two variables"));
    }
    if rows.iter().any(|r| r.len() != p) {
        return Err(invalid("every observation must have the same number of variables"));
    }
    if rows.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "principal-components chart data" });
    }
    Ok(p)
}

/// The SPE limit of `method` from the residual eigenvalues `λ_{A+1}, …`;
/// `None` when the residual variance is zero, and for Jackson–Mudholkar
/// when its bracket is not positive.
#[must_use]
pub fn spe_limit(residual_eigenvalues: &[f64], alpha: f64, method: SpeLimitMethod) -> Option<f64> {
    let theta = |k: i32| residual_eigenvalues.iter().map(|l| l.max(0.0).powi(k)).sum::<f64>();
    let (t1, t2, t3) = (theta(1), theta(2), theta(3));
    if !(t1 > 0.0 && t2 > 0.0) {
        return None;
    }
    match method {
        SpeLimitMethod::JacksonMudholkar => {
            let h0 = 1.0 - 2.0 * t1 * t3 / (3.0 * t2 * t2);
            let z = Normal::standard().quantile(1.0 - alpha).ok()?;
            let bracket = z * (2.0 * t2 * h0 * h0).sqrt() / t1 + 1.0 + t2 * h0 * (h0 - 1.0) / (t1 * t1);
            (bracket > 0.0 && h0 != 0.0).then(|| t1 * bracket.powf(1.0 / h0)).filter(|q| q.is_finite())
        }
        SpeLimitMethod::ScaledChiSquare => {
            let (g, h) = (t2 / t1, t1 * t1 / t2);
            ChiSquared::new(h).and_then(|d| d.quantile(1.0 - alpha)).ok().map(|q| g * q)
        }
    }
}

/// The T² limits `(reference, monitoring)` for `a` components and `m`
/// reference observations.
///
/// # Errors
/// `m ≤ a + 1`.
pub fn pca_t2_limits(a: usize, m: usize, alpha: f64) -> Result<(f64, f64)> {
    if m <= a + 1 {
        return Err(invalid(format!("{a} components need more than {} reference observations", a + 1)));
    }
    let (af, mf) = (a as f64, m as f64);
    let beta = beta_quantile(af / 2.0, (mf - af - 1.0) / 2.0, 1.0 - alpha)?;
    let phase1 = (mf - 1.0).powi(2) / mf * beta;
    let phase2 =
        FisherSnedecor::new(af, mf - af)?.quantile(1.0 - alpha)? * af * (mf + 1.0) * (mf - 1.0) / (mf * (mf - af));
    Ok((phase1, phase2))
}

fn to_matrix(rows: &[&Vec<f64>]) -> Result<Matrix> {
    Matrix::from_rows(rows)
}

/// `(P₋ⱼᵀP₋ⱼ)⁻¹P₋ⱼᵀz₋ⱼ` predictions `pⱼᵀt̂` for every variable `j` of the
/// standardized row `z`, with the first `a` loadings of `pca`.
fn ekf_errors(pca: &Pca, a: usize, z: &[f64]) -> f64 {
    let p = z.len();
    let load = |j: usize, k: usize| pca.loadings[(j, k)];
    // PᵀP and Pᵀz over all variables; leaving out j subtracts its terms.
    let mut ptp = vec![vec![0.0; a]; a];
    let mut ptz = vec![0.0; a];
    for j in 0..p {
        for k in 0..a {
            ptz[k] += load(j, k) * z[j];
            for l in 0..a {
                ptp[k][l] += load(j, k) * load(j, l);
            }
        }
    }
    let mut press = 0.0;
    for j in 0..p {
        let mut m = ptp.clone();
        let mut rhs = ptz.clone();
        for k in 0..a {
            rhs[k] -= load(j, k) * z[j];
            for l in 0..a {
                m[k][l] -= load(j, k) * load(j, l);
            }
        }
        let t = match cholesky(&m) {
            Some(lower) => solve_cholesky(&lower, &rhs),
            None => vec![0.0; a],
        };
        let pred: f64 = (0..a).map(|k| load(j, k) * t[k]).sum();
        press += (z[j] - pred).powi(2);
    }
    press
}

fn solve_cholesky(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut y = vec![0.0; n];
    for i in 0..n {
        y[i] = (b[i] - (0..i).map(|k| l[i][k] * y[k]).sum::<f64>()) / l[i][i];
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        x[i] = (y[i] - (i + 1..n).map(|k| l[k][i] * x[k]).sum::<f64>()) / l[i][i];
    }
    x
}

/// Cross-validated PRESS of principal-components models with
/// `1, 2, …, max_components` components (element-wise EKF scheme with
/// venetian-blind folds; see the module notes). The maximum is reduced to
/// `p − 1` and to one less than the smallest training set.
///
/// # Errors
/// Fewer than two folds, fewer observations than folds, a constant variable
/// in a training set of a correlation model, and the shape errors.
pub fn pca_cross_validation(
    rows: &[Vec<f64>],
    standardize: bool,
    folds: usize,
    max_components: usize,
) -> Result<Vec<f64>> {
    let p = checked_shape(rows)?;
    let n = rows.len();
    if folds < 2 {
        return Err(invalid("cross-validation needs at least two folds"));
    }
    if n < 2 * folds.max(2) {
        return Err(invalid(format!("cross-validation with {folds} folds needs at least {} observations", 2 * folds)));
    }
    let smallest_train = n - n.div_ceil(folds);
    let max_a = max_components.min(p - 1).min(smallest_train - 1).max(1);
    let mut press = vec![0.0; max_a];
    for f in 0..folds {
        let train: Vec<&Vec<f64>> = rows.iter().enumerate().filter(|(i, _)| i % folds != f).map(|(_, r)| r).collect();
        let pca = principal_components(&to_matrix(&train)?, standardize)?;
        for (_, row) in rows.iter().enumerate().filter(|(i, _)| i % folds == f) {
            let z: Vec<f64> = (0..p).map(|j| (row[j] - pca.means[j]) / pca.scales[j]).collect();
            for (a, total) in press.iter_mut().enumerate() {
                if a < pca.n_components() {
                    *total += ekf_errors(&pca, a + 1, &z);
                } else {
                    *total = f64::NAN;
                }
            }
        }
    }
    Ok(press)
}

/// Fit the principal-components model on the first `reference` observations
/// (all when `None`) and chart T² and SPE for every observation.
///
/// # Errors
/// See the module notes.
pub fn pca_chart(rows: &[Vec<f64>], reference: Option<usize>, options: &PcaChartOptions) -> Result<PcaChart> {
    check_alpha(options.alpha)?;
    let p = checked_shape(rows)?;
    let n = rows.len();
    let m = reference.unwrap_or(n).min(n);
    if m < 3 {
        return Err(StatError::InsufficientData { required: 3, found: m });
    }
    let reference_rows: Vec<&Vec<f64>> = rows[..m].iter().collect();
    let pca = principal_components(&to_matrix(&reference_rows)?, options.standardize)?;
    let rank = pca.n_components();
    let (components, press) = match options.components {
        ComponentChoice::Fixed(a) => {
            if a == 0 || a > rank {
                return Err(invalid(format!("choose between 1 and {rank} components")));
            }
            (a, Vec::new())
        }
        ComponentChoice::CrossValidation { folds } => {
            let press = pca_cross_validation(&rows[..m], options.standardize, folds, m.saturating_sub(2).max(1))?;
            let best = press
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_finite())
                .min_by(|a, b| a.1.total_cmp(b.1))
                .map_or(1, |(a, _)| a + 1);
            (best.min(rank), press)
        }
    };
    let (t2_reference_limit, t2_monitoring) = pca_t2_limits(components, m, options.alpha)?;
    let mut eigenvalues = pca.eigenvalues.clone();
    eigenvalues.resize(p, 0.0);
    let residual = &eigenvalues[components..];
    let (spe_limit_value, spe_method_used) = match spe_limit(residual, options.alpha, options.spe_method) {
        Some(q) => (q, options.spe_method),
        None => match spe_limit(residual, options.alpha, SpeLimitMethod::ScaledChiSquare) {
            Some(q) => (q, SpeLimitMethod::ScaledChiSquare),
            None => (f64::NAN, options.spe_method),
        },
    };
    let loadings: Vec<Vec<f64>> = (0..p).map(|j| (0..components).map(|a| pca.loadings[(j, a)]).collect()).collect();
    let standardized: Vec<Vec<f64>> =
        rows.iter().map(|r| (0..p).map(|j| (r[j] - pca.means[j]) / pca.scales[j]).collect()).collect();
    let scores: Vec<Vec<f64>> = standardized
        .iter()
        .map(|z| (0..components).map(|a| (0..p).map(|j| z[j] * loadings[j][a]).sum()).collect())
        .collect();
    let residuals: Vec<Vec<f64>> = standardized
        .iter()
        .zip(&scores)
        .map(|(z, t)| (0..p).map(|j| z[j] - (0..components).map(|a| loadings[j][a] * t[a]).sum::<f64>()).collect())
        .collect();
    let phase = |i: usize| if i < m { ChartPhase::Reference } else { ChartPhase::Monitoring };
    let t2 = scores
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let statistic: f64 = (0..components).map(|a| t[a] * t[a] / eigenvalues[a]).sum();
            let upper = if i < m { t2_reference_limit } else { t2_monitoring };
            MultivariatePoint { statistic, lower: 0.0, upper, phase: phase(i), signal: statistic > upper }
        })
        .collect();
    let spe = residuals
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let statistic: f64 = e.iter().map(|v| v * v).sum();
            let signal = spe_limit_value.is_finite() && statistic > spe_limit_value;
            MultivariatePoint { statistic, lower: 0.0, upper: spe_limit_value, phase: phase(i), signal }
        })
        .collect();
    Ok(PcaChart {
        variables: p,
        reference_count: m,
        options: *options,
        components,
        means: pca.means.clone(),
        scales: pca.scales.clone(),
        eigenvalues,
        loadings,
        press,
        standardized,
        scores,
        residuals,
        t2,
        spe,
        t2_reference_limit,
        t2_monitoring_limit: if m < n { t2_monitoring } else { f64::NAN },
        spe_limit: spe_limit_value,
        spe_method_used,
    })
}

/// A likelihood-ratio estimate of a single shift in the mean vector.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangePoint {
    /// Number of observations before the estimated shift (`τ̂`, 1-based: the
    /// shift happens after observation `τ̂`).
    pub before: usize,
    /// The largest statistic `Λ_τ̂`.
    pub statistic: f64,
    /// Permutation p-value of the largest statistic.
    pub p_value: f64,
    /// Permutations used.
    pub permutations: usize,
    /// `Λ_τ` for `τ = 1, …, n − 1`.
    pub statistics: Vec<f64>,
}

/// `Λ_τ` for every split, from the vectors and the inverse-covariance
/// Cholesky factor.
fn split_statistics(y: &[&[f64]], l: &[Vec<f64>]) -> Vec<f64> {
    let n = y.len();
    let q = y[0].len();
    let total: Vec<f64> = (0..q).map(|k| y.iter().map(|v| v[k]).sum()).collect();
    let mut before = vec![0.0; q];
    let nf = n as f64;
    let mut out = Vec::with_capacity(n - 1);
    for (tau, v) in y.iter().enumerate().take(n - 1) {
        for k in 0..q {
            before[k] += v[k];
        }
        let t = (tau + 1) as f64;
        let d: Vec<f64> = (0..q).map(|k| before[k] / t - (total[k] - before[k]) / (nf - t)).collect();
        // dᵀ S⁻¹ d through the Cholesky factor of S.
        let mut w = vec![0.0; q];
        let mut quad = 0.0;
        for i in 0..q {
            w[i] = (d[i] - (0..i).map(|k| l[i][k] * w[k]).sum::<f64>()) / l[i][i];
            quad += w[i] * w[i];
        }
        let r = (t * (nf - t) / (nf * nf) * quad).min(1.0 - 1e-15);
        out.push(-nf * (1.0 - r).ln());
    }
    out
}

/// The likelihood-ratio change-point estimate for a single shift in the mean
/// of `y` (rows of equal length), with a permutation p-value from
/// `permutations` shuffles drawn with `seed` (see the module notes).
///
/// # Errors
/// Fewer than `q + 2` vectors of `q` values, unequal lengths, non-finite
/// values, or a singular covariance.
pub fn mean_change_point(y: &[Vec<f64>], permutations: usize, seed: u64) -> Result<ChangePoint> {
    let q = y.first().map_or(0, Vec::len);
    let n = y.len();
    if q == 0 || y.iter().any(|v| v.len() != q) {
        return Err(invalid("change-point vectors must be non-empty and of equal length"));
    }
    if n < q + 2 {
        return Err(StatError::InsufficientData { required: q + 2, found: n });
    }
    if y.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "change-point data" });
    }
    let nf = n as f64;
    let mean: Vec<f64> = (0..q).map(|k| y.iter().map(|v| v[k]).sum::<f64>() / nf).collect();
    let cov: Vec<Vec<f64>> = (0..q)
        .map(|a| (0..q).map(|b| y.iter().map(|v| (v[a] - mean[a]) * (v[b] - mean[b])).sum::<f64>() / nf).collect())
        .collect();
    let l = cholesky(&cov).ok_or_else(|| invalid("the covariance of the change-point data is singular"))?;
    let refs: Vec<&[f64]> = y.iter().map(Vec::as_slice).collect();
    let statistics = split_statistics(&refs, &l);
    let (best, &statistic) =
        statistics.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).expect("at least one split");
    let mut rng = Rng::new(seed);
    let mut order = refs.clone();
    let mut exceed = 0usize;
    for _ in 0..permutations {
        rng.shuffle(&mut order);
        let m = split_statistics(&order, &l).into_iter().fold(f64::NEG_INFINITY, f64::max);
        if m >= statistic * (1.0 - 1e-12) {
            exceed += 1;
        }
    }
    Ok(ChangePoint {
        before: best + 1,
        statistic,
        p_value: (exceed + 1) as f64 / (permutations + 1) as f64,
        permutations,
        statistics,
    })
}

#[cfg(test)]
mod tests;
