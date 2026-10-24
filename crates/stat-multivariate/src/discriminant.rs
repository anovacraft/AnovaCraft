//! Normal-theory discriminant analysis: linear and quadratic classification
//! rules with prior probabilities, and Fisher's canonical discriminant
//! functions.
//!
//! # Classification
//! With `g` groups of sizes `n_k`, means `x̄_k`, priors `π_k` and, for each
//! group, a multivariate normal density, an observation `x` is assigned the
//! posterior probabilities
//! `P(k | x) = π_k f_k(x) / Σ_j π_j f_j(x)` (Anderson, *An Introduction to
//! Multivariate Statistical Analysis*, 3rd ed., 2003, §6.4–6.6; Rencher &
//! Christensen, *Methods of Multivariate Analysis*, 3rd ed., 2012, ch. 9):
//!
//! * **linear** rule: every group shares the pooled within-group covariance
//!   `S_w = W / (n − g)`, `W = Σ_k Σ_{i∈k} (xᵢ − x̄_k)(xᵢ − x̄_k)ᵀ`, so
//!   `ln π_k f_k(x) = ln π_k − ½ (x − x̄_k)ᵀ S_w⁻¹ (x − x̄_k) + const`;
//! * **quadratic** rule: each group has its own covariance `S_k` (divisor
//!   `n_k − 1`), adding `−½ ln|S_k|`.
//!
//! The most probable group is the predicted one.
//!
//! # Canonical discriminant functions
//! Fisher's linear combinations `vᵀx` that best separate the group means
//! relative to the within-group variation solve the generalized eigenproblem
//! `B v = λ W v`, with `B = Σ_k n_k (x̄_k − x̄)(x̄_k − x̄)ᵀ` (Fisher 1936,
//! "The use of multiple measurements in taxonomic problems", *Annals of
//! Eugenics* 7; Rao 1948). There are `r = min(p, g − 1)` functions:
//!
//! * eigenvalues `λ_k` of `W⁻¹B` and their shares of `Σλ`;
//! * canonical correlations `√(λ_k / (1 + λ_k))`;
//! * Wilks' `Λ_k = Π_{j≥k} 1/(1 + λ_j)` testing that functions `k, …, r` are
//!   all zero, with Bartlett's approximation
//!   `−(n − 1 − (p + g)/2) ln Λ_k ~ χ²` on `(p − k + 1)(g − k)` degrees of
//!   freedom (`k` counted from 1; Bartlett 1947);
//! * **raw** coefficients `v` scaled so that the scores have unit pooled
//!   within-group variance (`vᵀ S_w v = 1`), with scores centred at the
//!   grand mean, `(x − x̄)ᵀ v`;
//! * **standardized** coefficients `vⱼ √(S_w)ⱼⱼ` (for variables in pooled
//!   within-group standard deviations);
//! * **structure** correlations: pooled within-group correlations of each
//!   variable with each function, `(S_w v)ⱼ / √(S_w)ⱼⱼ`.
//!
//! Each function's sign makes its largest-magnitude raw coefficient positive.

use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_core::eigen::generalized_symmetric_eigen;
use stat_core::{Result, StatError};

use crate::spd;

/// Which covariance model classifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscriminantRule {
    /// Pooled within-group covariance (linear boundaries).
    #[default]
    Linear,
    /// A covariance per group (quadratic boundaries).
    Quadratic,
}

/// Wilks' Λ test of the canonical functions from one onwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WilksTest {
    /// `Π_{j≥k} 1/(1 + λ_j)`.
    pub lambda: f64,
    /// Bartlett's chi-square `−(n − 1 − (p + g)/2) ln Λ`.
    pub chi_square: f64,
    /// Degrees of freedom `(p − k + 1)(g − k)`.
    pub df: f64,
    /// Upper-tail probability.
    pub p_value: f64,
}

