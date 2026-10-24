//! Multivariate analysis of variance: several continuous responses on the
//! effects of one linear model.
//!
//! # Model
//! The multivariate linear model `Y = X B + U` has `n` rows, `p` responses
//! (the columns of `Y`) and `k` design columns (the intercept plus the
//! columns of [`crate::ModelDesign`]); the rows of `U` are independent
//! `N_p(0, Σ)`. Each response's coefficients are its own least-squares fit,
//! `B̂ = (XᵀX)⁻¹XᵀY`, computed from one pivoted Householder QR of `X`.
//!
//! # Hypothesis and error matrices
//! The **error SSCP matrix** is `E = (Y − XB̂)ᵀ(Y − XB̂)` with `ν = n − k`
//! degrees of freedom; it is accumulated from the trailing `n − k`
//! components of `QᵀY`, so it is exactly symmetric and as accurate as the
//! QR. The **hypothesis SSCP matrix** of an effect (all its design columns)
//! is the increase in the error SSCP when the effect is dropped,
//! `H = E_reduced − E`, the multivariate extra-sum-of-squares principle; its
//! diagonal holds each response's partial (Type III, under the design's
//! sum-to-zero coding) sum of squares. A general linear hypothesis `L B = 0`
//! (a contrast, `h` rows of `L`) has `H = (LB̂)ᵀ [L(XᵀX)⁻¹Lᵀ]⁻¹ (LB̂)`
//! (Rencher & Christensen, *Methods of Multivariate Analysis*, 3rd ed.,
//! §10.4 and §6.1.4).
//!
//! # Test statistics
//! With `λ₁ ≥ … ≥ λ_s` the nonzero eigenvalues of `E⁻¹H`, `s = min(p, h)`,
//! `m = (|p − h| − 1)/2` and `N = (ν − p − 1)/2`:
//!
//! * **Wilks' Λ** `= |E| / |E + H| = Π 1/(1 + λᵢ)`, with Rao's (1951) F:
//!   `t = √((p²h² − 4)/(p² + h² − 5))` (1 when the denominator is not
//!   positive), `df₁ = ph`, `df₂ = t[ν − (p − h + 1)/2] − (ph − 2)/2`,
//!   `F = (1 − Λ^{1/t})/Λ^{1/t} · df₂/df₁`; exact when `min(p, h) ≤ 2`.
//! * **Pillai's trace** `V = Σ λᵢ/(1 + λᵢ)`:
//!   `F = (2N + s + 1)/(2m + s + 1) · V/(s − V)` on `s(2m + s + 1)` and
//!   `s(2N + s + 1)` df (Pillai 1955).
//! * **Hotelling–Lawley trace** `U = Σ λᵢ`: for `N > 0` McKeon's (1974)
//!   approximation `b = (p + 2N)(h + 2N)/[2(2N + 1)(N − 1)]`,
//!   `df₂ = 4 + (ph + 2)/(b − 1)`, `c = (df₂ − 2)/(2N)`,
//!   `F = (df₂/(ph)) · U/c` on `ph`, `df₂` df; otherwise Pillai & Samson's
//!   (1959) `F = 2(sN + 1)U / [s²(2m + s + 1)]` on `s(2m + s + 1)`,
//!   `2(sN + 1)` df.
//! * **Roy's largest root** `θ = λ₁`: with `r = max(p, h)`,
//!   `F = θ (ν − r + h)/r` on `r`, `ν − r + h` df, an upper bound on the
//!   true F, so its p-value is a lower bound.
//!
//! When `s = 1` the four statistics are equivalent and every F above is
//! replaced by the exact `F = λ₁ (ν − r + h)/r` (Hotelling's T² for
//! `h = 1`, the univariate F for `p = 1`). These are the forms of Rencher &
//! Christensen §6.1 and of the SAS/STAT documentation's "Multivariate
//! Tests" section.
//!
//! # Canonical analysis
//! The eigenvectors `aᵢ` of `E⁻¹H`, scaled so `aᵢᵀ(E/ν)aᵢ = 1`, are the
//! **canonical variates** of the effect: `aᵢᵀy` has unit error variance and
//! the largest ratio of hypothesis to error variation after the earlier
//! ones; `√(λᵢ/(1 + λᵢ))` is its canonical correlation.
//!
//! # Equal covariance matrices
//! [`box_m_test`] tests `Σ₁ = … = Σ_g` across groups (Box 1949):
//! `M = (n − g) ln|S_p| − Σ(nᵢ − 1) ln|Sᵢ|`,
//! `c = [Σ 1/(nᵢ − 1) − 1/(n − g)] (2p² + 3p − 1)/[6(p + 1)(g − 1)]`,
//! `χ² = (1 − c)M` on `p(p + 1)(g − 1)/2` df.

