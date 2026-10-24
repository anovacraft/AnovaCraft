//! Gaussian mixture clustering by the EM algorithm, with the covariance
//! structures of Celeux & Govaert (1995) and model choice by BIC (Fraley &
//! Raftery 2002).
//!
//! # Model
//! Each point `x ∈ ℝᵖ` comes from one of `K` multivariate normal
//! components: `f(x) = Σₖ πₖ φ(x; μₖ, Σₖ)` with mixing proportions
//! `πₖ > 0`, `Σ πₖ = 1`. The covariance matrices are constrained by one of six
//! structures, named by volume, shape and orientation (E = equal across
//! components, V = varying, I = identity):
//!
//! | Key | Covariance | Parameters |
//! |-----|------------|------------|
//! | EII | `λ I` (spherical, equal volume) | 1 |
//! | VII | `λₖ I` (spherical, varying volume) | K |
//! | EEI | `diag(b)` shared (diagonal, equal) | p |
//! | VVI | `diag(bₖ)` (diagonal, varying) | K p |
//! | EEE | `Σ` shared (full, equal) | p(p+1)/2 |
//! | VVV | `Σₖ` (full, varying) | K p(p+1)/2 |
//!
//! # Estimation
//! EM (Dempster, Laird & Rubin 1977). The E-step computes membership
//! probabilities `zᵢₖ = πₖ φ(xᵢ; μₖ, Σₖ) / Σⱼ πⱼ φ(xᵢ; μⱼ, Σⱼ)` (on the log
//! scale with log-sum-exp). The M-step sets `nₖ = Σᵢ zᵢₖ`, `πₖ = nₖ/n`,
//! `μₖ = Σᵢ zᵢₖ xᵢ / nₖ` and, from the weighted scatter matrices
//! `Wₖ = Σᵢ zᵢₖ (xᵢ − μₖ)(xᵢ − μₖ)ᵀ` and `W = Σₖ Wₖ`, the closed-form
//! covariance estimates of Celeux & Govaert (1995): EII `λ = tr W / (n p)`;
//! VII `λₖ = tr Wₖ / (nₖ p)`; EEI `diag(W)/n`; VVI `diag(Wₖ)/nₖ`; EEE `W/n`;
//! VVV `Wₖ/nₖ`. An optional ridge is added to every covariance diagonal
//! (as `reg_covar` in scikit-learn) to keep the estimates positive
//! definite. Iterations stop when the log-likelihood changes by less than
//! `tolerance · (1 + |ℓ|)`.
//!
//! # Starts
//! The first start is a hard partition from k-means (k-means++ seeding, ten
//! starts) or from Ward hierarchical clustering; each further start is a
//! single seeded k-means++ run. The start with the largest final
//! log-likelihood is kept. Everything is reproducible from the seed.
//!
//! # Choosing a model
//! `BIC = −2 ℓ + m ln n` (Schwarz 1978), with `m` free parameters:
//! `K − 1` proportions, `K p` means and the covariance parameters above.
//! Smaller is better. (Fraley & Raftery report `2ℓ − m ln n`, the same
//! criterion with the opposite sign.) [`select_gaussian_mixture`] fits every
//! requested structure and number of components and picks the smallest BIC;
//! fits that fail (a component collapsing onto too few points, a singular
//! covariance) are recorded as such, as mclust records them as missing.
//!
//! Components are numbered by the first row (in data order) whose most
//! likely component they are, so labels do not depend on the start.

use stat_core::{Result, StatError};

use crate::hierarchical::{Linkage, hierarchical_clustering};
use crate::kmeans::{KMeansOptions, k_means};
use crate::spd::cholesky;

/// Covariance structure of the mixture components (Celeux & Govaert 1995).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CovarianceStructure {
    /// Spherical, equal volume: `λ I`.
    Eii,
    /// Spherical, varying volume: `λₖ I`.
    Vii,
    /// Diagonal, equal across components: `diag(b)`.
    Eei,
    /// Diagonal, varying: `diag(bₖ)`.
    Vvi,
    /// Full, equal across components: `Σ`.
    Eee,
    /// Full, varying: `Σₖ`.
    Vvv,
}

