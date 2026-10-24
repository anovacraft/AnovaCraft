//! Multivariate control charts for several correlated quality
//! characteristics measured on the same items (Montgomery, *Introduction to
//! Statistical Quality Control*, 7th ed., ch. 11).
//!
//! Every chart takes the first `m` points (subgroups, or single
//! observations) as the **reference** sample that estimates the in-control
//! mean vector and covariance matrix; points after it are **monitored**
//! against those estimates. Without a separate reference the whole sample is
//! the reference (a phase I study).
//!
//! * **Hotelling T²** (Hotelling 1947). Subgroups of size `n ≥ 2`:
//!   `T²ⱼ = n(x̄ⱼ − x̿)ᵀ S̄⁻¹ (x̄ⱼ − x̿)` with `S̄` the average within-subgroup
//!   covariance matrix; reference points have the upper limit
//!   `p(m − 1)(n − 1)/(mn − m − p + 1) · F_α(p, mn − m − p + 1)` and monitored
//!   points `p(m + 1)(n − 1)/(mn − m − p + 1) · F_α(p, mn − m − p + 1)`
//!   (Montgomery eqs. 11.19–11.20; Alt 1985). Individual observations:
//!   `T²ᵢ = (xᵢ − x̄)ᵀ S⁻¹ (xᵢ − x̄)` with the sample covariance `S`; reference
//!   points follow a scaled beta distribution, with upper limit
//!   `((m − 1)²/m) · B_α(p/2, (m − p − 1)/2)` (Tracy, Young & Mason 1992,
//!   *J. Quality Technology* 24), and monitored points
//!   `p(m + 1)(m − 1)/(m(m − p)) · F_α(p, m − p)` (Montgomery eqs. 11.23–11.24).
//!   `F_α` and `B_α` are upper-`α` quantiles. There is no lower limit.
//! * **Contributions** of each variable to a point's T²:
//!   `dⱼ = T² − T²₍₋ⱼ₎`, where `T²₍₋ⱼ₎` leaves variable `j` out (its row and
//!   column of the covariance matrix and its deviation); a large `dⱼ`
//!   (compare with `χ²_α(1)`) marks a variable that drives the signal (Runger,
//!   Alt & Montgomery 1996, "Contributors to a multivariate statistical
//!   process control chart signal", *Communications in Statistics — Theory
//!   and Methods* 25; Montgomery §11.3.2).
//! * **Generalized variance** `|Sⱼ|` (Alt 1985, "Multivariate quality
//!   control", *Encyclopedia of Statistical Sciences* 6; Montgomery
//!   §11.3.3): with `b₁ = Π_{i=1}^p (n − i) / (n − 1)^p` and
//!   `b₂ = Π_{i=1}^p (n − i) / (n − 1)^{2p} · [Π_{j=1}^p (n − j + 2) − Π_{j=1}^p (n − j)]`,
//!   `E|S| = b₁|Σ|` and `Var|S| = b₂|Σ|²`; estimating `|Σ|` by `|S̄|/b₁`, the
//!   limits are `|S̄|/b₁ · (b₁ ± 3√b₂)` around the center `|S̄|`, with a
//!   negative lower limit set to zero. Needs `n > p`.
//! * **MEWMA** (Lowry, Woodall, Champ & Rigdon 1992, "A multivariate
//!   exponentially weighted moving average control chart", *Technometrics*
//!   34): `Zᵢ = λ(xᵢ − x̄) + (1 − λ)Zᵢ₋₁`, `Z₀ = 0`, plotted as
//!   `T²ᵢ = Zᵢᵀ Σ_{Zᵢ}⁻¹ Zᵢ` with the exact covariance
//!   `Σ_{Zᵢ} = λ/(2 − λ) · [1 − (1 − λ)^{2i}] · S`, against an upper limit `H`.
//!   `H` is given, or found for a target in-control average run length by
//!   simulation ([`mewma_limit_for_arl`]).
//!
//! # Failure modes
//! An empty sample, observations of unequal length, unequal subgroup sizes, a
//! reference too small for the limits (`m > p + 1` for individuals,
//! `mn − m − p + 1 > 0` for subgroups, `n > p` for `|S|`), a singular
//! covariance matrix, `α ∉ (0, 1)`, `λ ∉ (0, 1]` or `H ≤ 0` are errors.

use std::sync::Mutex;

use stat_core::discrete::beta_quantile;
use stat_core::rng::Rng;
use stat_core::{ContinuousDistribution, FisherSnedecor, Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Whether a point belongs to the reference sample that estimated the
/// limits, or is monitored against them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartPhase {
    /// One of the first `m` points (phase I).
    Reference,
    /// A later point (phase II).
    Monitoring,
}