use stat_core::distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor};
use stat_core::eigen::generalized_symmetric_eigen;
use stat_core::qr::QrDecomposition;
use stat_core::summation::{dot, sum_iter};
use stat_core::{Matrix, Result, StatError};
use std::ops::Range;

/// One of the four multivariate test statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MultivariateStatistic {
    /// Wilks' Λ (likelihood ratio).
    Wilks,
    /// Pillai's trace.
    Pillai,
    /// Hotelling–Lawley trace.
    HotellingLawley,
    /// Roy's largest root.
    Roy,
}

impl MultivariateStatistic {
    /// The four statistics, in report order.
    pub const ALL: [Self; 4] = [Self::Wilks, Self::Pillai, Self::HotellingLawley, Self::Roy];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Wilks => "Wilks' Λ",
            Self::Pillai => "Pillai's trace",
            Self::HotellingLawley => "Hotelling–Lawley trace",
            Self::Roy => "Roy's largest root",
        }
    }

    /// Stable key for project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Wilks => "wilks",
            Self::Pillai => "pillai",
            Self::HotellingLawley => "hotelling-lawley",
            Self::Roy => "roy",
        }
    }

    /// The statistic with this key.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// A multivariate test statistic with its F approximation.
#[derive(Debug, Clone, PartialEq)]
pub struct MultivariateTest {
    /// Which statistic.
    pub statistic: MultivariateStatistic,
    /// Its value.
    pub value: f64,
    /// F statistic (`None` when it is undefined, e.g. Pillai's `V = s`).
    pub f: Option<f64>,
    /// Numerator degrees of freedom.
    pub df1: f64,
    /// Denominator degrees of freedom.
    pub df2: f64,
    /// Upper-tail p-value of `f`.
    pub p_value: Option<f64>,
    /// The F distribution is exact (otherwise an approximation; for Roy's
    /// root with `s > 1`, an upper bound).
    pub exact: bool,
}

/// A univariate F-test of one response within a hypothesis.
#[derive(Debug, Clone, PartialEq)]
pub struct UnivariateTest {
    /// Hypothesis sum of squares (diagonal of `H`).
    pub sum_of_squares: f64,
    /// `(SS/h) / (E_jj/ν)`.
    pub f: Option<f64>,
    /// Upper-tail p-value on `h`, `ν` df.
    pub p_value: Option<f64>,
}

/// The test of one hypothesis (a model effect or a contrast).
#[derive(Debug, Clone, PartialEq)]
pub struct HypothesisTest {
    /// Effect or contrast name.
    pub name: String,
    /// Hypothesis degrees of freedom `h` (rank of the hypothesis).
    pub df: usize,
    /// Hypothesis SSCP matrix `H` (`p × p`, rows).
    pub sscp: Vec<Vec<f64>>,
    /// Nonzero eigenvalues of `E⁻¹H`, largest first (`s = min(p, h)` of them).
    pub eigenvalues: Vec<f64>,
    /// Canonical variates (one per eigenvalue), each with `p` coefficients
    /// scaled to unit error variance: `aᵀ(E/ν)a = 1`.
    pub canonical: Vec<Vec<f64>>,
    /// Wilks, Pillai, Hotelling–Lawley and Roy, in that order.
    pub tests: Vec<MultivariateTest>,
    /// Univariate F-test of each response.
    pub univariate: Vec<UnivariateTest>,
}