impl CovarianceStructure {
    /// Every structure, simplest first.
    pub const ALL: [Self; 6] = [Self::Eii, Self::Vii, Self::Eei, Self::Vvi, Self::Eee, Self::Vvv];

    /// The three-letter key (volume, shape, orientation), e.g. `"VVV"`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Eii => "EII",
            Self::Vii => "VII",
            Self::Eei => "EEI",
            Self::Vvi => "VVI",
            Self::Eee => "EEE",
            Self::Vvv => "VVV",
        }
    }

    /// The structure with this key (case-insensitive).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key().eq_ignore_ascii_case(key))
    }

    /// Plain description, e.g. `"Full, varying"`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Eii => "Spherical, equal",
            Self::Vii => "Spherical, varying",
            Self::Eei => "Diagonal, equal",
            Self::Vvi => "Diagonal, varying",
            Self::Eee => "Full, equal",
            Self::Vvv => "Full, varying",
        }
    }

    /// Whether every component has the same covariance matrix.
    #[must_use]
    pub fn shared(self) -> bool {
        matches!(self, Self::Eii | Self::Eei | Self::Eee)
    }

    /// Free covariance parameters for `k` components in `p` dimensions.
    #[must_use]
    pub fn covariance_parameters(self, k: usize, p: usize) -> usize {
        let full = p * (p + 1) / 2;
        match self {
            Self::Eii => 1,
            Self::Vii => k,
            Self::Eei => p,
            Self::Vvi => k * p,
            Self::Eee => full,
            Self::Vvv => k * full,
        }
    }

    /// All free parameters: `K − 1` proportions, `K p` means and the
    /// covariance parameters.
    #[must_use]
    pub fn parameters(self, k: usize, p: usize) -> usize {
        k.saturating_sub(1) + k * p + self.covariance_parameters(k, p)
    }
}

/// How the first EM start partitions the points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MixtureStart {
    /// K-means (k-means++ seeding, ten starts).
    #[default]
    KMeans,
    /// Ward hierarchical clustering cut into `K` groups (deterministic).
    Hierarchical,
}

impl MixtureStart {
    /// Stable key: `"k-means"` or `"hierarchical"`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::KMeans => "k-means",
            Self::Hierarchical => "hierarchical",
        }
    }

    /// The start with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::KMeans, Self::Hierarchical].into_iter().find(|s| s.key() == key)
    }

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::KMeans => "k-means",
            Self::Hierarchical => "Ward hierarchical clustering",
        }
    }
}

/// Settings of one mixture fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixtureOptions {
    /// Number of components `K` (at least 1).
    pub k: usize,
    /// Covariance structure.
    pub structure: CovarianceStructure,
    /// Number of EM starts (at least 1): the first from [`MixtureStart`],
    /// the others from seeded single k-means++ runs.
    pub starts: usize,
    /// Partition of the first start.
    pub start: MixtureStart,
    /// Most EM iterations per start.
    pub max_iterations: usize,
    /// Relative change in log-likelihood that ends the iterations.
    pub tolerance: f64,
    /// Added to every covariance diagonal after each M-step (≥ 0).
    pub ridge: f64,
    /// Seed of the random starts.
    pub seed: u64,
}

impl MixtureOptions {
    /// `k` full, varying (VVV) components; 5 starts from k-means; up to 1000
    /// iterations with tolerance 1e-8; no ridge; seed 1.
    #[must_use]
    pub fn new(k: usize, structure: CovarianceStructure) -> Self {
        Self {
            k,
            structure,
            starts: 5,
            start: MixtureStart::KMeans,
            max_iterations: 1000,
            tolerance: 1e-8,
            ridge: 0.0,
            seed: 1,
        }
    }
}

/// A fitted Gaussian mixture.
#[derive(Debug, Clone, PartialEq)]
pub struct GaussianMixture {
    /// Covariance structure.
    pub structure: CovarianceStructure,
    /// Mixing proportions `πₖ`.
    pub weights: Vec<f64>,
    /// Component means `μₖ` (`K` vectors of length `p`).
    pub means: Vec<Vec<f64>>,
    /// Component covariance matrices `Σₖ` (one per component, equal for
    /// the shared structures).
    pub covariances: Vec<Vec<Vec<f64>>>,
    /// Membership probabilities `zᵢₖ` (`n` rows of `K`).
    pub probabilities: Vec<Vec<f64>>,
    /// Maximized log-likelihood `ℓ`.
    pub log_likelihood: f64,
    /// EM iterations of the kept start.
    pub iterations: usize,
    /// Whether the kept start met the tolerance.
    pub converged: bool,
}