/// Fisher's canonical discriminant functions. Coefficient matrices are rows
/// of variables (`p` rows of `r` values).
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalFunctions {
    /// Eigenvalues of `W⁻¹B`, largest first.
    pub eigenvalues: Vec<f64>,
    /// Canonical correlations.
    pub correlations: Vec<f64>,
    /// Wilks' Λ for functions `k..r`, one entry per `k`.
    pub tests: Vec<WilksTest>,
    /// Raw coefficients (unit pooled within-group variance).
    pub raw: Vec<Vec<f64>>,
    /// Standardized coefficients.
    pub standardized: Vec<Vec<f64>>,
    /// Structure (pooled within-group) correlations.
    pub structure: Vec<Vec<f64>>,
    /// Group means on the functions (`g` rows of `r`).
    pub centroids: Vec<Vec<f64>>,
}

impl CanonicalFunctions {
    /// Number of functions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.eigenvalues.len()
    }

    /// Whether there are no functions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.eigenvalues.is_empty()
    }

    /// Each eigenvalue's share of their total.
    #[must_use]
    pub fn proportions(&self) -> Vec<f64> {
        let total: f64 = self.eigenvalues.iter().sum();
        self.eigenvalues.iter().map(|l| if total > 0.0 { l / total } else { f64::NAN }).collect()
    }
}

/// A fitted discriminant analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscriminantFit {
    /// Classification rule.
    pub rule: DiscriminantRule,
    /// Observations per group.
    pub counts: Vec<usize>,
    /// Prior probabilities (sum 1).
    pub priors: Vec<f64>,
    /// Group means (`g` rows of `p`).
    pub means: Vec<Vec<f64>>,
    /// Mean of all observations.
    pub grand_mean: Vec<f64>,
    /// Pooled within-group covariance `S_w`.
    pub pooled_covariance: Vec<Vec<f64>>,
    /// Group covariances `S_k` (quadratic rule only; empty otherwise).
    pub group_covariances: Vec<Vec<Vec<f64>>>,
    /// Canonical discriminant functions.
    pub canonical: CanonicalFunctions,
    /// Cholesky factors of the covariance of each group (one shared factor
    /// for the linear rule).
    factors: Vec<Vec<Vec<f64>>>,
    /// `ln|S|` for each factor.
    log_dets: Vec<f64>,
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

impl DiscriminantFit {
    /// Number of groups.
    #[must_use]
    pub fn n_groups(&self) -> usize {
        self.counts.len()
    }

    /// Number of variables.
    #[must_use]
    pub fn n_variables(&self) -> usize {
        self.grand_mean.len()
    }

    /// `ln π_k f_k(x)` up to a constant shared by the groups.
    #[must_use]
    pub fn log_scores(&self, x: &[f64]) -> Vec<f64> {
        (0..self.n_groups())
            .map(|k| {
                let f = if self.rule == DiscriminantRule::Linear { 0 } else { k };
                let d: Vec<f64> = x.iter().zip(&self.means[k]).map(|(a, b)| a - b).collect();
                self.priors[k].ln() - 0.5 * self.log_dets[f] - 0.5 * spd::quadratic_form(&self.factors[f], &d)
            })
            .collect()
    }

    /// Posterior probabilities of the groups for `x` (NaN if any value is
    /// missing).
    #[must_use]
    pub fn posterior(&self, x: &[f64]) -> Vec<f64> {
        if x.len() != self.n_variables() || x.iter().any(|v| !v.is_finite()) {
            return vec![f64::NAN; self.n_groups()];
        }
        let s = self.log_scores(x);
        let top = s.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let e: Vec<f64> = s.iter().map(|v| (v - top).exp()).collect();
        let total: f64 = e.iter().sum();
        e.iter().map(|v| v / total).collect()
    }

    /// Scores of `x` on the canonical functions (centred at the grand mean).
    #[must_use]
    pub fn canonical_scores(&self, x: &[f64]) -> Vec<f64> {
        let raw = &self.canonical.raw;
        (0..self.canonical.len()).map(|k| (0..x.len()).map(|j| (x[j] - self.grand_mean[j]) * raw[j][k]).sum()).collect()
    }