impl HypothesisTest {
    /// The test by `statistic`.
    #[must_use]
    pub fn test(&self, statistic: MultivariateStatistic) -> &MultivariateTest {
        self.tests.iter().find(|t| t.statistic == statistic).unwrap_or(&self.tests[0])
    }

    /// Canonical correlation `√(λ/(1 + λ))` of each eigenvalue.
    #[must_use]
    pub fn canonical_correlations(&self) -> Vec<f64> {
        self.eigenvalues.iter().map(|l| (l / (1.0 + l)).sqrt()).collect()
    }
}

/// A fitted multivariate linear model with its effect tests.
#[derive(Debug, Clone)]
pub struct ManovaFit {
    n: usize,
    /// Coefficients: `coefficients[c][j]` for design column `c` (0 = the
    /// intercept) and response `j`.
    coefficients: Vec<Vec<f64>>,
    error_sscp: Vec<Vec<f64>>,
    df_error: usize,
    /// `(XᵀX)⁻¹` including the intercept.
    inverse_gram: Matrix,
    effects: Vec<HypothesisTest>,
}

impl ManovaFit {
    /// Number of rows.
    #[must_use]
    pub fn n(&self) -> usize {
        self.n
    }

    /// Number of responses.
    #[must_use]
    pub fn n_responses(&self) -> usize {
        self.error_sscp.len()
    }

    /// Error degrees of freedom `ν = n − k`.
    #[must_use]
    pub fn df_error(&self) -> usize {
        self.df_error
    }

    /// Error SSCP matrix `E`.
    #[must_use]
    pub fn error_sscp(&self) -> &[Vec<f64>] {
        &self.error_sscp
    }

    /// Partial correlations of the responses given the model:
    /// `E_ij / √(E_ii E_jj)`.
    #[must_use]
    pub fn residual_correlations(&self) -> Vec<Vec<f64>> {
        let e = &self.error_sscp;
        let p = e.len();
        (0..p).map(|i| (0..p).map(|j| e[i][j] / (e[i][i] * e[j][j]).sqrt()).collect()).collect()
    }

    /// `(XᵀX)⁻¹`, the intercept first: the standard error of a mean
    /// prediction of response `j` at design row `x₀` (intercept 1 first) is
    /// `√(x₀ᵀ(XᵀX)⁻¹x₀ · E_jj/ν)`.
    #[must_use]
    pub fn inverse_gram(&self) -> &Matrix {
        &self.inverse_gram
    }

    /// Coefficients by design column (0 = intercept), then response.
    #[must_use]
    pub fn coefficients(&self) -> &[Vec<f64>] {
        &self.coefficients
    }

    /// The test of each effect, in the order given to [`fit_manova`].
    #[must_use]
    pub fn effect_tests(&self) -> &[HypothesisTest] {
        &self.effects
    }