/// One plotted point of a multivariate chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MultivariatePoint {
    /// Plotted statistic (T², |S| or the MEWMA T²).
    pub statistic: f64,
    /// Lower control limit (zero when the chart has none).
    pub lower: f64,
    /// Upper control limit.
    pub upper: f64,
    /// Reference or monitored point.
    pub phase: ChartPhase,
    /// The statistic lies outside the limits.
    pub signal: bool,
}

/// Lower-triangular Cholesky factor of a symmetric positive definite matrix
/// (row-major `p × p`), or `None` when it is not positive definite.
fn cholesky(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let p = a.len();
    let mut l = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in 0..=i {
            let s: f64 = a[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if i == j {
                // Relative tolerance against the diagonal, so a constant or
                // collinear column is reported as singular.
                if s.is_nan() || s <= 1e-12 * a[i][i].abs() {
                    return None;
                }
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    Some(l)
}

/// `dᵀ A⁻¹ d` from the Cholesky factor `L` of `A`.
fn quadratic_form(l: &[Vec<f64>], d: &[f64]) -> f64 {
    let p = d.len();
    let mut y = vec![0.0; p];
    let mut total = 0.0;
    for i in 0..p {
        let s = d[i] - (0..i).map(|k| l[i][k] * y[k]).sum::<f64>();
        y[i] = s / l[i][i];
        total += y[i] * y[i];
    }
    total
}

fn singular() -> StatError {
    invalid("the covariance matrix is singular: a variable is constant or a linear combination of the others")
}

/// Contributions `dⱼ = T² − T²₍₋ⱼ₎` of each variable to
/// `T² = scale · dᵀ Σ⁻¹ d` (see the module notes).
///
/// # Errors
/// A singular covariance matrix or a length mismatch.
pub fn t2_contributions(covariance: &[Vec<f64>], deviation: &[f64], scale: f64) -> Result<Vec<f64>> {
    let p = deviation.len();
    if covariance.len() != p || covariance.iter().any(|r| r.len() != p) {
        return Err(invalid("the covariance matrix must match the number of variables"));
    }
    let full = scale * quadratic_form(&cholesky(covariance).ok_or_else(singular)?, deviation);
    (0..p)
        .map(|j| {
            if p == 1 {
                return Ok(full);
            }
            let keep: Vec<usize> = (0..p).filter(|&i| i != j).collect();
            let sub: Vec<Vec<f64>> = keep.iter().map(|&r| keep.iter().map(|&c| covariance[r][c]).collect()).collect();
            let d: Vec<f64> = keep.iter().map(|&i| deviation[i]).collect();
            let without = scale * quadratic_form(&cholesky(&sub).ok_or_else(singular)?, &d);
            Ok(full - without)
        })
        .collect()
}

fn check_alpha(alpha: f64) -> Result<()> {
    if alpha > 0.0 && alpha < 1.0 {
        Ok(())
    } else {
        Err(invalid(format!("the false-alarm probability must lie strictly between 0 and 1 (got {alpha})")))
    }
}

/// Validates subgroups (all non-empty, the same size, every observation with
/// `p` finite values) and returns `(n, p)`.
fn shape(subgroups: &[Vec<Vec<f64>>]) -> Result<(usize, usize)> {
    let first = subgroups.first().ok_or(StatError::InsufficientData { required: 1, found: 0 })?;
    let n = first.len();
    let p = first.first().map_or(0, Vec::len);
    if n == 0 || p == 0 {
        return Err(invalid("subgroups must hold at least one observation of at least one variable"));
    }
    if subgroups.iter().any(|g| g.len() != n) {
        return Err(invalid("every subgroup must have the same number of observations"));
    }
    for obs in subgroups.iter().flatten() {
        if obs.len() != p {
            return Err(invalid("every observation must have a value for each variable"));
        }
        if obs.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "multivariate chart values" });
        }
    }
    Ok((n, p))
}

fn reference_count(reference: Option<usize>, total: usize) -> Result<usize> {
    match reference {
        None => Ok(total),
        Some(m) if m >= 1 && m <= total => Ok(m),
        Some(m) => Err(invalid(format!("the reference sample must hold between 1 and {total} points (got {m})"))),
    }
}

fn mean_vector(obs: &[&[f64]], p: usize) -> Vec<f64> {
    let n = obs.len() as f64;
    (0..p).map(|j| obs.iter().map(|o| o[j]).sum::<f64>() / n).collect()
}

/// Sample covariance (divisor `n − 1`) of observations.
fn covariance(obs: &[&[f64]], p: usize) -> Vec<Vec<f64>> {
    let mean = mean_vector(obs, p);
    let denom = obs.len() as f64 - 1.0;
    let mut c = vec![vec![0.0; p]; p];
    for o in obs {
        for i in 0..p {
            for j in 0..=i {
                c[i][j] += (o[i] - mean[i]) * (o[j] - mean[j]);
            }
        }
    }
    for i in 0..p {
        for j in 0..=i {
            c[i][j] /= denom;
            c[j][i] = c[i][j];
        }
    }
    c
}