impl GaussianMixture {
    /// Number of components.
    #[must_use]
    pub fn k(&self) -> usize {
        self.weights.len()
    }

    /// Number of points.
    #[must_use]
    pub fn n(&self) -> usize {
        self.probabilities.len()
    }

    /// Dimension `p`.
    #[must_use]
    pub fn p(&self) -> usize {
        self.means.first().map_or(0, Vec::len)
    }

    /// Free parameters `m`.
    #[must_use]
    pub fn n_parameters(&self) -> usize {
        self.structure.parameters(self.k(), self.p())
    }

    /// `BIC = −2 ℓ + m ln n` (smaller is better).
    #[must_use]
    pub fn bic(&self) -> f64 {
        -2.0 * self.log_likelihood + self.n_parameters() as f64 * (self.n() as f64).ln()
    }

    /// Most likely component of each point (`0..K`).
    #[must_use]
    pub fn classification(&self) -> Vec<usize> {
        self.probabilities.iter().map(|z| argmax(z)).collect()
    }

    /// Uncertainty of each point's classification, `1 − maxₖ zᵢₖ`.
    #[must_use]
    pub fn uncertainty(&self) -> Vec<f64> {
        self.probabilities.iter().map(|z| 1.0 - z.iter().copied().fold(0.0, f64::max)).collect()
    }

    /// Membership probabilities of new points under the fitted mixture.
    ///
    /// # Errors
    /// A point of the wrong dimension or with a non-finite value, or a
    /// covariance matrix that is not positive definite.
    pub fn predict(&self, points: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
        let p = self.p();
        if points.iter().any(|x| x.len() != p) {
            return Err(invalid("every point needs as many coordinates as the fitted means"));
        }
        if points.iter().flatten().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "mixture prediction" });
        }
        let params = Params { weights: self.weights.clone(), means: self.means.clone(), cov: self.covariances.clone() };
        let (z, _) =
            e_step(points, &params).ok_or_else(|| invalid("a component covariance is not positive definite"))?;
        Ok(z)
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn argmax(z: &[f64]) -> usize {
    let mut best = 0;
    for (k, &v) in z.iter().enumerate() {
        if v > z[best] {
            best = k;
        }
    }
    best
}

/// Current parameters of an EM run.
struct Params {
    weights: Vec<f64>,
    means: Vec<Vec<f64>>,
    cov: Vec<Vec<Vec<f64>>>,
}