    /// Test the general linear hypothesis `L B = 0`, `L` having one row per
    /// constraint and one column per design column (the intercept first).
    ///
    /// # Errors
    /// A row of the wrong length, no rows, or an `L(XᵀX)⁻¹Lᵀ` that is not
    /// positive definite (dependent rows).
    pub fn test_hypothesis(&self, name: &str, l: &[Vec<f64>]) -> Result<HypothesisTest> {
        let k = self.coefficients.len();
        if l.is_empty() || l.iter().any(|r| r.len() != k) {
            return Err(invalid(format!("a hypothesis needs rows of {k} coefficients")));
        }
        let h = l.len();
        // LB̂ (h × p) and T = L G Lᵀ (h × h).
        let p = self.n_responses();
        let lb: Vec<Vec<f64>> = l
            .iter()
            .map(|row| (0..p).map(|j| sum_iter((0..k).map(|c| row[c] * self.coefficients[c][j]))).collect())
            .collect();
        let g = &self.inverse_gram;
        let t: Vec<Vec<f64>> = l
            .iter()
            .map(|a| {
                l.iter()
                    .map(|b| {
                        sum_iter((0..k).flat_map(|i| (0..k).map(move |j| (i, j))).map(|(i, j)| a[i] * g[(i, j)] * b[j]))
                    })
                    .collect()
            })
            .collect();
        let chol = stat_core::eigen::cholesky(&t)
            .ok_or_else(|| invalid(format!("{name}: the hypothesis rows are linearly dependent")))?;
        // W = L_T⁻¹ (LB̂), H = Wᵀ W.
        let w: Vec<Vec<f64>> = (0..p).map(|j| forward(&chol, &lb.iter().map(|r| r[j]).collect::<Vec<_>>())).collect();
        let sscp: Vec<Vec<f64>> = (0..p).map(|a| (0..p).map(|b| dot(&w[a], &w[b])).collect()).collect();
        hypothesis(name, h, sscp, &self.error_sscp, self.df_error)
    }
}

/// Fit the multivariate linear model and test each effect.
///
/// `x` is the design matrix **without** the intercept (an intercept column
/// is added first); `y` holds one vector per response, each of `x.nrows()`
/// values; `effects` names each effect and its columns in `x`.
///
/// # Errors
/// * [`StatError::InvalidArgument`] for no responses, responses of the wrong
///   length, non-finite values, or an effect range outside `x`.
/// * [`StatError::InsufficientData`] when `ν = n − k` is smaller than the
///   number of responses (the error SSCP matrix would be singular).
/// * [`StatError::RankDeficient`] for collinear design columns.
/// * A singular error SSCP matrix (a response that is a linear combination
///   of others, or fitted exactly).
pub fn fit_manova(x: &Matrix, y: &[Vec<f64>], effects: &[(String, Range<usize>)]) -> Result<ManovaFit> {
    let n = x.nrows();
    let p = y.len();
    if p == 0 {
        return Err(invalid("choose at least one response"));
    }
    if y.iter().any(|v| v.len() != n) {
        return Err(StatError::DimensionMismatch { context: "response length vs. design rows", expected: n, found: 0 });
    }
    if y.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "MANOVA responses" });
    }
    if effects.iter().any(|(_, r)| r.end > x.ncols() || r.is_empty()) {
        return Err(invalid("an effect's columns are outside the design"));
    }
    let full = with_intercept(x, &(0..x.ncols()).collect::<Vec<_>>())?;
    let k = full.ncols();
    if n < k + p {
        return Err(StatError::InsufficientData { required: k + p, found: n });
    }
    let qr = QrDecomposition::new(full)?;
    let rank = qr.rank(qr.default_rank_tolerance());
    if rank < k {
        return Err(StatError::RankDeficient { rank, columns: k });
    }
    let coefficients_by_response: Vec<Vec<f64>> = y.iter().map(|v| qr.solve_least_squares(v)).collect::<Result<_>>()?;
    let coefficients: Vec<Vec<f64>> = (0..k).map(|c| coefficients_by_response.iter().map(|b| b[c]).collect()).collect();
    let error_sscp = residual_sscp(&qr, y)?;
    let df_error = n - k;
    if (0..p).any(|j| error_sscp[j][j].is_nan() || error_sscp[j][j] <= 0.0)
        || stat_core::eigen::cholesky(&error_sscp).is_none()
    {
        return Err(invalid(
            "the error SSCP matrix is singular: a response is fitted exactly or is a linear combination of the others",
        ));
    }
    let inverse_gram = qr.inverse_gram()?;
    let mut tests = Vec::with_capacity(effects.len());
    for (name, cols) in effects {
        let keep: Vec<usize> = (0..x.ncols()).filter(|j| !cols.contains(j)).collect();
        let reduced = with_intercept(x, &keep)?;
        let rqr = QrDecomposition::new(reduced)?;
        let e_reduced = residual_sscp(&rqr, y)?;
        let sscp: Vec<Vec<f64>> =
            (0..p).map(|a| (0..p).map(|b| e_reduced[a][b] - error_sscp[a][b]).collect()).collect();
        tests.push(hypothesis(name, cols.len(), sscp, &error_sscp, df_error)?);
    }
    Ok(ManovaFit { n, coefficients, error_sscp, df_error, inverse_gram, effects: tests })
}