fn average_covariance(subgroups: &[Vec<Vec<f64>>], p: usize) -> Vec<Vec<f64>> {
    let mut total = vec![vec![0.0; p]; p];
    for g in subgroups {
        let obs: Vec<&[f64]> = g.iter().map(Vec::as_slice).collect();
        let c = covariance(&obs, p);
        for i in 0..p {
            for j in 0..p {
                total[i][j] += c[i][j];
            }
        }
    }
    let m = subgroups.len() as f64;
    total.iter_mut().flatten().for_each(|v| *v /= m);
    total
}

fn determinant(a: &[Vec<f64>]) -> f64 {
    cholesky(a).map_or(0.0, |l| (0..a.len()).map(|i| l[i][i] * l[i][i]).product())
}

/// A Hotelling T² chart.
#[derive(Debug, Clone, PartialEq)]
pub struct HotellingChart {
    /// Number of variables `p`.
    pub variables: usize,
    /// Observations per subgroup `n` (1 for individual observations).
    pub subgroup_size: usize,
    /// Points in the reference sample `m`.
    pub reference_count: usize,
    /// False-alarm probability `α` of each limit.
    pub alpha: f64,
    /// Reference mean vector `x̄` (or grand mean `x̿`).
    pub mean: Vec<f64>,
    /// Reference covariance matrix `S` (or average subgroup covariance `S̄`).
    pub covariance: Vec<Vec<f64>>,
    /// Upper limit of the reference points.
    pub reference_limit: f64,
    /// Upper limit of the monitored points (`NaN` when every point is in
    /// the reference).
    pub monitoring_limit: f64,
    /// One point per subgroup or observation.
    pub points: Vec<MultivariatePoint>,
    /// Deviation of each point's mean from the reference mean.
    pub deviations: Vec<Vec<f64>>,
}

impl HotellingChart {
    /// Contributions of each variable to point `i`'s T² (see
    /// [`t2_contributions`]).
    ///
    /// # Errors
    /// An index out of range.
    pub fn contributions(&self, i: usize) -> Result<Vec<f64>> {
        let d = self.deviations.get(i).ok_or_else(|| invalid(format!("no point {i}")))?;
        t2_contributions(&self.covariance, d, self.subgroup_size as f64)
    }
}

/// Hotelling T² chart of subgroups (`subgroups[j][obs][variable]`; subgroups
/// of one observation give the individuals chart), with the first
/// `reference` points as the reference sample (all when `None`).
///
/// # Errors
/// See the module notes.
pub fn hotelling_t2_chart(subgroups: &[Vec<Vec<f64>>], reference: Option<usize>, alpha: f64) -> Result<HotellingChart> {
    check_alpha(alpha)?;
    let (n, p) = shape(subgroups)?;
    let total = subgroups.len();
    let m = reference_count(reference, total)?;
    let (mf, nf, pf) = (m as f64, n as f64, p as f64);
    let reference_set = &subgroups[..m];
    let means: Vec<Vec<f64>> =
        subgroups.iter().map(|g| mean_vector(&g.iter().map(Vec::as_slice).collect::<Vec<_>>(), p)).collect();
    let ref_means: Vec<&[f64]> = means[..m].iter().map(Vec::as_slice).collect();
    let mean = mean_vector(&ref_means, p);
    let (cov, reference_limit, monitoring_limit) = if n == 1 {
        if m <= p + 1 {
            return Err(invalid(format!(
                "a T² chart of individual observations of {p} variables needs more than {} reference observations",
                p + 1
            )));
        }
        let cov = covariance(&ref_means, p);
        let beta = beta_quantile(pf / 2.0, (mf - pf - 1.0) / 2.0, 1.0 - alpha)?;
        let phase1 = (mf - 1.0).powi(2) / mf * beta;
        let phase2 =
            FisherSnedecor::new(pf, mf - pf)?.quantile(1.0 - alpha)? * pf * (mf + 1.0) * (mf - 1.0) / (mf * (mf - pf));
        (cov, phase1, phase2)
    } else {
        let df2 = mf * nf - mf - pf + 1.0;
        if df2 <= 0.0 {
            return Err(invalid(format!(
                "too few reference subgroups for {p} variables: need m(n − 1) ≥ p (m = {m}, n = {n})"
            )));
        }
        let cov = average_covariance(reference_set, p);
        let f = FisherSnedecor::new(pf, df2)?.quantile(1.0 - alpha)?;
        let scale = pf * (nf - 1.0) / df2;
        (cov, scale * (mf - 1.0) * f, scale * (mf + 1.0) * f)
    };
    let l = cholesky(&cov).ok_or_else(singular)?;
    let deviations: Vec<Vec<f64>> = means.iter().map(|x| x.iter().zip(&mean).map(|(a, b)| a - b).collect()).collect();
    let points = deviations
        .iter()
        .enumerate()
        .map(|(j, d)| {
            let statistic = nf * quadratic_form(&l, d);
            let phase = if j < m { ChartPhase::Reference } else { ChartPhase::Monitoring };
            let upper = if phase == ChartPhase::Reference { reference_limit } else { monitoring_limit };
            MultivariatePoint { statistic, lower: 0.0, upper, phase, signal: statistic > upper }
        })
        .collect();
    Ok(HotellingChart {
        variables: p,
        subgroup_size: n,
        reference_count: m,
        alpha,
        mean,
        covariance: cov,
        reference_limit,
        monitoring_limit: if m < total { monitoring_limit } else { f64::NAN },
        points,
        deviations,
    })
}