/// M-step from membership probabilities `z`; `None` when a component is
/// (nearly) empty.
fn m_step(points: &[Vec<f64>], z: &[Vec<f64>], structure: CovarianceStructure, ridge: f64) -> Option<Params> {
    let (n, p, k) = (points.len(), points[0].len(), z[0].len());
    let nk: Vec<f64> = (0..k).map(|c| z.iter().map(|zi| zi[c]).sum()).collect();
    if nk.iter().any(|&m| m <= 10.0 * f64::EPSILON * n as f64) {
        return None;
    }
    let means: Vec<Vec<f64>> = (0..k)
        .map(|c| (0..p).map(|j| points.iter().zip(z).map(|(x, zi)| zi[c] * x[j]).sum::<f64>() / nk[c]).collect())
        .collect();
    // Weighted scatter matrices (only the diagonal for spherical/diagonal).
    let full = matches!(structure, CovarianceStructure::Eee | CovarianceStructure::Vvv);
    let mut scatter = vec![vec![vec![0.0; p]; p]; k];
    for (x, zi) in points.iter().zip(z) {
        for c in 0..k {
            let w = zi[c];
            if w == 0.0 {
                continue;
            }
            let s = &mut scatter[c];
            for a in 0..p {
                let da = x[a] - means[c][a];
                if full {
                    for b in 0..=a {
                        s[a][b] += w * da * (x[b] - means[c][b]);
                    }
                } else {
                    s[a][a] += w * da * da;
                }
            }
        }
    }
    for s in &mut scatter {
        for a in 0..p {
            for b in 0..a {
                s[b][a] = s[a][b];
            }
        }
    }
    let nf = n as f64;
    let pf = p as f64;
    let diag = |v: Vec<f64>| -> Vec<Vec<f64>> {
        let mut m = vec![vec![0.0; p]; p];
        for (a, x) in v.into_iter().enumerate() {
            m[a][a] = x;
        }
        m
    };
    let trace = |s: &Vec<Vec<f64>>| (0..p).map(|a| s[a][a]).sum::<f64>();
    let total = || {
        let mut w = vec![vec![0.0; p]; p];
        for s in &scatter {
            for a in 0..p {
                for b in 0..p {
                    w[a][b] += s[a][b];
                }
            }
        }
        w
    };
    let mut cov: Vec<Vec<Vec<f64>>> = match structure {
        CovarianceStructure::Eii => {
            let lambda = trace(&total()) / (nf * pf);
            vec![diag(vec![lambda; p]); k]
        }
        CovarianceStructure::Vii => (0..k).map(|c| diag(vec![trace(&scatter[c]) / (nk[c] * pf); p])).collect(),
        CovarianceStructure::Eei => {
            let w = total();
            vec![diag((0..p).map(|a| w[a][a] / nf).collect()); k]
        }
        CovarianceStructure::Vvi => (0..k).map(|c| diag((0..p).map(|a| scatter[c][a][a] / nk[c]).collect())).collect(),
        CovarianceStructure::Eee => {
            let w = total();
            vec![w.iter().map(|r| r.iter().map(|v| v / nf).collect()).collect(); k]
        }
        CovarianceStructure::Vvv => scatter
            .iter()
            .zip(&nk)
            .map(|(s, &m)| s.iter().map(|r| r.iter().map(|v| v / m).collect()).collect())
            .collect(),
    };
    for c in &mut cov {
        for (a, row) in c.iter_mut().enumerate() {
            row[a] += ridge;
        }
    }
    Some(Params { weights: nk.iter().map(|m| m / nf).collect(), means, cov })
}

/// E-step: membership probabilities and the log-likelihood; `None` when a
/// covariance matrix is not positive definite.
fn e_step(points: &[Vec<f64>], params: &Params) -> Option<(Vec<Vec<f64>>, f64)> {
    let k = params.weights.len();
    let p = params.means[0].len();
    let ln2pi = (2.0 * std::f64::consts::PI).ln();
    let mut factors = Vec::with_capacity(k);
    let mut consts = Vec::with_capacity(k);
    for c in 0..k {
        let l = cholesky(&params.cov[c])?;
        let log_det: f64 = 2.0 * (0..p).map(|a| l[a][a].ln()).sum::<f64>();
        consts.push(params.weights[c].ln() - 0.5 * (p as f64 * ln2pi + log_det));
        factors.push(l);
    }
    let mut ll = 0.0;
    let mut z = Vec::with_capacity(points.len());
    let mut d = vec![0.0; p];
    for x in points {
        let mut logs = Vec::with_capacity(k);
        for c in 0..k {
            for a in 0..p {
                d[a] = x[a] - params.means[c][a];
            }
            let q = crate::spd::quadratic_form(&factors[c], &d);
            logs.push(consts[c] - 0.5 * q);
        }
        let m = logs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let s: f64 = logs.iter().map(|v| (v - m).exp()).sum();
        let lse = m + s.ln();
        ll += lse;
        z.push(logs.iter().map(|v| (v - lse).exp()).collect());
    }
    ll.is_finite().then_some((z, ll))
}

