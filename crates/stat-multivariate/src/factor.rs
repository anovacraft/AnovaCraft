//! Exploratory factor analysis of a correlation or covariance matrix.
//!
//! # Model
//! The common factor model writes the `p × p` covariance (or correlation)
//! matrix of the variables as `Σ = Λ Φ Λᵀ + Ψ`: `Λ` (`p × m`) holds the
//! loadings of the variables on `m` common factors, `Φ` the correlations of
//! the factors (the identity before rotation) and `Ψ` the diagonal matrix of
//! uniquenesses (variance not shared with the other variables). The
//! **communality** of variable `j` is `hⱼ = Σₖ λⱼₖ²` (for orthogonal
//! factors), and `hⱼ + ψⱼ = sⱼⱼ`.
//!
//! # Extraction
//! * **Maximum likelihood** (Lawley & Maxwell, *Factor Analysis as a
//!   Statistical Method*, 2nd ed., 1971; Jöreskog, "Some contributions to
//!   maximum likelihood factor analysis", *Psychometrika* 32, 1967). For given
//!   uniquenesses let `θ₁ ≥ … ≥ θₚ` be the eigenvalues, with unit vectors
//!   `ωₖ`, of `Ψ^{-1/2} S Ψ^{-1/2}`. The likelihood is maximised over `Λ` by
//!   `Λ = Ψ^{1/2} Ω_m (Θ_m − I)^{1/2}`, which leaves the discrepancy
//!   `F(Ψ) = Σ_{k>m} (θₖ − ln θₖ − 1)` (equal to
//!   `ln|Σ̂| − ln|S| + tr(Σ̂⁻¹S) − p`) to minimise over the uniquenesses. Its
//!   gradient is `∂F/∂ψⱼ = (Λ̂Λ̂ᵀ + Ψ − S)ⱼⱼ / ψⱼ²`. The minimisation is a
//!   projected quasi-Newton (BFGS) iteration inside the box
//!   `0.005 sⱼⱼ ≤ ψⱼ ≤ sⱼⱼ`, started from `ψⱼ = (1 − m/2p) / (S⁻¹)ⱼⱼ`
//!   (Jöreskog's start). A uniqueness that stops on the lower bound is a
//!   **Heywood case** (a communality of essentially the whole variance); it is
//!   flagged rather than allowed to become zero or negative.
//! * **Principal axis** (iterated principal factors; Harman, *Modern Factor
//!   Analysis*, 3rd ed., 1976, ch. 8): the diagonal of `S` is replaced by
//!   communality estimates — first the squared multiple correlations
//!   `sⱼⱼ − 1/(S⁻¹)ⱼⱼ` — the `m` leading eigenvectors give
//!   `Λ = V_m Λ_m^{1/2}`, and the row sums of squares of `Λ` become the next
//!   communalities until they change by less than the tolerance. A
//!   communality that reaches the variance is held there and flagged as a
//!   Heywood case: its diagonal entry stays at `sⱼⱼ`, and its row of final
//!   loadings is scaled to a communality of exactly `sⱼⱼ` (uniqueness 0;
//!   Bartlett scores are then unavailable).
//!
//! # Test of fit (maximum likelihood)
//! Bartlett's corrected statistic `(n − 1 − (2p + 5)/6 − 2m/3) · F(Ψ̂)` is
//! referred to χ² with `((p − m)² − p − m)/2` degrees of freedom (Lawley &
//! Maxwell §4.3). Small p-values mean `m` factors do not reproduce the
//! correlations.
//!
//! # Rotation
//! * **Varimax** (Kaiser, "The varimax criterion for analytic rotation in
//!   factor analysis", *Psychometrika* 23, 1958) maximises the variance of
//!   the squared loadings in each column. With Kaiser normalisation each row
//!   is divided by `√hⱼ` before rotating and multiplied back afterwards. The
//!   rotation `T` is found by the singular-value iteration
//!   `B = Aᵀ(Z³ − Z diag(1ᵀZ²)/p)`, `T = U Vᵀ` for `B = U Σ Vᵀ`, until `Σσ`
//!   stops increasing.
//! * **Promax** (Hendrickson & White, "Promax: a quick method for rotation to
//!   oblique simple structure", *Brit. J. Statist. Psych.* 17, 1964): from the
//!   varimax loadings `V`, the target `Q = V ∘ |V|^{k−1}` (power `k`, 4 by
//!   default) is fitted by least squares, `U = (VᵀV)⁻¹VᵀQ`, whose columns are
//!   rescaled so that `diag((UᵀU)⁻¹) = 1`. The **pattern** loadings are `VU`,
//!   the factor correlations `Φ = (UᵀU)⁻¹` and the **structure** loadings
//!   (correlations of variables with factors) `VU Φ`.
//!
//! Each factor's sign is chosen so that its loadings sum to a positive value.
//!
//! # Factor scores
//! For standardized (correlation) or centred (covariance) data `z`:
//! regression (Thomson/Thurstone) scores `z S⁻¹ Λ Φ`; Bartlett's weighted
//! least-squares scores `z Ψ⁻¹ Λ (Λᵀ Ψ⁻¹ Λ)⁻¹` (Bartlett, "The statistical
//! conception of mental factors", *Brit. J. Psych.* 28, 1937).

use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_core::eigen::symmetric_eigen;
use stat_core::{Matrix, Result, StatError, svd};

use crate::spd;

/// How the factors are extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Extraction {
    /// Maximum likelihood (with a chi-square test of fit).
    #[default]
    MaximumLikelihood,
    /// Principal axis factoring with iterated communalities.
    PrincipalAxis,
}