/// The constants `(b₁, b₂)` of the generalized variance for subgroups of
/// size `n` and `p` variables (see the module notes).
#[must_use]
pub fn generalized_variance_constants(n: usize, p: usize) -> (f64, f64) {
    let (nf, pf) = (n as f64, p as f64);
    let prod = |f: &dyn Fn(f64) -> f64| (1..=p).map(|i| f(i as f64)).product::<f64>();
    let lower = prod(&|i| nf - i);
    let b1 = lower / (nf - 1.0).powf(pf);
    let b2 = lower / (nf - 1.0).powf(2.0 * pf) * (prod(&|j| nf - j + 2.0) - lower);
    (b1, b2)
}

/// A generalized variance (|S|) chart.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneralizedVarianceChart {
    /// Number of variables `p`.
    pub variables: usize,
    /// Observations per subgroup `n`.
    pub subgroup_size: usize,
    /// Subgroups in the reference sample `m`.
    pub reference_count: usize,
    /// `b₁` and `b₂`.
    pub constants: (f64, f64),
    /// Grand mean of the reference subgroups.
    pub mean: Vec<f64>,
    /// Average covariance matrix `S̄` of the reference subgroups.
    pub covariance: Vec<Vec<f64>>,
    /// Center line `|S̄|`.
    pub center: f64,
    /// Lower control limit (zero when the formula gives a negative value).
    pub lower: f64,
    /// Upper control limit.
    pub upper: f64,
    /// One point per subgroup: its `|Sⱼ|`.
    pub points: Vec<MultivariatePoint>,
}

/// Generalized variance chart: each subgroup's `|Sⱼ|` against limits
/// `|S̄|/b₁ · (b₁ ± width·√b₂)` (width 3 in Alt's chart).
///
/// # Errors
/// See the module notes; also `width ≤ 0`.
pub fn generalized_variance_chart(
    subgroups: &[Vec<Vec<f64>>],
    reference: Option<usize>,
    width: f64,
) -> Result<GeneralizedVarianceChart> {
    let (n, p) = shape(subgroups)?;
    if n <= p {
        return Err(invalid(format!(
            "a generalized variance chart of {p} variables needs subgroups of more than {p} observations"
        )));
    }
    if width.is_nan() || width <= 0.0 {
        return Err(invalid("the limit width must be positive"));
    }
    let m = reference_count(reference, subgroups.len())?;
    let avg = average_covariance(&subgroups[..m], p);
    let all: Vec<&[f64]> = subgroups[..m].iter().flatten().map(Vec::as_slice).collect();
    let mean = mean_vector(&all, p);
    let center = determinant(&avg);
    if center <= 0.0 {
        return Err(singular());
    }
    let (b1, b2) = generalized_variance_constants(n, p);
    let sigma = center / b1;
    let upper = sigma * (b1 + width * b2.sqrt());
    let lower = (sigma * (b1 - width * b2.sqrt())).max(0.0);
    let points = subgroups
        .iter()
        .enumerate()
        .map(|(j, g)| {
            let obs: Vec<&[f64]> = g.iter().map(Vec::as_slice).collect();
            let statistic = determinant(&covariance(&obs, p));
            let phase = if j < m { ChartPhase::Reference } else { ChartPhase::Monitoring };
            let signal = statistic > upper || (lower > 0.0 && statistic < lower);
            MultivariatePoint { statistic, lower, upper, phase, signal }
        })
        .collect();
    Ok(GeneralizedVarianceChart {
        variables: p,
        subgroup_size: n,
        reference_count: m,
        constants: (b1, b2),
        mean,
        covariance: avg,
        center,
        lower,
        upper,
        points,
    })
}

/// How the MEWMA upper limit is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MewmaLimit {
    /// A given limit `H`.
    Value(f64),
    /// The `H` whose in-control average run length is this many points
    /// ([`mewma_limit_for_arl`]).
    InControlArl(f64),
}