/// One EM run from membership probabilities `z0`; `None` when it breaks
/// down (an empty component or a singular covariance).
fn em(points: &[Vec<f64>], z0: Vec<Vec<f64>>, options: &MixtureOptions) -> Option<GaussianMixture> {
    let mut z = z0;
    let mut params = m_step(points, &z, options.structure, options.ridge)?;
    let (mut znew, mut ll) = e_step(points, &params)?;
    let mut converged = false;
    let mut iterations = 0;
    while iterations < options.max_iterations {
        iterations += 1;
        z = znew;
        let next = m_step(points, &z, options.structure, options.ridge)?;
        let (zn, lln) = e_step(points, &next)?;
        params = next;
        znew = zn;
        let change = lln - ll;
        ll = lln;
        if change.abs() <= options.tolerance * (1.0 + ll.abs()) {
            converged = true;
            break;
        }
    }
    Some(GaussianMixture {
        structure: options.structure,
        weights: params.weights,
        means: params.means,
        covariances: params.cov,
        probabilities: znew,
        log_likelihood: ll,
        iterations,
        converged,
    })
}

/// Hard 0/1 membership probabilities from labels `0..k`.
fn one_hot(labels: &[usize], k: usize) -> Vec<Vec<f64>> {
    labels
        .iter()
        .map(|&l| {
            let mut z = vec![0.0; k];
            z[l.min(k - 1)] = 1.0;
            z
        })
        .collect()
}

fn check_points(points: &[Vec<f64>]) -> Result<usize> {
    let Some(first) = points.first() else {
        return Err(StatError::InsufficientData { required: 2, found: 0 });
    };
    let p = first.len();
    if p == 0 || points.iter().any(|x| x.len() != p) {
        return Err(invalid("every point needs the same, non-zero number of coordinates"));
    }
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "Gaussian mixture data" });
    }
    Ok(p)
}