/// Rotation of the extracted loadings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    /// The unrotated solution.
    None,
    /// Varimax (orthogonal).
    #[default]
    Varimax,
    /// Promax (oblique; varimax followed by a power target).
    Promax,
}

/// How factor scores are computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScoreMethod {
    /// Regression (Thomson) scores: `z S⁻¹ Λ Φ`.
    #[default]
    Regression,
    /// Bartlett's weighted least-squares scores.
    Bartlett,
}

/// Settings of [`factor_analysis`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FactorOptions {
    /// Extraction method.
    pub extraction: Extraction,
    /// Number of factors `m` (at least 1, below the number of variables).
    pub factors: usize,
    /// Rotation.
    pub rotation: Rotation,
    /// Kaiser normalisation of the rows before varimax (also inside promax).
    pub kaiser_normalize: bool,
    /// Power `k` of the promax target (above 1; 4 by default).
    pub promax_power: f64,
    /// Most iterations of the extraction.
    pub max_iterations: usize,
    /// Convergence tolerance (largest change in a communality for principal
    /// axis; largest projected gradient for maximum likelihood).
    pub tolerance: f64,
}

impl FactorOptions {
    /// Maximum likelihood with `factors` factors, varimax with Kaiser
    /// normalisation, promax power 4.
    #[must_use]
    pub fn new(factors: usize) -> Self {
        Self {
            extraction: Extraction::MaximumLikelihood,
            factors,
            rotation: Rotation::Varimax,
            kaiser_normalize: true,
            promax_power: 4.0,
            max_iterations: 1000,
            tolerance: 1e-9,
        }
    }
}

/// Bartlett-corrected likelihood-ratio test that `m` factors suffice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitTest {
    /// Minimised discrepancy `F(Ψ̂)`.
    pub discrepancy: f64,
    /// `(n − 1 − (2p + 5)/6 − 2m/3) F(Ψ̂)`.
    pub chi_square: f64,
    /// `((p − m)² − p − m) / 2`.
    pub df: f64,
    /// Upper-tail χ² probability (`None` when `df = 0`: the model is just
    /// identified and cannot be tested).
    pub p_value: Option<f64>,
}

/// A fitted factor model. Loading matrices are rows of variables
/// (`p` rows of `m` values).
#[derive(Debug, Clone, PartialEq)]
pub struct FactorFit {
    /// Options used.
    pub options: FactorOptions,
    /// The analysed matrix `S` (correlation or covariance).
    pub matrix: Vec<Vec<f64>>,
    /// Number of observations behind `S`.
    pub n_obs: usize,
    /// Eigenvalues of `S`, largest first (for the scree plot).
    pub eigenvalues: Vec<f64>,
    /// Starting communalities (squared multiple correlations, in the units of `S`).
    pub initial_communalities: Vec<f64>,
    /// Unrotated loadings `Λ` (`p × m`).
    pub unrotated: Vec<Vec<f64>>,
    /// Rotated pattern loadings (equal to `unrotated` with no rotation).
    pub pattern: Vec<Vec<f64>>,
    /// Structure loadings `pattern · Φ` (equal to `pattern` for orthogonal factors).
    pub structure: Vec<Vec<f64>>,
    /// Factor correlations `Φ` (`m × m`; the identity for orthogonal factors).
    pub factor_correlation: Vec<Vec<f64>>,
    /// Rotation `T` with `pattern = unrotated · T` (`m × m`).
    pub rotation_matrix: Vec<Vec<f64>>,
    /// Communalities `hⱼ` (row sums of squares of the unrotated loadings).
    pub communalities: Vec<f64>,
    /// Uniquenesses `ψⱼ = sⱼⱼ − hⱼ`.
    pub uniquenesses: Vec<f64>,
    /// Variables whose communality reached its bound (Heywood cases).
    pub heywood: Vec<bool>,
    /// Iterations of the extraction.
    pub iterations: usize,
    /// Whether the extraction met its tolerance.
    pub converged: bool,
    /// Test of fit (maximum likelihood only).
    pub fit_test: Option<FitTest>,
}

impl FactorFit {
    /// Number of variables.
    #[must_use]
    pub fn n_variables(&self) -> usize {
        self.matrix.len()
    }

    /// Number of factors.
    #[must_use]
    pub fn n_factors(&self) -> usize {
        self.options.factors
    }

    /// Total variance `tr S` (`p` for a correlation matrix).
    #[must_use]
    pub fn total_variance(&self) -> f64 {
        (0..self.matrix.len()).map(|j| self.matrix[j][j]).sum()
    }

    /// Sum of squared loadings of each factor in `loadings`.
    #[must_use]
    pub fn sums_of_squares(loadings: &[Vec<f64>]) -> Vec<f64> {
        let m = loadings.first().map_or(0, Vec::len);
        (0..m).map(|k| loadings.iter().map(|r| r[k] * r[k]).sum()).collect()
    }

    /// Whether the rotation is oblique (factors may correlate).
    #[must_use]
    pub fn oblique(&self) -> bool {
        self.options.rotation == Rotation::Promax && self.n_factors() > 1
    }