/// A MEWMA chart.
#[derive(Debug, Clone, PartialEq)]
pub struct MewmaChart {
    /// Number of variables `p`.
    pub variables: usize,
    /// Smoothing weight `λ`.
    pub lambda: f64,
    /// Upper control limit `H`.
    pub limit: f64,
    /// Target in-control average run length, when `H` was found from it.
    pub target_arl: Option<f64>,
    /// Observations in the reference sample `m`.
    pub reference_count: usize,
    /// Reference mean vector.
    pub mean: Vec<f64>,
    /// Reference covariance matrix `S`.
    pub covariance: Vec<Vec<f64>>,
    /// One point per observation.
    pub points: Vec<MultivariatePoint>,
    /// The MEWMA vectors `Zᵢ`.
    pub vectors: Vec<Vec<f64>>,
    /// Variance factors `cᵢ = λ/(2 − λ)·[1 − (1 − λ)^{2i}]`.
    pub factors: Vec<f64>,
}

impl MewmaChart {
    /// Contributions of each variable to point `i`'s statistic, by
    /// [`t2_contributions`] applied to `Zᵢ` with covariance `cᵢS`.
    ///
    /// # Errors
    /// An index out of range.
    pub fn contributions(&self, i: usize) -> Result<Vec<f64>> {
        let z = self.vectors.get(i).ok_or_else(|| invalid(format!("no point {i}")))?;
        t2_contributions(&self.covariance, z, 1.0 / self.factors[i])
    }
}

/// MEWMA chart of individual observations (`observations[i][variable]`)
/// with the first `reference` observations estimating the mean and
/// covariance (all when `None`).
///
/// # Errors
/// See the module notes.
pub fn mewma_chart(
    observations: &[Vec<f64>],
    reference: Option<usize>,
    lambda: f64,
    limit: MewmaLimit,
) -> Result<MewmaChart> {
    if lambda.is_nan() || lambda <= 0.0 || lambda > 1.0 {
        return Err(invalid(format!("the smoothing weight λ must lie in (0, 1] (got {lambda})")));
    }
    let groups: Vec<Vec<Vec<f64>>> = observations.iter().map(|o| vec![o.clone()]).collect();
    let (_, p) = shape(&groups)?;
    let m = reference_count(reference, observations.len())?;
    if m <= p {
        return Err(invalid(format!("a MEWMA chart of {p} variables needs more than {p} reference observations")));
    }
    let (h, target_arl) = match limit {
        MewmaLimit::Value(h) if h > 0.0 && h.is_finite() => (h, None),
        MewmaLimit::Value(h) => return Err(invalid(format!("the upper limit H must be positive (got {h})"))),
        MewmaLimit::InControlArl(arl) => (mewma_limit_for_arl(p, lambda, arl)?, Some(arl)),
    };
    let obs: Vec<&[f64]> = observations[..m].iter().map(Vec::as_slice).collect();
    let mean = mean_vector(&obs, p);
    let cov = covariance(&obs, p);
    let l = cholesky(&cov).ok_or_else(singular)?;
    let mut z = vec![0.0; p];
    let mut decay = 1.0;
    let (mut points, mut vectors, mut factors) = (Vec::new(), Vec::new(), Vec::new());
    for (i, x) in observations.iter().enumerate() {
        for k in 0..p {
            z[k] = lambda * (x[k] - mean[k]) + (1.0 - lambda) * z[k];
        }
        decay *= (1.0 - lambda) * (1.0 - lambda);
        let c = lambda / (2.0 - lambda) * (1.0 - decay);
        let statistic = quadratic_form(&l, &z) / c;
        let phase = if i < m { ChartPhase::Reference } else { ChartPhase::Monitoring };
        points.push(MultivariatePoint { statistic, lower: 0.0, upper: h, phase, signal: statistic > h });
        vectors.push(z.clone());
        factors.push(c);
    }
    Ok(MewmaChart {
        variables: p,
        lambda,
        limit: h,
        target_arl,
        reference_count: m,
        mean,
        covariance: cov,
        points,
        vectors,
        factors,
    })
}

/// Most simulated in-control runs used by [`mewma_limit_for_arl`].
pub const MEWMA_SIMULATION_RUNS: usize = 5000;

/// Fewest simulated in-control runs used by [`mewma_limit_for_arl`] (at the
/// largest run length, [`MEWMA_MAX_ARL`]).
pub const MEWMA_MIN_SIMULATION_RUNS: usize = 500;

/// Largest in-control average run length [`mewma_limit_for_arl`] accepts,
/// so that the simulation stays interactive.
pub const MEWMA_MAX_ARL: f64 = 2000.0;

/// Simulated points (runs × horizon) the limit search aims for.
const MEWMA_SIMULATION_BUDGET: f64 = 1e7;