/// `[1, x_keep]` as a matrix.
fn with_intercept(x: &Matrix, keep: &[usize]) -> Result<Matrix> {
    let ones = vec![1.0; x.nrows()];
    let mut cols: Vec<&[f64]> = vec![&ones];
    cols.extend(keep.iter().map(|&j| x.col(j)));
    Matrix::from_columns(&cols)
}

/// Residual SSCP from the trailing components of `Qᵀy`.
fn residual_sscp(qr: &QrDecomposition, y: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    let k = qr.ncols();
    let tails: Vec<Vec<f64>> = y
        .iter()
        .map(|v| {
            let mut c = v.clone();
            qr.apply_qt(&mut c)?;
            Ok(c.split_off(k))
        })
        .collect::<Result<_>>()?;
    let p = y.len();
    Ok((0..p).map(|a| (0..p).map(|b| dot(&tails[a], &tails[b])).collect()).collect())
}

/// Eigen-analysis and the four statistics for `H` against `E`.
fn hypothesis(name: &str, h: usize, sscp: Vec<Vec<f64>>, e: &[Vec<f64>], nu: usize) -> Result<HypothesisTest> {
    let p = e.len();
    let (values, vectors) = generalized_symmetric_eigen(&sscp, e)?;
    let s = p.min(h);
    let scale = (nu as f64).sqrt();
    let eigenvalues: Vec<f64> = values.iter().take(s).map(|v| v.max(0.0)).collect();
    let canonical: Vec<Vec<f64>> = vectors.iter().take(s).map(|v| v.iter().map(|a| a * scale).collect()).collect();
    let tests = multivariate_tests(&eigenvalues, p, h, nu)?;
    let univariate = (0..p)
        .map(|j| {
            let ss = sscp[j][j].max(0.0);
            let mse = e[j][j] / nu as f64;
            let f = (mse > 0.0).then(|| ss / h as f64 / mse);
            let p_value = match f {
                Some(f) => Some(FisherSnedecor::new(h as f64, nu as f64)?.sf(f)?),
                None => None,
            };
            Ok(UnivariateTest { sum_of_squares: ss, f, p_value })
        })
        .collect::<Result<_>>()?;
    Ok(HypothesisTest { name: name.to_owned(), df: h, sscp, eigenvalues, canonical, tests, univariate })
}