    /// Weights `W` (`p × m`) giving factor scores `z W` for standardized
    /// (correlation analysis) or centred (covariance analysis) data `z`.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] if `S` (regression) or `ΛᵀΨ⁻¹Λ`
    /// (Bartlett) is not positive definite, or for Bartlett scores when a
    /// uniqueness is zero (a principal-axis Heywood case).
    pub fn score_weights(&self, method: ScoreMethod) -> Result<Vec<Vec<f64>>> {
        let (p, m) = (self.n_variables(), self.n_factors());
        match method {
            ScoreMethod::Regression => {
                let l = spd::cholesky(&self.matrix)
                    .ok_or_else(|| invalid("factor scores: the matrix is not positive definite"))?;
                let inv = spd::inverse(&l);
                Ok(matmul(&inv, &self.structure))
            }
            ScoreMethod::Bartlett => {
                if let Some(j) = (0..p).find(|&j| self.uniquenesses[j].is_nan() || self.uniquenesses[j] <= 0.0) {
                    return Err(invalid(format!(
                        "Bartlett scores need every uniqueness above zero, but variable {} is a Heywood case \
                         (its communality reached its variance); use regression scores",
                        j + 1
                    )));
                }
                let a = &self.pattern;
                let wa: Vec<Vec<f64>> =
                    (0..p).map(|j| a[j].iter().map(|v| v / self.uniquenesses[j]).collect()).collect();
                let mut g = vec![vec![0.0; m]; m];
                for r in 0..m {
                    for c in 0..m {
                        g[r][c] = (0..p).map(|j| a[j][r] * wa[j][c]).sum();
                    }
                }
                let l =
                    spd::cholesky(&g).ok_or_else(|| invalid("Bartlett scores: the loadings are not of full rank"))?;
                Ok(matmul(&wa, &spd::inverse(&l)))
            }
        }
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn matmul(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let k = b.len();
    let m = b.first().map_or(0, Vec::len);
    a.iter().map(|row| (0..m).map(|c| (0..k).map(|i| row[i] * b[i][c]).sum()).collect()).collect()
}

fn transpose(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let m = a.first().map_or(0, Vec::len);
    (0..m).map(|c| a.iter().map(|r| r[c]).collect()).collect()
}

fn identity(m: usize) -> Vec<Vec<f64>> {
    (0..m).map(|i| (0..m).map(|j| f64::from(u8::from(i == j))).collect()).collect()
}

/// Factor analysis of the correlation or covariance matrix `s` of `n_obs`
/// observations.
///
/// # Errors
/// [`StatError::InvalidArgument`] for a matrix that is not square,
/// symmetric and positive definite, no factors or too many (`m ≥ p`, or for
/// maximum likelihood more than leave non-negative degrees of freedom),
/// fewer observations than variables plus one (maximum likelihood), or a
/// promax power not above 1; [`StatError::NonFiniteInput`] for NaN or ±∞.
pub fn factor_analysis(s: &[Vec<f64>], n_obs: usize, options: &FactorOptions) -> Result<FactorFit> {
    let p = s.len();
    let m = options.factors;
    if s.iter().any(|r| r.len() != p) {
        return Err(invalid("factor analysis needs a square matrix"));
    }
    if s.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "factor analysis matrix" });
    }
    if p < 2 {
        return Err(invalid("factor analysis needs at least two variables"));
    }
    if m == 0 || m >= p {
        return Err(invalid(format!("choose between 1 and {} factors for {p} variables", p - 1)));
    }
    if options.rotation == Rotation::Promax
        && options.promax_power.partial_cmp(&1.0) != Some(std::cmp::Ordering::Greater)
    {
        return Err(invalid("the promax power must be above 1"));
    }
    let scale = (0..p).map(|j| s[j][j].abs()).fold(0.0, f64::max);
    for i in 0..p {
        for j in 0..i {
            if (s[i][j] - s[j][i]).abs() > 1e-9 * scale {
                return Err(invalid("factor analysis needs a symmetric matrix"));
            }
        }
    }
    let l = spd::cholesky(s).ok_or_else(|| {
        invalid("the correlation matrix is singular: a variable is constant or a combination of the others")
    })?;
    let inv = spd::inverse(&l);
    let smc: Vec<f64> = (0..p).map(|j| s[j][j] - 1.0 / inv[j][j]).collect();
    let (eigenvalues, _) = symmetric_eigen(s)?;
    let df = (((p - m) * (p - m)) as f64 - (p + m) as f64) / 2.0;

    let (mut unrotated, uniquenesses, heywood, iterations, converged, fit_test) = match options.extraction {
        Extraction::MaximumLikelihood => {
            if df < 0.0 {
                let most = (1..p).filter(|&k| (p - k) * (p - k) >= p + k).max().unwrap_or(0);
                return Err(invalid(format!(
                    "{m} factors are too many for maximum likelihood with {p} variables (at most {most})"
                )));
            }
            if n_obs <= p {
                return Err(StatError::InsufficientData { required: p + 1, found: n_obs });
            }
            let start: Vec<f64> = (0..p).map(|j| (1.0 - 0.5 * m as f64 / p as f64) / inv[j][j]).collect();
            let ml = maximum_likelihood(s, m, start, options)?;
            let pf = p as f64;
            let mf = m as f64;
            let factor = n_obs as f64 - 1.0 - (2.0 * pf + 5.0) / 6.0 - 2.0 * mf / 3.0;
            let chi_square = (factor * ml.discrepancy).max(0.0);
            let p_value = if df > 0.0 { Some(ChiSquared::new(df)?.sf(chi_square)?) } else { None };
            let test = FitTest { discrepancy: ml.discrepancy, chi_square, df, p_value };
            (ml.loadings, ml.psi, ml.heywood, ml.iterations, ml.converged, Some(test))
        }
        Extraction::PrincipalAxis => {
            let pa = principal_axis(s, m, &smc, options)?;
            (pa.loadings, pa.psi, pa.heywood, pa.iterations, pa.converged, None)
        }
    };
    orient_columns(&mut unrotated, None);
    let communalities: Vec<f64> = unrotated.iter().map(|r| r.iter().map(|v| v * v).sum()).collect();

    let (mut pattern, mut rotation_matrix, mut phi) = match (options.rotation, m) {
        (Rotation::None, _) | (_, 1) => (unrotated.clone(), identity(m), identity(m)),
        (Rotation::Varimax, _) => {
            let (v, t) = varimax(&unrotated, options.kaiser_normalize)?;
            (v, t, identity(m))
        }
        (Rotation::Promax, _) => promax(&unrotated, options.kaiser_normalize, options.promax_power)?,
    };
    // Orient rotated factors: flip pattern columns, T columns, Φ rows/columns.
    let flips = orient_columns(&mut pattern, Some(&mut rotation_matrix));
    for (k, &flip) in flips.iter().enumerate() {
        if flip {
            for c in 0..m {
                if c != k {
                    phi[k][c] = -phi[k][c];
                    phi[c][k] = -phi[c][k];
                }
            }
        }
    }
    let structure = matmul(&pattern, &phi);
    Ok(FactorFit {
        options: *options,
        matrix: s.to_vec(),
        n_obs,
        eigenvalues,
        initial_communalities: smc,
        unrotated,
        pattern,
        structure,
        factor_correlation: phi,
        rotation_matrix,
        communalities,
        uniquenesses,
        heywood,
        iterations,
        converged,
        fit_test,
    })
}