/// The MEWMA limit `H` whose in-control average run length is `arl`, for `p`
/// variables and weight `λ`, by simulation: `R` in-control runs (independent
/// standard normal vectors, as the statistic is invariant to the mean and
/// covariance) from a fixed seed, each followed for up to `10·arl` points,
/// with `H` solved by bisection so that the mean run length equals `arl`.
/// The work is bounded: `R = 10⁷ / (10·arl)` clamped to
/// [`MEWMA_MIN_SIMULATION_RUNS`]..=[`MEWMA_SIMULATION_RUNS`], so up to
/// `arl = 200` there are 5000 runs (run-length standard error about
/// `arl/√5000`, 1.4%, so `H` is good to a few hundredths) and at the largest
/// accepted `arl` ([`MEWMA_MAX_ARL`]) 500 runs (about 4.5%). Results are
/// cached.
///
/// # Errors
/// `p = 0`, `λ ∉ (0, 1]` or `arl` outside `[2, 2000]`.
pub fn mewma_limit_for_arl(p: usize, lambda: f64, arl: f64) -> Result<f64> {
    if p == 0 {
        return Err(invalid("give at least one variable"));
    }
    if lambda.is_nan() || lambda <= 0.0 || lambda > 1.0 {
        return Err(invalid(format!("the smoothing weight λ must lie in (0, 1] (got {lambda})")));
    }
    if !(2.0..=MEWMA_MAX_ARL).contains(&arl) {
        return Err(invalid(format!(
            "the in-control run length must lie between 2 and {MEWMA_MAX_ARL} (got {arl}); \
             larger values take too long to simulate"
        )));
    }
    /// Variables, λ bits and run-length bits of a simulated limit.
    type LimitKey = (usize, u64, u64);
    static CACHE: Mutex<Vec<(LimitKey, f64)>> = Mutex::new(Vec::new());
    let key = (p, lambda.to_bits(), arl.to_bits());
    if let Some(&(_, h)) = CACHE.lock().ok().and_then(|c| c.iter().find(|(k, _)| *k == key).copied()).as_ref() {
        return Ok(h);
    }
    let horizon = (10.0 * arl).ceil() as usize;
    let runs =
        ((MEWMA_SIMULATION_BUDGET / horizon as f64) as usize).clamp(MEWMA_MIN_SIMULATION_RUNS, MEWMA_SIMULATION_RUNS);
    let mut rng = Rng::new(0x4D45_5745_4D41 ^ p as u64);
    // For each run, the points where the running maximum of T² rises.
    let mut records: Vec<Vec<(usize, f64)>> = Vec::with_capacity(runs);
    let mut z = vec![0.0; p];
    for _ in 0..runs {
        z.iter_mut().for_each(|v| *v = 0.0);
        let mut decay = 1.0;
        let mut best = f64::NEG_INFINITY;
        let mut run = Vec::new();
        for i in 1..=horizon {
            let mut ss = 0.0;
            for v in &mut z {
                *v = lambda * rng.normal() + (1.0 - lambda) * *v;
                ss += *v * *v;
            }
            decay *= (1.0 - lambda) * (1.0 - lambda);
            let t2 = ss / (lambda / (2.0 - lambda) * (1.0 - decay));
            if t2 > best {
                best = t2;
                run.push((i, t2));
            }
        }
        records.push(run);
    }
    let mean_run_length = |h: f64| -> f64 {
        let total: usize =
            records.iter().map(|run| run.iter().find(|(_, t2)| *t2 > h).map_or(horizon, |&(i, _)| i)).sum();
        total as f64 / runs as f64
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    while mean_run_length(hi) < arl {
        hi *= 2.0;
        if hi > 1e6 {
            return Err(StatError::ConvergenceFailure { routine: "MEWMA limit", iterations: 20 });
        }
    }
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if mean_run_length(mid) < arl {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-9 * hi {
            break;
        }
    }
    let h = 0.5 * (lo + hi);
    if let Ok(mut c) = CACHE.lock() {
        c.push((key, h));
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two correlated variables, 12 observations (hand-built).
    fn individuals() -> Vec<Vec<f64>> {
        [
            (10.0, 20.0),
            (11.0, 22.5),
            (9.5, 19.0),
            (10.5, 21.5),
            (10.2, 19.8),
            (9.8, 20.4),
            (10.9, 21.9),
            (9.1, 18.2),
            (10.4, 20.3),
            (10.1, 20.6),
            (9.7, 19.6),
            (11.2, 22.0),
        ]
        .iter()
        .map(|&(a, b)| vec![a, b])
        .collect()
    }

    #[test]
    fn cholesky_and_quadratic_form() {
        let a = vec![vec![4.0, 2.0], vec![2.0, 3.0]];
        let l = cholesky(&a).unwrap();
        // a⁻¹ = [[3, −2], [−2, 4]]/8; d = (1, 2): (3 − 8 + 16)/8.
        assert!((quadratic_form(&l, &[1.0, 2.0]) - 11.0 / 8.0).abs() < 1e-14);
        assert!((determinant(&a) - 8.0).abs() < 1e-12);
        assert!(cholesky(&[vec![1.0, 1.0], vec![1.0, 1.0]]).is_none());
    }

    #[test]
    fn individuals_t2_matches_hand_computation() {
        let x = individuals();
        let groups: Vec<Vec<Vec<f64>>> = x.iter().map(|o| vec![o.clone()]).collect();
        let c = hotelling_t2_chart(&groups, None, 0.05).unwrap();
        assert_eq!((c.variables, c.subgroup_size, c.reference_count), (2, 1, 12));
        // T² by an explicit 2 × 2 inverse.
        let s = &c.covariance;
        let det = s[0][0] * s[1][1] - s[0][1] * s[1][0];
        for (i, pt) in c.points.iter().enumerate() {
            let d = &c.deviations[i];
            let t2 = (s[1][1] * d[0] * d[0] - 2.0 * s[0][1] * d[0] * d[1] + s[0][0] * d[1] * d[1]) / det;
            assert!((pt.statistic - t2).abs() < 1e-10);
        }
        // Σ T² over the reference sample = (m − 1)p for the sample covariance.
        let total: f64 = c.points.iter().map(|p| p.statistic).sum();
        assert!((total - 22.0).abs() < 1e-9);
        // Beta limit: (m − 1)²/m · B(0.95; 1, 4.5), and B(1, b) has the
        // closed-form quantile 1 − (1 − q)^{1/b}.
        let beta = 1.0 - 0.05_f64.powf(1.0 / 4.5);
        assert!((c.reference_limit - 121.0 / 12.0 * beta).abs() < 1e-9);
        assert!(c.monitoring_limit.is_nan());
        // Contributions: with p = 2 each is T² minus the other variable's
        // squared standardized deviation.
        let k = c.contributions(3).unwrap();
        let t2 = c.points[3].statistic;
        let d = &c.deviations[3];
        assert!((k[0] - (t2 - d[1] * d[1] / s[1][1])).abs() < 1e-10);
        assert!((k[1] - (t2 - d[0] * d[0] / s[0][0])).abs() < 1e-10);
    }

    #[test]
    fn monitoring_limits_use_f() {
        let mut x = individuals();
        x.push(vec![12.5, 19.0]); // off the correlation: should signal
        let groups: Vec<Vec<Vec<f64>>> = x.iter().map(|o| vec![o.clone()]).collect();
        let c = hotelling_t2_chart(&groups, Some(12), 0.01).unwrap();
        let f = FisherSnedecor::new(2.0, 10.0).unwrap().quantile(0.99).unwrap();
        assert!((c.monitoring_limit - 2.0 * 13.0 * 11.0 / (12.0 * 10.0) * f).abs() < 1e-9);
        let last = c.points.last().unwrap();
        assert_eq!(last.phase, ChartPhase::Monitoring);
        assert!(last.signal);
        // The reference statistics are unchanged by the monitored point.
        let all = hotelling_t2_chart(&groups[..12], None, 0.01).unwrap();
        assert!((all.points[4].statistic - c.points[4].statistic).abs() < 1e-12);
        let contrib = c.contributions(12).unwrap();
        assert!(contrib.iter().all(|v| v.is_finite()));
        assert!(hotelling_t2_chart(&groups[..3], None, 0.01).is_err(), "m must exceed p + 1");
    }

    #[test]
    fn subgroup_t2_and_generalized_variance() {
        // Six subgroups of four, two variables.
        let raw = [
            [(5.0, 3.0), (5.5, 3.4), (4.8, 2.9), (5.2, 3.3)],
            [(5.1, 3.1), (4.9, 2.8), (5.4, 3.5), (5.0, 3.0)],
            [(4.7, 2.7), (5.3, 3.3), (5.0, 3.2), (5.2, 3.0)],
            [(5.6, 3.6), (5.4, 3.3), (5.8, 3.8), (5.5, 3.5)],
            [(4.9, 3.0), (5.1, 3.1), (5.0, 2.9), (5.3, 3.4)],
            [(5.2, 3.2), (4.6, 2.6), (5.7, 3.7), (5.0, 3.1)],
        ];
        let groups: Vec<Vec<Vec<f64>>> = raw.iter().map(|g| g.iter().map(|&(a, b)| vec![a, b]).collect()).collect();
        let c = hotelling_t2_chart(&groups, None, 0.005).unwrap();
        let df2 = 6.0 * 4.0 - 6.0 - 2.0 + 1.0;
        let f = FisherSnedecor::new(2.0, df2).unwrap().quantile(0.995).unwrap();
        assert!((c.reference_limit - 2.0 * 5.0 * 3.0 / df2 * f).abs() < 1e-9);
        // T² = n·dᵀS̄⁻¹d by an explicit inverse.
        let s = &c.covariance;
        let det = s[0][0] * s[1][1] - s[0][1] * s[1][0];
        let d = &c.deviations[3];
        let t2 = 4.0 * (s[1][1] * d[0] * d[0] - 2.0 * s[0][1] * d[0] * d[1] + s[0][0] * d[1] * d[1]) / det;
        assert!((c.points[3].statistic - t2).abs() < 1e-10);
        // |S| chart: b₁ = (n−1)(n−2)/(n−1)² and b₂ from the definition.
        let (b1, b2) = generalized_variance_constants(4, 2);
        assert!((b1 - 6.0 / 9.0).abs() < 1e-15);
        assert!((b2 - 6.0 / 81.0 * (5.0 * 4.0 - 6.0)).abs() < 1e-15);
        let g = generalized_variance_chart(&groups, None, 3.0).unwrap();
        assert!((g.center - det).abs() < 1e-12);
        assert!((g.upper - det / b1 * (b1 + 3.0 * b2.sqrt())).abs() < 1e-12);
        assert_eq!(g.lower, 0.0);
        let s3 = covariance(&groups[2].iter().map(Vec::as_slice).collect::<Vec<_>>(), 2);
        assert!((g.points[2].statistic - (s3[0][0] * s3[1][1] - s3[0][1] * s3[1][0])).abs() < 1e-12);
        // p = 1: b₁ = 1, b₂ = 2/(n − 1), the moments of s².
        let (b1, b2) = generalized_variance_constants(6, 1);
        assert!((b1 - 1.0).abs() < 1e-15 && (b2 - 0.4).abs() < 1e-15);
        assert!(generalized_variance_chart(&groups, None, 0.0).is_err());
        let pairs: Vec<Vec<Vec<f64>>> = groups.iter().map(|g| g[..2].to_vec()).collect();
        assert!(generalized_variance_chart(&pairs, None, 3.0).is_err(), "n must exceed p");
    }

    #[test]
    fn mewma_recursion_and_limit() {
        let x = individuals();
        let c = mewma_chart(&x, None, 0.2, MewmaLimit::Value(9.0)).unwrap();
        // λ = 1 reduces to the individuals T² with the same S.
        let one = mewma_chart(&x, None, 1.0, MewmaLimit::Value(9.0)).unwrap();
        let groups: Vec<Vec<Vec<f64>>> = x.iter().map(|o| vec![o.clone()]).collect();
        let t2 = hotelling_t2_chart(&groups, None, 0.05).unwrap();
        for (a, b) in one.points.iter().zip(&t2.points) {
            assert!((a.statistic - b.statistic).abs() < 1e-10);
        }
        // First point: Z₁ = λd₁, c₁ = λ², so T²₁ equals the individual T².
        assert!((c.points[0].statistic - t2.points[0].statistic).abs() < 1e-10);
        assert!((c.factors[1] - 0.2 / 1.8 * (1.0 - 0.8_f64.powi(4))).abs() < 1e-15);
        let k = c.contributions(5).unwrap();
        assert_eq!(k.len(), 2);
        assert!(mewma_chart(&x, None, 0.0, MewmaLimit::Value(9.0)).is_err());
        assert!(mewma_chart(&x, None, 0.2, MewmaLimit::Value(-1.0)).is_err());
    }

    /// The simulated limit for p = 2, λ = 0.1 and an in-control ARL of 200
    /// against an independent NumPy simulation (10⁵ runs, separate random
    /// stream) of the same exact-covariance chart.
    #[test]
    fn mewma_simulated_limit() {
        let h = mewma_limit_for_arl(2, 0.1, 200.0).unwrap();
        assert!((h - MEWMA_REFERENCE).abs() < 0.1, "{h}");
        // Cached and monotone in the run length.
        assert_eq!(mewma_limit_for_arl(2, 0.1, 200.0).unwrap(), h);
        assert!(mewma_limit_for_arl(2, 0.1, 100.0).unwrap() < h);
        assert!(mewma_limit_for_arl(2, 0.1, 1.0).is_err());
    }

    const MEWMA_REFERENCE: f64 = 8.782;

    /// The run length is capped at 2000 so that the simulation stays
    /// interactive: larger values are refused with a message naming the cap,
    /// and the cap itself is computed quickly (500 runs) and agrees with an
    /// independent NumPy simulation (4000 runs, separate random stream:
    /// H = 13.85 for p = 2, λ = 0.1).
    #[test]
    fn mewma_limit_run_length_is_capped() {
        let e = mewma_limit_for_arl(3, 0.1, 1e5).unwrap_err().to_string();
        assert!(e.contains("2000"), "{e}");
        let start = std::time::Instant::now();
        let h = mewma_limit_for_arl(2, 0.1, MEWMA_MAX_ARL).unwrap();
        let elapsed = start.elapsed();
        assert!((h - 13.85).abs() < 0.3, "{h}");
        assert!(elapsed.as_secs_f64() < 5.0, "{elapsed:?}");
    }
}