/// Wilks' Λ, Pillai's trace, the Hotelling–Lawley trace and Roy's largest
/// root, with their F approximations, from the nonzero eigenvalues of
/// `E⁻¹H` for `p` responses, `h` hypothesis and `nu` error degrees of
/// freedom (formulas in the module documentation).
///
/// # Errors
/// `p`, `h` or `nu` zero, `nu < p`, or a negative eigenvalue.
pub fn multivariate_tests(eigenvalues: &[f64], p: usize, h: usize, nu: usize) -> Result<Vec<MultivariateTest>> {
    if p == 0 || h == 0 || nu < p {
        return Err(invalid(
            "multivariate tests need responses, hypothesis df, and error df of at least the responses",
        ));
    }
    if eigenvalues.iter().any(|l| l.is_nan() || *l < 0.0) {
        return Err(invalid("eigenvalues of E⁻¹H must be non-negative"));
    }
    let (pf, hf, nuf) = (p as f64, h as f64, nu as f64);
    let s = p.min(h);
    let sf = s as f64;
    let m = ((pf - hf).abs() - 1.0) / 2.0;
    let nn = (nuf - pf - 1.0) / 2.0;
    let wilks = eigenvalues.iter().map(|l| 1.0 / (1.0 + l)).product::<f64>();
    let pillai = eigenvalues.iter().map(|l| l / (1.0 + l)).sum::<f64>();
    let lawley = eigenvalues.iter().sum::<f64>();
    let roy = eigenvalues.first().copied().unwrap_or(0.0);
    let f_test = |statistic, value: f64, f: Option<f64>, df1: f64, df2: f64, exact: bool| -> Result<MultivariateTest> {
        let f = f.filter(|v| v.is_finite() && df1 > 0.0 && df2 > 0.0);
        let p_value = match f {
            Some(v) => Some(FisherSnedecor::new(df1, df2)?.sf(v.max(0.0))?),
            None => None,
        };
        Ok(MultivariateTest { statistic, value, f, df1, df2, p_value, exact })
    };
    let r = p.max(h) as f64;
    if s == 1 {
        // All four are equivalent and exact.
        let (df1, df2) = (r, nuf - r + hf);
        let f = Some(roy * df2 / df1);
        return Ok(vec![
            f_test(MultivariateStatistic::Wilks, wilks, f, df1, df2, true)?,
            f_test(MultivariateStatistic::Pillai, pillai, f, df1, df2, true)?,
            f_test(MultivariateStatistic::HotellingLawley, lawley, f, df1, df2, true)?,
            f_test(MultivariateStatistic::Roy, roy, f, df1, df2, true)?,
        ]);
    }
    // Wilks (Rao 1951).
    let t = if pf * pf + hf * hf - 5.0 > 0.0 {
        ((pf * pf * hf * hf - 4.0) / (pf * pf + hf * hf - 5.0)).sqrt()
    } else {
        1.0
    };
    let w_df1 = pf * hf;
    let w_df2 = t * (nuf - (pf - hf + 1.0) / 2.0) - (pf * hf - 2.0) / 2.0;
    let root = wilks.powf(1.0 / t);
    let w_f = (root > 0.0).then(|| (1.0 - root) / root * w_df2 / w_df1);
    // Pillai.
    let v_df1 = sf * (2.0 * m + sf + 1.0);
    let v_df2 = sf * (2.0 * nn + sf + 1.0);
    let v_f = (pillai < sf).then(|| v_df2 / v_df1 * pillai / (sf - pillai));
    // Hotelling–Lawley.
    let (u_f, u_df1, u_df2) = if nn > 0.0 {
        let b = (pf + 2.0 * nn) * (hf + 2.0 * nn) / (2.0 * (2.0 * nn + 1.0) * (nn - 1.0));
        let df2 = 4.0 + (pf * hf + 2.0) / (b - 1.0);
        let c = (df2 - 2.0) / (2.0 * nn);
        (Some(df2 / (pf * hf) * lawley / c), pf * hf, df2)
    } else {
        let df1 = sf * (2.0 * m + sf + 1.0);
        let df2 = 2.0 * (sf * nn + 1.0);
        (Some(df2 * lawley / (sf * sf * (2.0 * m + sf + 1.0))), df1, df2)
    };
    // Roy (upper bound).
    let (r_df1, r_df2) = (r, nuf - r + hf);
    Ok(vec![
        f_test(MultivariateStatistic::Wilks, wilks, w_f, w_df1, w_df2, p.min(h) <= 2)?,
        f_test(MultivariateStatistic::Pillai, pillai, v_f, v_df1, v_df2, false)?,
        f_test(MultivariateStatistic::HotellingLawley, lawley, u_f, u_df1, u_df2, false)?,
        f_test(MultivariateStatistic::Roy, roy, Some(roy * r_df2 / r_df1), r_df1, r_df2, false)?,
    ])
}