/// Make each column's sum positive; flip the matching column of `t` too.
/// Returns which columns were flipped.
fn orient_columns(a: &mut [Vec<f64>], mut t: Option<&mut Vec<Vec<f64>>>) -> Vec<bool> {
    let m = a.first().map_or(0, Vec::len);
    (0..m)
        .map(|k| {
            let sum: f64 = a.iter().map(|r| r[k]).sum();
            let flip = sum < 0.0;
            if flip {
                a.iter_mut().for_each(|r| r[k] = -r[k]);
                if let Some(t) = t.as_deref_mut() {
                    t.iter_mut().for_each(|r| r[k] = -r[k]);
                }
            }
            flip
        })
        .collect()
}

struct Extracted {
    loadings: Vec<Vec<f64>>,
    psi: Vec<f64>,
    heywood: Vec<bool>,
    iterations: usize,
    converged: bool,
    discrepancy: f64,
}

/// The ML discrepancy, its gradient and the loadings for uniquenesses `psi`.
fn ml_objective(s: &[Vec<f64>], m: usize, psi: &[f64]) -> Result<(f64, Vec<f64>, Vec<Vec<f64>>)> {
    let p = s.len();
    let d: Vec<f64> = psi.iter().map(|v| 1.0 / v.sqrt()).collect();
    let star: Vec<Vec<f64>> = (0..p).map(|i| (0..p).map(|j| s[i][j] * d[i] * d[j]).collect()).collect();
    let (theta, omega) = symmetric_eigen(&star)?;
    let f: f64 = theta[m..].iter().map(|&t| if t > 0.0 { t - t.ln() - 1.0 } else { f64::INFINITY }).sum();
    let loadings: Vec<Vec<f64>> = (0..p)
        .map(|j| (0..m).map(|k| psi[j].sqrt() * omega[k][j] * (theta[k] - 1.0).max(0.0).sqrt()).collect())
        .collect();
    let grad = (0..p)
        .map(|j| {
            let h: f64 = loadings[j].iter().map(|v| v * v).sum();
            (h + psi[j] - s[j][j]) / (psi[j] * psi[j])
        })
        .collect();
    Ok((f, grad, loadings))
}