/// Fit a `K`-component Gaussian mixture by EM from the starts in `options`,
/// keeping the start with the largest log-likelihood.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `K = 0`, `K > n`, no starts, a
/// negative ridge or points of different dimensions;
/// [`StatError::NonFiniteInput`] for NaN or ±∞;
/// [`StatError::ConvergenceFailure`] when every start breaks down (a
/// component left with almost no weight, or a covariance matrix that is not
/// positive definite — add a ridge or use a simpler structure).
pub fn fit_gaussian_mixture(points: &[Vec<f64>], options: &MixtureOptions) -> Result<GaussianMixture> {
    check_points(points)?;
    let (n, k) = (points.len(), options.k);
    if k == 0 || k > n {
        return Err(invalid(format!("the number of components must be between 1 and the number of rows ({n})")));
    }
    if options.starts == 0 {
        return Err(invalid("use at least one start"));
    }
    if options.ridge.is_nan() || options.ridge < 0.0 {
        return Err(invalid("the ridge must be zero or positive"));
    }
    let starts = if k == 1 { 1 } else { options.starts };
    let mut best: Option<GaussianMixture> = None;
    for s in 0..starts {
        let labels = if k == 1 {
            vec![0; n]
        } else if s == 0 {
            match options.start {
                MixtureStart::KMeans => {
                    k_means(points, &KMeansOptions { seed: options.seed, ..KMeansOptions::new(k) })?.assignments
                }
                MixtureStart::Hierarchical => hierarchical_clustering(points, Linkage::Ward)?.cut(k)?,
            }
        } else {
            let seed = options.seed.wrapping_add((s as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            k_means(points, &KMeansOptions { seed, starts: 1, ..KMeansOptions::new(k) })?.assignments
        };
        if let Some(fit) = em(points, one_hot(&labels, k), options) {
            if best.as_ref().is_none_or(|b| fit.log_likelihood > b.log_likelihood + 1e-10 * b.log_likelihood.abs()) {
                best = Some(fit);
            }
        }
    }
    let fit = best
        .ok_or(StatError::ConvergenceFailure { routine: "Gaussian mixture EM", iterations: options.max_iterations })?;
    Ok(relabel(fit))
}

/// Run EM from given membership probabilities `z0` (`n` rows of `K`, e.g. a
/// hard partition), with no further starts. The result is not relabelled.
///
/// # Errors
/// As [`fit_gaussian_mixture`]; also `z0` of the wrong shape.
pub fn fit_gaussian_mixture_from(
    points: &[Vec<f64>],
    z0: Vec<Vec<f64>>,
    options: &MixtureOptions,
) -> Result<GaussianMixture> {
    check_points(points)?;
    if z0.len() != points.len() || z0.iter().any(|z| z.len() != z0[0].len()) || z0[0].is_empty() {
        return Err(invalid("the starting probabilities need one row of K values per point"));
    }
    em(points, z0, options)
        .ok_or(StatError::ConvergenceFailure { routine: "Gaussian mixture EM", iterations: options.max_iterations })
}

/// Number components by the first point whose most likely component they are.
fn relabel(mut fit: GaussianMixture) -> GaussianMixture {
    let k = fit.k();
    let mut order: Vec<usize> = Vec::with_capacity(k);
    for c in fit.classification() {
        if !order.contains(&c) {
            order.push(c);
        }
    }
    for c in 0..k {
        if !order.contains(&c) {
            order.push(c);
        }
    }
    fit.weights = order.iter().map(|&c| fit.weights[c]).collect();
    fit.means = order.iter().map(|&c| fit.means[c].clone()).collect();
    fit.covariances = order.iter().map(|&c| fit.covariances[c].clone()).collect();
    fit.probabilities = fit.probabilities.iter().map(|z| order.iter().map(|&c| z[c]).collect()).collect();
    fit
}

/// One candidate model of a [`MixtureSelection`].
#[derive(Debug, Clone, PartialEq)]
pub struct MixtureCandidate {
    /// Covariance structure.
    pub structure: CovarianceStructure,
    /// Number of components.
    pub k: usize,
    /// The fit, or why it failed.
    pub fit: std::result::Result<GaussianMixture, String>,
}

impl MixtureCandidate {
    /// BIC of the fit (`None` if it failed).
    #[must_use]
    pub fn bic(&self) -> Option<f64> {
        self.fit.as_ref().ok().map(GaussianMixture::bic)
    }
}

/// Every structure and number of components tried, and the one with the
/// smallest BIC.
#[derive(Debug, Clone, PartialEq)]
pub struct MixtureSelection {
    /// Candidates, by structure and then by number of components.
    pub candidates: Vec<MixtureCandidate>,
    /// Index of the chosen candidate in `candidates`.
    pub best: usize,
}

impl MixtureSelection {
    /// The chosen mixture.
    #[must_use]
    pub fn chosen(&self) -> &GaussianMixture {
        match &self.candidates[self.best].fit {
            Ok(fit) => fit,
            Err(_) => unreachable!("the chosen candidate was fitted"),
        }
    }
}

/// Fit every structure in `structures` with every number of components in
/// `ks`, each with the starts and settings of `base`, and choose the
/// smallest BIC. Fits run in parallel and are deterministic.
///
/// # Errors
/// No structures or numbers of components, invalid data or settings (as in
/// [`fit_gaussian_mixture`]), or every fit failing.
pub fn select_gaussian_mixture(
    points: &[Vec<f64>],
    ks: &[usize],
    structures: &[CovarianceStructure],
    base: &MixtureOptions,
) -> Result<MixtureSelection> {
    use rayon::prelude::*;
    check_points(points)?;
    if ks.is_empty() || structures.is_empty() {
        return Err(invalid("choose at least one covariance structure and number of components"));
    }
    if let Some(&k) = ks.iter().find(|&&k| k == 0 || k > points.len()) {
        return Err(invalid(format!("cannot fit {k} components to {} rows", points.len())));
    }
    let jobs: Vec<(CovarianceStructure, usize)> =
        structures.iter().flat_map(|&s| ks.iter().map(move |&k| (s, k))).collect();
    let candidates: Vec<MixtureCandidate> = jobs
        .par_iter()
        .map(|&(structure, k)| MixtureCandidate {
            structure,
            k,
            fit: fit_gaussian_mixture(points, &MixtureOptions { k, structure, ..*base }).map_err(|e| e.to_string()),
        })
        .collect();
    let best = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.bic().map(|b| (i, b)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .ok_or_else(|| {
            let why = candidates.iter().find_map(|c| c.fit.as_ref().err().cloned()).unwrap_or_default();
            invalid(format!("no mixture could be fitted: {why}"))
        })?;
    Ok(MixtureSelection { candidates, best })
}

#[cfg(test)]
mod tests;