/// Box's M test of equal covariance matrices across groups.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxMTest {
    /// `M = (n − g) ln|S_p| − Σ(nᵢ − 1) ln|Sᵢ|`.
    pub m: f64,
    /// `(1 − c) M`.
    pub chi_square: f64,
    /// `p(p + 1)(g − 1)/2`.
    pub df: f64,
    /// Upper-tail chi-square p-value.
    pub p_value: f64,
}

/// Box's (1949) M test that `g` groups share one covariance matrix. Each
/// group is a list of rows of `p` values.
///
/// # Errors
/// Fewer than two groups, rows of unequal length, or a group whose
/// covariance matrix is singular (it needs more than `p` rows).
pub fn box_m_test(groups: &[Vec<Vec<f64>>]) -> Result<BoxMTest> {
    let g = groups.len();
    if g < 2 {
        return Err(invalid("Box's M test needs at least two groups"));
    }
    let p = groups[0].first().map_or(0, Vec::len);
    if p == 0 || groups.iter().flatten().any(|r| r.len() != p) {
        return Err(invalid("Box's M test: every row needs the same number of values"));
    }
    let mut pooled = vec![vec![0.0; p]; p];
    let mut sum_log = 0.0;
    let mut sum_inv = 0.0;
    let mut n = 0usize;
    for rows in groups {
        let ni = rows.len();
        if ni <= p {
            return Err(invalid(format!("Box's M test: each group needs more than {p} rows")));
        }
        let mean: Vec<f64> = (0..p).map(|j| sum_iter(rows.iter().map(|r| r[j])) / ni as f64).collect();
        let s: Vec<Vec<f64>> = (0..p)
            .map(|a| {
                (0..p)
                    .map(|b| sum_iter(rows.iter().map(|r| (r[a] - mean[a]) * (r[b] - mean[b]))) / (ni - 1) as f64)
                    .collect()
            })
            .collect();
        let ld = log_det(&s).ok_or_else(|| invalid("Box's M test: a group's covariance matrix is singular"))?;
        sum_log += (ni - 1) as f64 * ld;
        sum_inv += 1.0 / (ni - 1) as f64;
        for a in 0..p {
            for b in 0..p {
                pooled[a][b] += (ni - 1) as f64 * s[a][b];
            }
        }
        n += ni;
    }
    let dof = (n - g) as f64;
    pooled.iter_mut().flatten().for_each(|v| *v /= dof);
    let ld = log_det(&pooled).ok_or_else(|| invalid("Box's M test: the pooled covariance matrix is singular"))?;
    let m = dof * ld - sum_log;
    let pf = p as f64;
    let gf = g as f64;
    let c = (sum_inv - 1.0 / dof) * (2.0 * pf * pf + 3.0 * pf - 1.0) / (6.0 * (pf + 1.0) * (gf - 1.0));
    let chi_square = (1.0 - c) * m;
    let df = pf * (pf + 1.0) * (gf - 1.0) / 2.0;
    let p_value = ChiSquared::new(df)?.sf(chi_square.max(0.0))?;
    Ok(BoxMTest { m, chi_square, df, p_value })
}

/// `ln |A|` of a symmetric positive-definite matrix by Cholesky.
fn log_det(a: &[Vec<f64>]) -> Option<f64> {
    let l = stat_core::eigen::cholesky(a)?;
    Some(2.0 * l.iter().enumerate().map(|(i, r)| r[i].ln()).sum::<f64>())
}

/// Solve `L z = d` for lower-triangular `L`.
fn forward(l: &[Vec<f64>], d: &[f64]) -> Vec<f64> {
    let mut z = vec![0.0; d.len()];
    for i in 0..d.len() {
        z[i] = (d[i] - (0..i).map(|k| l[i][k] * z[k]).sum::<f64>()) / l[i][i];
    }
    z
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}