/// Minimise the ML discrepancy over the uniquenesses by projected BFGS.
fn maximum_likelihood(s: &[Vec<f64>], m: usize, start: Vec<f64>, options: &FactorOptions) -> Result<Extracted> {
    let p = s.len();
    let lower: Vec<f64> = (0..p).map(|j| 0.005 * s[j][j]).collect();
    let upper: Vec<f64> = (0..p).map(|j| s[j][j]).collect();
    let clamp = |x: &mut [f64]| {
        for j in 0..p {
            x[j] = x[j].clamp(lower[j], upper[j]);
        }
    };
    let mut x = start;
    clamp(&mut x);
    let (mut f, mut g, _) = ml_objective(s, m, &x)?;
    let mut h = identity(p);
    let mut converged = false;
    let mut iterations = 0;
    let mut previous_free: Vec<bool> = Vec::new();
    while iterations < options.max_iterations {
        iterations += 1;
        let free: Vec<bool> =
            (0..p).map(|j| !((x[j] <= lower[j] && g[j] > 0.0) || (x[j] >= upper[j] && g[j] < 0.0))).collect();
        // Scale-free projected gradient: ψ² ∂F/∂ψ is in units of variance.
        let pg = (0..p).filter(|&j| free[j]).map(|j| (g[j] * x[j]).abs() / s[j][j]).fold(0.0, f64::max);
        if pg < options.tolerance {
            converged = true;
            break;
        }
        if free != previous_free {
            h = identity(p);
            previous_free.clone_from(&free);
        }
        let mut d: Vec<f64> = (0..p)
            .map(|i| if free[i] { -(0..p).filter(|&j| free[j]).map(|j| h[i][j] * g[j]).sum::<f64>() } else { 0.0 })
            .collect();
        let mut slope: f64 = (0..p).map(|j| d[j] * g[j]).sum();
        if slope >= 0.0 {
            h = identity(p);
            d = (0..p).map(|j| if free[j] { -g[j] } else { 0.0 }).collect();
            slope = (0..p).map(|j| d[j] * g[j]).sum();
            if slope >= 0.0 {
                converged = true;
                break;
            }
        }
        // Backtracking (Armijo) along the projected path.
        let mut t = 1.0;
        let mut accepted = None;
        for _ in 0..60 {
            let mut trial: Vec<f64> = (0..p).map(|j| x[j] + t * d[j]).collect();
            clamp(&mut trial);
            let (ft, gt, _) = ml_objective(s, m, &trial)?;
            let decrease: f64 = (0..p).map(|j| g[j] * (trial[j] - x[j])).sum();
            if ft.is_finite() && ft <= f + 1e-4 * decrease {
                accepted = Some((trial, ft, gt));
                break;
            }
            t *= 0.5;
        }
        let Some((xn, fn_, gn)) = accepted else {
            // No decrease along the descent direction: at the minimum to
            // working precision.
            converged = pg < 1e-5;
            break;
        };
        let sv: Vec<f64> = (0..p).map(|j| xn[j] - x[j]).collect();
        let yv: Vec<f64> = (0..p).map(|j| gn[j] - g[j]).collect();
        let sy: f64 = sv.iter().zip(&yv).map(|(a, b)| a * b).sum();
        let small_step = sv.iter().zip(&x).all(|(a, b)| a.abs() <= 1e-15 * b.abs().max(1e-300));
        x = xn;
        let df_ = (f - fn_).abs();
        f = fn_;
        g = gn;
        if small_step || df_ <= 1e-16 * f.abs().max(1e-300) && sy <= 0.0 {
            converged = true;
            break;
        }
        if sy > 1e-12 * sv.iter().map(|v| v * v).sum::<f64>().sqrt() * yv.iter().map(|v| v * v).sum::<f64>().sqrt() {
            // BFGS update of the inverse Hessian.
            let hy: Vec<f64> = (0..p).map(|i| (0..p).map(|j| h[i][j] * yv[j]).sum()).collect();
            let yhy: f64 = yv.iter().zip(&hy).map(|(a, b)| a * b).sum();
            let rho = 1.0 / sy;
            for i in 0..p {
                for j in 0..p {
                    h[i][j] += rho * ((1.0 + rho * yhy) * sv[i] * sv[j] - hy[i] * sv[j] - sv[i] * hy[j]);
                }
            }
        }
    }
    let (f, _, loadings) = ml_objective(s, m, &x)?;
    let heywood = (0..p).map(|j| x[j] <= lower[j] * (1.0 + 1e-6)).collect();
    Ok(Extracted { loadings, psi: x, heywood, iterations, converged, discrepancy: f })
}

/// Iterated principal-axis factoring from the starting communalities `h0`.
fn principal_axis(s: &[Vec<f64>], m: usize, h0: &[f64], options: &FactorOptions) -> Result<Extracted> {
    let p = s.len();
    let mut h = h0.to_vec();
    let mut heywood = vec![false; p];
    let mut loadings: Vec<Vec<f64>> = Vec::new();
    let mut converged = false;
    let mut iterations = 0;
    while iterations < options.max_iterations {
        iterations += 1;
        let mut reduced = s.to_vec();
        for j in 0..p {
            reduced[j][j] = h[j];
        }
        let (values, vectors) = symmetric_eigen(&reduced)?;
        loadings = (0..p).map(|j| (0..m).map(|k| vectors[k][j] * values[k].max(0.0).sqrt()).collect()).collect();
        let mut change = 0.0_f64;
        for j in 0..p {
            let mut next: f64 = loadings[j].iter().map(|v: &f64| v * v).sum();
            heywood[j] = next >= s[j][j];
            if heywood[j] {
                next = s[j][j];
            }
            change = change.max((next - h[j]).abs() / s[j][j]);
            h[j] = next;
        }
        if change < options.tolerance {
            converged = true;
            break;
        }
    }
    // Hold each Heywood variable at its variance: the eigenvectors of the
    // reduced matrix can imply a communality above sⱼⱼ, so scale that row of
    // loadings back to a communality of exactly sⱼⱼ (uniqueness 0).
    for j in 0..p {
        let h: f64 = loadings[j].iter().map(|v| v * v).sum();
        if h >= s[j][j] && h > 0.0 {
            heywood[j] = true;
            let f = (s[j][j] / h).sqrt();
            loadings[j].iter_mut().for_each(|v| *v *= f);
        }
    }
    // Loadings consistent with the final communalities.
    let psi = (0..p)
        .map(|j| if heywood[j] { 0.0 } else { s[j][j] - loadings[j].iter().map(|v| v * v).sum::<f64>() })
        .collect();
    Ok(Extracted { loadings, psi, heywood, iterations, converged, discrepancy: f64::NAN })
}

/// A dense matrix as rows.
type Rows = Vec<Vec<f64>>;

fn to_matrix(a: &[Vec<f64>]) -> Result<Matrix> {
    Matrix::from_rows(a)
}

fn from_matrix(a: &Matrix) -> Vec<Vec<f64>> {
    (0..a.nrows()).map(|i| a.row(i)).collect()
}