    /// Squared Mahalanobis distance of `x` from group `k`'s mean (in the
    /// rule's covariance metric).
    #[must_use]
    pub fn distance_squared(&self, x: &[f64], k: usize) -> f64 {
        let f = if self.rule == DiscriminantRule::Linear { 0 } else { k };
        let d: Vec<f64> = x.iter().zip(&self.means[k]).map(|(a, b)| a - b).collect();
        spd::quadratic_form(&self.factors[f], &d)
    }
}

/// Fit a discriminant analysis to observations `x` (rows of `p` values)
/// in groups `groups` (`0..n_groups`), with `priors` (one per group,
/// positive; rescaled to sum to 1).
///
/// # Errors
/// [`StatError::InvalidArgument`] for mismatched lengths, a group code out of
/// range, a group with no observations, non-positive priors, a singular
/// pooled covariance (a variable constant within groups, or a combination of
/// others) or, for the quadratic rule, a group with no more observations
/// than variables or a singular group covariance;
/// [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn fit_discriminant(
    x: &[Vec<f64>],
    groups: &[usize],
    n_groups: usize,
    rule: DiscriminantRule,
    priors: &[f64],
) -> Result<DiscriminantFit> {
    let n = x.len();
    let p = x.first().map_or(0, Vec::len);
    if groups.len() != n {
        return Err(StatError::DimensionMismatch { context: "discriminant groups", expected: n, found: groups.len() });
    }
    if p == 0 || x.iter().any(|r| r.len() != p) {
        return Err(invalid("discriminant analysis needs at least one variable, the same number in every row"));
    }
    if n_groups < 2 {
        return Err(invalid("discriminant analysis needs at least two groups"));
    }
    if priors.len() != n_groups || priors.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
        return Err(invalid("give one positive prior probability per group"));
    }
    if x.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "discriminant analysis data" });
    }
    if groups.iter().any(|&k| k >= n_groups) {
        return Err(invalid("a group code is out of range"));
    }
    let mut members: Vec<Vec<Vec<f64>>> = vec![Vec::new(); n_groups];
    for (row, &k) in x.iter().zip(groups) {
        members[k].push(row.clone());
    }
    let counts: Vec<usize> = members.iter().map(Vec::len).collect();
    if let Some(k) = counts.iter().position(|&c| c == 0) {
        return Err(invalid(format!("group {} has no observations", k + 1)));
    }
    if n <= n_groups {
        return Err(StatError::InsufficientData { required: n_groups + 1, found: n });
    }
    let total: f64 = priors.iter().sum();
    let priors: Vec<f64> = priors.iter().map(|v| v / total).collect();
    let means: Vec<Vec<f64>> = members.iter().map(|m| spd::mean_vector(m, p)).collect();
    let grand_mean = spd::mean_vector(x, p);

    // Within (W) and between (B) SSCP matrices.
    let mut w = vec![vec![0.0; p]; p];
    let mut group_sscp = Vec::with_capacity(n_groups);
    for (m, mean) in members.iter().zip(&means) {
        let mut s = vec![vec![0.0; p]; p];
        for a in 0..p {
            for b in a..p {
                let v = stat_core::summation::sum_iter(m.iter().map(|r| (r[a] - mean[a]) * (r[b] - mean[b])));
                s[a][b] = v;
                s[b][a] = v;
            }
        }
        for a in 0..p {
            for b in 0..p {
                w[a][b] += s[a][b];
            }
        }
        group_sscp.push(s);
    }
    let mut bm = vec![vec![0.0; p]; p];
    for (mean, &c) in means.iter().zip(&counts) {
        for a in 0..p {
            for b in 0..p {
                bm[a][b] += c as f64 * (mean[a] - grand_mean[a]) * (mean[b] - grand_mean[b]);
            }
        }
    }
    let dfw = (n - n_groups) as f64;
    let pooled: Vec<Vec<f64>> = w.iter().map(|r| r.iter().map(|v| v / dfw).collect()).collect();
    let singular = || {
        invalid(
            "the pooled within-group covariance is singular: a variable is constant within the groups or a \
             combination of the others",
        )
    };
    let pooled_l = spd::cholesky(&pooled).ok_or_else(singular)?;

    let (factors, group_covariances) = match rule {
        DiscriminantRule::Linear => (vec![pooled_l], Vec::new()),
        DiscriminantRule::Quadratic => {
            let mut factors = Vec::with_capacity(n_groups);
            let mut covs = Vec::with_capacity(n_groups);
            for (k, (s, &c)) in group_sscp.iter().zip(&counts).enumerate() {
                if c <= p {
                    return Err(invalid(format!(
                        "the quadratic rule needs more observations than variables in every group; group {} has {c}",
                        k + 1
                    )));
                }
                let cov: Vec<Vec<f64>> = s.iter().map(|r| r.iter().map(|v| v / (c - 1) as f64).collect()).collect();
                let l = spd::cholesky(&cov)
                    .ok_or_else(|| invalid(format!("the covariance matrix of group {} is singular", k + 1)))?;
                factors.push(l);
                covs.push(cov);
            }
            (factors, covs)
        }
    };
    let log_dets = factors.iter().map(|l| 2.0 * (0..p).map(|i| l[i][i].ln()).sum::<f64>()).collect();

    // Canonical functions: B v = μ S_w v gives vᵀ S_w v = 1 and μ = (n − g) λ.
    let r = p.min(n_groups - 1);
    let (mu, vectors) = generalized_symmetric_eigen(&bm, &pooled)?;
    let eigenvalues: Vec<f64> = mu.iter().take(r).map(|m| (m / dfw).max(0.0)).collect();
    let vectors: Vec<Vec<f64>> = vectors.into_iter().take(r).collect();
    let raw: Vec<Vec<f64>> = (0..p).map(|j| vectors.iter().map(|v| v[j]).collect()).collect();
    let sd: Vec<f64> = (0..p).map(|j| pooled[j][j].sqrt()).collect();
    let standardized = (0..p).map(|j| raw[j].iter().map(|v| v * sd[j]).collect()).collect();
    let structure = (0..p)
        .map(|j| vectors.iter().map(|v| (0..p).map(|i| pooled[j][i] * v[i]).sum::<f64>() / sd[j]).collect::<Vec<f64>>())
        .collect();
    let centroids = means
        .iter()
        .map(|m| vectors.iter().map(|v| (0..p).map(|j| (m[j] - grand_mean[j]) * v[j]).sum()).collect())
        .collect();
    let (pf, gf) = (p as f64, n_groups as f64);
    let bartlett = n as f64 - 1.0 - (pf + gf) / 2.0;
    let mut tests = Vec::with_capacity(r);
    for k in 0..r {
        let lambda: f64 = eigenvalues[k..].iter().map(|l| 1.0 / (1.0 + l)).product();
        let chi_square = (-bartlett * lambda.ln()).max(0.0);
        let df = ((p - k) * (n_groups - k - 1)) as f64;
        let p_value = ChiSquared::new(df)?.sf(chi_square)?;
        tests.push(WilksTest { lambda, chi_square, df, p_value });
    }
    let correlations = eigenvalues.iter().map(|l| (l / (1.0 + l)).sqrt()).collect();
    Ok(DiscriminantFit {
        rule,
        counts,
        priors,
        means,
        grand_mean,
        pooled_covariance: pooled,
        group_covariances,
        canonical: CanonicalFunctions { eigenvalues, correlations, tests, raw, standardized, structure, centroids },
        factors,
        log_dets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fisher's iris data (`tests/data/discriminant_reference.py`).
    fn iris() -> (Vec<Vec<f64>>, Vec<usize>) {
        include_str!("../tests/data/iris.csv")
            .lines()
            .skip(1)
            .map(|l| {
                let v: Vec<f64> = l.split(',').map(|s| s.parse().unwrap()).collect();
                (v[..4].to_vec(), v[4] as usize)
            })
            .unzip()
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    /// Compare columns of `got` with `want` up to the sign of each column.
    fn assert_columns(got: &[Vec<f64>], want: &[[f64; 2]], tol: f64) {
        for k in 0..2 {
            let sign = if got.iter().zip(want).map(|(g, w)| g[k] * w[k]).sum::<f64>() < 0.0 { -1.0 } else { 1.0 };
            for (g, w) in got.iter().zip(want) {
                assert!(close(sign * g[k], w[k], tol), "column {k}: {} vs {}", g[k], w[k]);
            }
        }
    }

    #[test]
    fn canonical_functions_of_iris() {
        let (x, y) = iris();
        let fit = fit_discriminant(&x, &y, 3, DiscriminantRule::Linear, &[50.0, 50.0, 50.0]).unwrap();
        let c = &fit.canonical;
        assert_eq!(c.len(), 2);
        // scipy.linalg.eigh(B, S_w) / (n − g); the classical values 32.192 and 0.2854.
        assert!(close(c.eigenvalues[0], 32.19192919827802, 1e-10));
        assert!(close(c.eigenvalues[1], 0.285391042623074, 1e-10));
        // scikit-learn explained_variance_ratio_.
        assert!(close(c.proportions()[0], 0.991212604965366, 1e-10));
        assert!(close(c.correlations[0], 0.984820894432084, 1e-10));
        assert!(close(c.correlations[1], 0.471197019230233, 1e-10));
        let t = &c.tests;
        assert!(close(t[0].lambda, 0.02343863065087835, 1e-9));
        assert!(close(t[0].chi_square, 546.1152964877402, 1e-9));
        assert!((t[0].df - 8.0).abs() < 1e-12 && (t[1].df - 3.0).abs() < 1e-12);
        assert!(close(t[1].lambda, 0.7779733690685432, 1e-9));
        assert!(close(t[1].chi_square, 36.52966437258978, 1e-9));
        assert!((t[1].p_value - 5.786050138400005e-08).abs() < 1e-14);
        let raw = [
            [0.829377642266007, 0.024102148876927],
            [1.53447306770001, 2.16452123465847],
            [-2.201211655561774, -0.931921210029332],
            [-2.810460308843101, 2.839187852982676],
        ];
        assert_columns(&c.raw, &raw, 1e-9);
        let standardized = [
            [0.426954848570771, 0.012407531623186],
            [0.521241675799793, 0.735261308527696],
            [-0.947257248677923, -0.401037818951442],
            [-0.575160771920777, 0.581039864541554],
        ];
        assert_columns(&c.standardized, &standardized, 1e-9);
        let structure = [
            [-0.222595941524314, 0.310811723076195],
            [0.119011514884775, 0.863680922376439],
            [-0.706065381085124, 0.167701384282831],
            [-0.633177926162023, 0.737242058780385],
        ];
        assert_columns(&c.structure, &structure, 1e-9);
        let centroids = [
            [7.607599926903663, 0.215133016704321],
            [-1.825049490147949, -0.727899621686191],
            [-5.782550436755694, 0.512766604981868],
        ];
        // Centroids follow the coefficients' signs.
        let s0 = (c.raw[3][0] / raw[3][0]).signum();
        let s1 = (c.raw[3][1] / raw[3][1]).signum();
        for k in 0..3 {
            assert!(close(s0 * c.centroids[k][0], centroids[k][0], 1e-9));
            assert!(close(s1 * c.centroids[k][1], centroids[k][1], 1e-9));
            let mean_score = fit.canonical_scores(&fit.means[k]);
            assert!(close(mean_score[0], c.centroids[k][0], 1e-12));
        }
    }

    #[test]
    fn linear_and_quadratic_posteriors_of_iris() {
        let (x, y) = iris();
        let rows = [0, 50, 70, 83, 100, 133];
        // scipy.stats.multivariate_normal with the pooled covariance.
        let lin = [
            [1.0, 3.896357927686729e-22, 2.611168274948194e-42],
            [1.969731755066091e-18, 9.99889412240982e-01, 1.105877590180953e-04],
            [7.408117581624783e-28, 2.532282247381769e-01, 7.467717752618231e-01],
            [4.241951944740664e-32, 1.433919080787562e-01, 8.566080919212438e-01],
            [7.50307535787316e-52, 7.127303045244229e-09, 9.999999928726968e-01],
            [1.283890624320788e-28, 7.293881280317965e-01, 2.706118719682036e-01],
        ];
        let fit = fit_discriminant(&x, &y, 3, DiscriminantRule::Linear, &[1.0, 1.0, 1.0]).unwrap();
        for (i, &r) in rows.iter().enumerate() {
            let post = fit.posterior(&x[r]);
            for k in 0..3 {
                assert!(close(post[k], lin[i][k], 1e-8) || (post[k] - lin[i][k]).abs() < 1e-15, "row {r}");
            }
        }
        let errors = (0..150).filter(|&i| most(&fit.posterior(&x[i])) != y[i]).count();
        assert_eq!(errors, 3, "the classical three resubstitution errors");
        let quad = [
            [1.0, 4.918516885667842e-26, 2.981541455009418e-41],
            [3.039340006704561e-90, 9.999560692411722e-01, 4.393075882790419e-05],
            [1.052723300173916e-103, 3.359441831241454e-01, 6.640558168758547e-01],
            [4.102009268056914e-114, 1.543483309816286e-01, 8.456516690183714e-01],
            [6.283089741922758e-199, 3.357730721473889e-09, 9.999999966422692e-01],
            [4.550669937647415e-111, 6.049611315124642e-01, 3.950388684875357e-01],
        ];
        let fit = fit_discriminant(&x, &y, 3, DiscriminantRule::Quadratic, &[2.0, 2.0, 2.0]).unwrap();
        assert!((fit.priors[0] - 1.0 / 3.0).abs() < 1e-15);
        for (i, &r) in rows.iter().enumerate() {
            let post = fit.posterior(&x[r]);
            for k in 0..3 {
                assert!(close(post[k], quad[i][k], 1e-8) || (post[k] - quad[i][k]).abs() < 1e-15, "row {r}");
            }
        }
        assert!(fit.posterior(&[1.0, f64::NAN, 1.0, 1.0])[0].is_nan());
    }

    fn most(p: &[f64]) -> usize {
        (0..p.len()).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap()
    }

    #[test]
    fn priors_shift_posteriors_and_bad_input_is_refused() {
        let (x, y) = iris();
        let eq = fit_discriminant(&x, &y, 3, DiscriminantRule::Linear, &[1.0, 1.0, 1.0]).unwrap();
        let tilted = fit_discriminant(&x, &y, 3, DiscriminantRule::Linear, &[1.0, 1.0, 4.0]).unwrap();
        // Posterior odds scale by the prior odds.
        let (a, b) = (eq.posterior(&x[70]), tilted.posterior(&x[70]));
        assert!(close(b[2] / b[1], 4.0 * a[2] / a[1], 1e-10));
        assert!(fit_discriminant(&x, &y, 4, DiscriminantRule::Linear, &[1.0; 4]).is_err(), "empty group");
        assert!(fit_discriminant(&x, &y, 3, DiscriminantRule::Linear, &[1.0, 0.0, 1.0]).is_err());
        let few: Vec<usize> = (0..150).filter(|&i| i % 50 < 4).collect();
        let xs: Vec<Vec<f64>> = few.iter().map(|&i| x[i].clone()).collect();
        let ys: Vec<usize> = few.iter().map(|&i| y[i]).collect();
        assert!(fit_discriminant(&xs, &ys, 3, DiscriminantRule::Quadratic, &[1.0; 3]).is_err(), "n_k ≤ p");
        assert!(fit_discriminant(&xs, &ys, 3, DiscriminantRule::Linear, &[1.0; 3]).is_ok());
        let dup: Vec<Vec<f64>> = x.iter().map(|r| vec![r[0], r[1], r[0] + r[1]]).collect();
        assert!(fit_discriminant(&dup, &y, 3, DiscriminantRule::Linear, &[1.0; 3]).is_err(), "singular");
    }
}