/// Varimax rotation of `a` (`p × m`): returns the rotated loadings and the
/// orthogonal rotation `T` (`rotated = a · T`).
///
/// # Errors
/// From the singular value decomposition (not expected for finite input).
pub fn varimax(a: &[Vec<f64>], kaiser_normalize: bool) -> Result<(Rows, Rows)> {
    let p = a.len();
    let m = a.first().map_or(0, Vec::len);
    let sc: Vec<f64> = a
        .iter()
        .map(|r| if kaiser_normalize { r.iter().map(|v| v * v).sum::<f64>().sqrt() } else { 1.0 })
        .map(|v| if v > 0.0 { v } else { 1.0 })
        .collect();
    let x: Vec<Vec<f64>> = a.iter().zip(&sc).map(|(r, s)| r.iter().map(|v| v / s).collect()).collect();
    let mut t = identity(m);
    let mut d = 0.0;
    for _ in 0..10_000 {
        let z = matmul(&x, &t);
        let col_ss: Vec<f64> = (0..m).map(|k| z.iter().map(|r| r[k] * r[k]).sum()).collect();
        let target: Vec<Vec<f64>> =
            z.iter().map(|r| (0..m).map(|k| r[k].powi(3) - r[k] * col_ss[k] / p as f64).collect()).collect();
        let b = matmul(&transpose(&x), &target);
        let dec = svd(&to_matrix(&b)?)?;
        let u = from_matrix(&dec.u);
        let v = from_matrix(&dec.v);
        t = matmul(&u, &transpose(&v));
        let past = d;
        d = dec.singular_values.iter().sum();
        if d <= past * (1.0 + 1e-13) {
            break;
        }
    }
    let rotated = matmul(&x, &t).into_iter().zip(&sc).map(|(r, s)| r.into_iter().map(|v| v * s).collect()).collect();
    Ok((rotated, t))
}

/// Promax rotation of `a`: returns the pattern loadings, the rotation `T`
/// (`pattern = a · T`) and the factor correlations `Φ`.
///
/// # Errors
/// [`StatError::InvalidArgument`] when the varimax loadings are not of
/// full column rank.
pub fn promax(a: &[Vec<f64>], kaiser_normalize: bool, power: f64) -> Result<(Rows, Rows, Rows)> {
    let (v, tv) = varimax(a, kaiser_normalize)?;
    let q: Vec<Vec<f64>> = v.iter().map(|r| r.iter().map(|x| x * x.abs().powf(power - 1.0)).collect()).collect();
    let vt = transpose(&v);
    let vtv = matmul(&vt, &v);
    let rank_error = || invalid("promax: the loadings are not of full column rank");
    let l = spd::cholesky(&vtv).ok_or_else(rank_error)?;
    let mut u = matmul(&spd::inverse(&l), &matmul(&vt, &q));
    let utu = matmul(&transpose(&u), &u);
    let lu = spd::cholesky(&utu).ok_or_else(rank_error)?;
    let utu_inv = spd::inverse(&lu);
    let m = u.len();
    for row in &mut u {
        for k in 0..m {
            row[k] *= utu_inv[k][k].sqrt();
        }
    }
    let pattern = matmul(&v, &u);
    let t = matmul(&tv, &u);
    let utu = matmul(&transpose(&u), &u);
    let lu = spd::cholesky(&utu).ok_or_else(rank_error)?;
    let phi = spd::inverse(&lu);
    Ok((pattern, t, phi))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulated ability-test scores (`tests/data/factor_reference.py`).
    pub(crate) fn abilities() -> Vec<Vec<f64>> {
        include_str!("../tests/data/factor_abilities.csv")
            .lines()
            .skip(1)
            .map(|l| l.split(',').map(|v| v.parse().unwrap()).collect())
            .collect()
    }

    fn correlation(x: &[Vec<f64>]) -> (Vec<f64>, Vec<f64>, Vec<Vec<f64>>) {
        let p = x[0].len();
        let mean = spd::mean_vector(x, p);
        let cov = spd::covariance(x, &mean);
        let sd: Vec<f64> = (0..p).map(|j| cov[j][j].sqrt()).collect();
        let r = (0..p).map(|i| (0..p).map(|j| cov[i][j] / (sd[i] * sd[j])).collect()).collect();
        (mean, sd, r)
    }

    /// Compare loading matrices up to the sign of each column.
    fn assert_loadings(got: &[Vec<f64>], want: &[[f64; 2]], tol: f64) {
        for k in 0..2 {
            let sign = if got.iter().zip(want).map(|(g, w)| g[k] * w[k]).sum::<f64>() < 0.0 { -1.0 } else { 1.0 };
            for (g, w) in got.iter().zip(want) {
                assert!((sign * g[k] - w[k]).abs() < tol, "factor {k}: {} vs {}", g[k], w[k]);
            }
        }
    }

    /// statsmodels `Factor(method="ml")` and a direct L-BFGS-B minimisation.
    const ML_LOADINGS: [[f64; 2]; 6] = [
        [0.7597386323751859, 0.4102454109258341],
        [0.7144613105857108, 0.16290499371188039],
        [0.7036978807979501, 0.3407330719199983],
        [0.5860622317889148, -0.5667244374528271],
        [0.5875791327007318, -0.4257413440963121],
        [0.5356288396013691, -0.4982813757461119],
    ];
    const ML_UNIQUENESS: [f64; 6] = [
        0.254495890079836,
        0.46300700205880924,
        0.38871028167165816,
        0.33535449935418066,
        0.47349504125555414,
        0.4648173898762412,
    ];

    #[test]
    fn maximum_likelihood_matches_statsmodels_and_tests_fit() {
        let (_, _, r) = correlation(&abilities());
        let mut o = FactorOptions::new(2);
        o.rotation = Rotation::None;
        let fit = factor_analysis(&r, 300, &o).unwrap();
        assert!(fit.converged);
        for j in 0..6 {
            assert!((fit.uniquenesses[j] - ML_UNIQUENESS[j]).abs() < 1e-6, "{j}");
            assert!((fit.communalities[j] + fit.uniquenesses[j] - 1.0).abs() < 1e-6);
        }
        assert_loadings(&fit.unrotated, &ML_LOADINGS, 1e-6);
        // Bartlett-corrected chi-square: F = 0.0064029728395305, χ² = 1.8878098255,
        // df = 4, p = 0.75638398.
        let t = fit.fit_test.unwrap();
        assert!((t.discrepancy - 0.006_402_972_839_530_5).abs() < 1e-10);
        assert!((t.chi_square - 1.887_809_825_521_578).abs() < 1e-7);
        assert!((t.df - 4.0).abs() < 1e-12);
        assert!((t.p_value.unwrap() - 0.756_383_980_442_197_7).abs() < 1e-7);
        assert!(fit.heywood.iter().all(|h| !h));
        // Eigenvalues of R (numpy.linalg.eigvalsh).
        let ev = [2.97335853895114, 1.4191026426780773, 0.46779459433887205];
        for (a, b) in fit.eigenvalues.iter().zip(ev) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn principal_axis_matches_statsmodels() {
        let (_, _, r) = correlation(&abilities());
        let o = FactorOptions {
            extraction: Extraction::PrincipalAxis,
            rotation: Rotation::None,
            tolerance: 1e-13,
            ..FactorOptions::new(2)
        };
        let fit = factor_analysis(&r, 300, &o).unwrap();
        assert!(fit.converged && fit.fit_test.is_none());
        let want = [
            [0.7059923356292821, 0.4948265793367237],
            [0.6886805956873199, 0.24825813017808993],
            [0.6584886831743132, 0.42393668813064805],
            [0.649742770261676, -0.4882979606681527],
            [0.6346988696073805, -0.3538591063857098],
            [0.589747626219852, -0.43681362164924553],
        ];
        assert_loadings(&fit.unrotated, &want, 1e-8);
        let h = [
            0.7432785215853719,
            0.5359130620757632,
            0.6133296614118233,
            0.6606005658999939,
            0.5280589222529797,
            0.5386084026902804,
        ];
        for j in 0..6 {
            assert!((fit.communalities[j] - h[j]).abs() < 1e-8);
        }
    }

    #[test]
    fn varimax_and_promax_match_references() {
        let (_, _, r) = correlation(&abilities());
        // Varimax without normalisation: statsmodels rotate_factors.
        let raw =
            factor_analysis(&r, 300, &FactorOptions { kaiser_normalize: false, ..FactorOptions::new(2) }).unwrap();
        let want_raw = [
            [0.8558833617929382, 0.11387606295900261],
            [0.6740307343619041, 0.28753359879442175],
            [0.7697086986073177, 0.13725251556089596],
            [0.14176467557015038, 0.8028376574975273],
            [0.2257549495328831, 0.6895938166918871],
            [0.14111439247063715, 0.7178226183854826],
        ];
        assert_loadings(&raw.pattern, &want_raw, 1e-6);
        // Kaiser-normalised varimax (Kaiser 1958, NumPy).
        let fit = factor_analysis(&r, 300, &FactorOptions::new(2)).unwrap();
        let want = [
            [0.855379919228101, 0.11759796125083127],
            [0.6727736257053977, 0.2904628200912002],
            [0.7691043869414776, 0.14059934468886656],
            [0.1382711003011786, 0.803446719053061],
            [0.22275317488611046, 0.6905692958251449],
            [0.13799062715382818, 0.7184296556002128],
        ];
        assert_loadings(&fit.pattern, &want, 1e-6);
        // Rotation preserves communalities; pattern = unrotated · T.
        let back = matmul(&fit.unrotated, &fit.rotation_matrix);
        for j in 0..6 {
            let h: f64 = fit.pattern[j].iter().map(|v| v * v).sum();
            assert!((h - fit.communalities[j]).abs() < 1e-12);
            for k in 0..2 {
                assert!((back[j][k] - fit.pattern[j][k]).abs() < 1e-12);
            }
        }
        // Promax, power 4 (Hendrickson & White 1964, NumPy).
        let pm =
            factor_analysis(&r, 300, &FactorOptions { rotation: Rotation::Promax, ..FactorOptions::new(2) }).unwrap();
        assert!(pm.oblique());
        let pattern = [
            [0.8971331370727889, -0.08251471553588445],
            [0.6528027653712175, 0.15094643633179272],
            [0.7973463003570748, -0.03618149436796158],
            [-0.0642369096534061, 0.841931961007278],
            [0.0575785888708191, 0.6980488983894821],
            [-0.0418621090817306, 0.7493071132157451],
        ];
        assert_loadings(&pm.pattern, &pattern, 1e-6);
        // Both factors oriented positive: the correlation is +0.4468.
        assert!((pm.factor_correlation[0][1] - 0.446_814_933_464_948).abs() < 1e-6);
        assert!((pm.factor_correlation[0][0] - 1.0).abs() < 1e-12);
        let structure = [
            [0.8602643299407435, 0.3183377674144938],
            [0.7202478872775785, 0.44262846050686727],
            [0.7811798683583916, 0.3200847397746072],
            [0.311950863486074, 0.8132299504944972],
            [0.3694772609599958, 0.7237758717448026],
            [0.29293949885457476, 0.7306024977316891],
        ];
        assert_loadings(&pm.structure, &structure, 1e-6);
    }

    #[test]
    fn factor_scores_match_references() {
        let x = abilities();
        let (mean, sd, r) = correlation(&x);
        let fit = factor_analysis(&r, 300, &FactorOptions::new(2)).unwrap();
        let z: Vec<Vec<f64>> = x[..3].iter().map(|row| (0..6).map(|j| (row[j] - mean[j]) / sd[j]).collect()).collect();
        let reg = matmul(&z, &fit.score_weights(ScoreMethod::Regression).unwrap());
        let want = [
            [0.7126895348172724, 1.7940812946863822],
            [0.6061308699576258, -0.47718360197561727],
            [0.5925686821916134, 0.5077216408096193],
        ];
        // The reference printed factor 1 negated (its loadings were negative).
        for i in 0..3 {
            assert!((reg[i][0] - want[i][0]).abs() < 1e-5 && (reg[i][1] - want[i][1]).abs() < 1e-5, "{:?}", reg[i]);
        }
        let bart = matmul(&z, &fit.score_weights(ScoreMethod::Bartlett).unwrap());
        let want = [
            [0.6910321282841614, 2.2182564355077172],
            [0.781714387604147, -0.6670708273639488],
            [0.6698645434307171, 0.5899037636348169],
        ];
        for i in 0..3 {
            assert!((bart[i][0] - want[i][0]).abs() < 1e-5 && (bart[i][1] - want[i][1]).abs() < 1e-5, "{:?}", bart[i]);
        }
        let pm =
            factor_analysis(&r, 300, &FactorOptions { rotation: Rotation::Promax, ..FactorOptions::new(2) }).unwrap();
        let reg = matmul(&z, &pm.score_weights(ScoreMethod::Regression).unwrap());
        let want = [
            [1.0905802455415836, 1.912326263109148],
            [0.4860543887876417, -0.31874141575220116],
            [0.6898991380752137, 0.6344686073620402],
        ];
        for i in 0..3 {
            assert!((reg[i][0] - want[i][0]).abs() < 1e-5 && (reg[i][1] - want[i][1]).abs() < 1e-5, "{:?}", reg[i]);
        }
    }

    #[test]
    fn covariance_input_rescales_ml_loadings_and_flags_problems() {
        let x = abilities();
        let (mean, sd, r) = correlation(&x);
        let cov = spd::covariance(&x, &mean);
        let mut o = FactorOptions::new(2);
        o.rotation = Rotation::None;
        let a = factor_analysis(&r, 300, &o).unwrap();
        let b = factor_analysis(&cov, 300, &o).unwrap();
        // Maximum likelihood is scale invariant: Λ_S = D Λ_R, ψ_S = D² ψ_R.
        for j in 0..6 {
            assert!((b.uniquenesses[j] / (sd[j] * sd[j]) - a.uniquenesses[j]).abs() < 1e-6);
            for k in 0..2 {
                assert!((b.unrotated[j][k] / sd[j] - a.unrotated[j][k]).abs() < 1e-5);
            }
        }
        assert!((b.fit_test.unwrap().chi_square - a.fit_test.unwrap().chi_square).abs() < 1e-6);
        assert!(factor_analysis(&r, 300, &FactorOptions::new(4)).is_err(), "df < 0");
        assert!(factor_analysis(&r, 300, &FactorOptions::new(0)).is_err());
        assert!(factor_analysis(&r, 5, &FactorOptions::new(2)).is_err(), "too few rows");
        let singular = vec![vec![1.0, 1.0, 0.0], vec![1.0, 1.0, 0.0], vec![0.0, 0.0, 1.0]];
        assert!(factor_analysis(&singular, 50, &FactorOptions::new(1)).is_err());
    }

    #[test]
    fn heywood_case_is_flagged() {
        // Three variables, one factor: λ = (0.99, 0.6, 0.5) implies r₁₂ = 0.594,
        // r₁₃ = 0.495, r₂₃ = 0.3. Raising r₁₂ and r₁₃ pushes the first
        // communality past 1, so its uniqueness stops on the bound.
        let r = vec![vec![1.0, 0.75, 0.7], vec![0.75, 1.0, 0.45], vec![0.7, 0.45, 1.0]];
        let mut o = FactorOptions::new(1);
        o.rotation = Rotation::None;
        let fit = factor_analysis(&r, 200, &o).unwrap();
        assert!(fit.heywood[0], "{:?}", fit.uniquenesses);
        assert!((fit.uniquenesses[0] - 0.005).abs() < 1e-9);
        assert!(fit.fit_test.unwrap().p_value.is_none(), "df = 0");
        let pa = factor_analysis(&r, 200, &FactorOptions { extraction: Extraction::PrincipalAxis, ..o }).unwrap();
        assert!(pa.heywood[0]);
        // Held at the variance: the reported communality is 1 and the
        // uniqueness 0 (the raw principal-axis loadings imply 1.029 and
        // −0.029), and the other variables are untouched.
        assert!((pa.communalities[0] - 1.0).abs() < 1e-12, "{:?}", pa.communalities);
        assert!(pa.uniquenesses[0].abs() < 1e-12, "{:?}", pa.uniquenesses);
        let h0: f64 = pa.unrotated[0].iter().map(|v| v * v).sum();
        assert!((h0 - 1.0).abs() < 1e-12);
        for j in 1..3 {
            assert!((pa.communalities[j] + pa.uniquenesses[j] - 1.0).abs() < 1e-12);
            assert!(pa.uniquenesses[j] > 0.0);
        }
        // Bartlett scores divide by the uniquenesses: a Heywood-specific error.
        let err = pa.score_weights(ScoreMethod::Bartlett).unwrap_err().to_string();
        assert!(err.contains("Heywood"), "{err}");
        // The maximum-likelihood Heywood case stops on a positive bound, so
        // Bartlett scores still exist.
        assert!(fit.score_weights(ScoreMethod::Bartlett).is_ok());
    }
}
