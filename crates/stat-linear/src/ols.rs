//! Ordinary least squares multiple regression.
//!
//! # Model
//! ```text
//! y = β₀ + β₁x₁ + … + β_p x_p + ε,     ε ~ N(0, σ² I)
//! ```
//! (the intercept β₀ is optional). The estimate `β̂ = argmin ‖y − Xβ‖²` and every
//! derived statistic follow the standard textbook definitions, e.g. Draper & Smith,
//! *Applied Regression Analysis* (3rd ed., 1998), chs. 4–5, and the NIST/SEMATECH
//! e-Handbook of Statistical Methods §4.4.
//!
//! # Numerical method
//! Accuracy is the priority, so the normal equations `XᵀX β = Xᵀy` are never
//! formed (that would square the condition number). Instead:
//!
//! 1. **Centering.** With an intercept, each predictor and the response are
//!    centered at their (compensated) means. The slopes are invariant to this
//!    shift, and removing the constant direction is the single largest
//!    improvement in conditioning for typical data (e.g. calendar years).
//! 2. **Power-of-two column scaling.** Each centered column is divided by the
//!    power of two nearest its Euclidean norm. Scaling by powers of two is exact
//!    in binary floating point, yet equilibrates the columns so the pivoted QR's
//!    rank test is scale-invariant.
//! 3. **Householder QR with column pivoting** of the scaled, centered design.
//! 4. **Iterative refinement.** The residual `r = y − Xβ̂` is recomputed with
//!    compensated dot products and the correction `δ = argmin ‖r − Xδ‖` is added
//!    to `β̂` (Björck, *Numerical Methods for Least Squares Problems*, 1996, §2.9).
//! 5. **Inference without cancellation.** Standard errors come from row norms of
//!    `R⁻¹`, and the intercept variance from `‖R⁻ᵀ x̄‖²`, both sums of squares.
//!
//! # Conventions
//! * **R² without an intercept** is the *uncentered* R² = Σŷ² / Σy², with the
//!   ANOVA sums of squares likewise uncentered. This matches the convention of the
//!   NIST StRD certified values (datasets NoInt1/NoInt2).
//! * **Information criteria** use the Gaussian maximum log-likelihood
//!   `ℓ = −(n/2)(ln 2π + ln(RSS/n) + 1)` and count σ² as a parameter, so with `k`
//!   coefficients `AIC = −2ℓ + 2(k+1)`, `BIC = −2ℓ + (k+1) ln n` and
//!   `AICc = AIC + 2(k+1)(k+2)/(n−k−2)` (Hurvich & Tsai 1989, Biometrika 76).

use stat_core::distributions::{ContinuousDistribution, FisherSnedecor, StudentsT};
use stat_core::qr::norm2;
use stat_core::summation::{dot, mean, sum_iter};
use stat_core::{Matrix, QrDecomposition, Result, StatError};

/// Name used for the intercept term in [`ParameterEstimate::name`].
pub const INTERCEPT_NAME: &str = "Intercept";

/// Options controlling [`fit_linear_model`].
#[derive(Debug, Clone, PartialEq)]
pub struct OlsOptions {
    /// Include an intercept term β₀. Default `true`.
    pub intercept: bool,
    /// Confidence level for parameter and prediction intervals, in (0, 1).
    /// Default `0.95`.
    pub confidence_level: f64,
    /// Names for the predictor columns. If empty, `x1, x2, …` are used.
    /// Otherwise the length must equal the number of columns of `X`.
    pub predictor_names: Vec<String>,
    /// Relative tolerance for detecting rank deficiency in the pivoted QR of the
    /// scaled design. `None` uses `max(n, p) · ε`.
    pub rank_tolerance: Option<f64>,
    /// Number of iterative-refinement steps applied to the coefficient vector.
    /// Default `2`; `0` disables refinement.
    pub refinement_steps: usize,
}

impl Default for OlsOptions {
    fn default() -> Self {
        Self {
            intercept: true,
            confidence_level: 0.95,
            predictor_names: Vec::new(),
            rank_tolerance: None,
            refinement_steps: 2,
        }
    }
}

impl OlsOptions {
    /// Default options without an intercept (regression through the origin).
    #[must_use]
    pub fn without_intercept() -> Self {
        Self { intercept: false, ..Self::default() }
    }
}

/// Estimate and inference for one model coefficient.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterEstimate {
    /// Term name ([`INTERCEPT_NAME`] or the predictor name).
    pub name: String,
    /// Point estimate β̂ⱼ.
    pub estimate: f64,
    /// Standard error `s · √[(XᵀX)⁻¹]ⱼⱼ`, where `s² = RSS / (n − k)`.
    pub std_error: f64,
    /// `t = β̂ⱼ / SE(β̂ⱼ)` for H₀: βⱼ = 0. `None` when the standard error is zero
    /// (a perfect fit), where the statistic is undefined.
    pub t_statistic: Option<f64>,
    /// Two-sided p-value `P(|T| ≥ |t|)`, `T ~ t(n − k)`. `None` when `t` is undefined.
    pub p_value: Option<f64>,
    /// Lower confidence limit `β̂ⱼ − t_{1−α/2, n−k} · SE`.
    pub ci_lower: f64,
    /// Upper confidence limit `β̂ⱼ + t_{1−α/2, n−k} · SE`.
    pub ci_upper: f64,
}

/// One row of an analysis-of-variance table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnovaRow {
    /// Degrees of freedom.
    pub df: usize,
    /// Sum of squares.
    pub sum_of_squares: f64,
    /// Mean square `SS / df`; `None` when `df = 0`.
    pub mean_square: Option<f64>,
}

impl AnovaRow {
    fn new(df: usize, sum_of_squares: f64) -> Self {
        let mean_square = (df > 0).then(|| sum_of_squares / df as f64);
        Self { df, sum_of_squares, mean_square }
    }
}

/// Regression analysis-of-variance table.
///
/// With an intercept, sums of squares are about the mean:
/// `SS_model = Σ(ŷᵢ − ȳ)²`, `SS_error = Σ(yᵢ − ŷᵢ)²`, `SS_total = Σ(yᵢ − ȳ)²`.
/// Without one they are uncentered (`Σŷᵢ²`, `Σeᵢ²`, `Σyᵢ²`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnovaTable {
    /// Model (regression) row; `df = p`, the number of non-intercept terms.
    pub model: AnovaRow,
    /// Error (residual) row; `df = n − k`.
    pub error: AnovaRow,
    /// Total row; `df = n − 1` (centered) or `n` (uncentered).
    pub total: AnovaRow,
    /// Overall `F = MS_model / MS_error`. `None` when it is undefined
    /// (no model terms, or `MS_error = 0`).
    pub f_statistic: Option<f64>,
    /// `P(F_{p, n−k} ≥ F)`. `None` when `F` is undefined.
    pub p_value: Option<f64>,
    /// `true` if sums of squares are corrected for the mean (model has an intercept).
    pub centered: bool,
}

/// Overall goodness-of-fit summary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitSummary {
    /// Number of observations `n`.
    pub n_observations: usize,
    /// Number of estimated coefficients `k` (including the intercept).
    pub n_parameters: usize,
    /// `R² = SS_model / (SS_model + SS_error)` (uncentered without an intercept).
    pub r_squared: f64,
    /// `1 − (1 − R²)(n − i)/(n − k)` with `i = 1` if there is an intercept, else `0`.
    pub adj_r_squared: f64,
    /// Residual standard deviation `s = √(SS_error / (n − k))` (root mean square error).
    pub root_mean_square_error: f64,
    /// Mean of the response `ȳ`.
    pub mean_response: f64,
    /// Maximized Gaussian log-likelihood. `None` for a perfect fit (RSS = 0),
    /// where it is unbounded.
    pub log_likelihood: Option<f64>,
    /// Akaike information criterion; `None` when the log-likelihood is.
    pub aic: Option<f64>,
    /// Small-sample corrected AIC; `None` when undefined (`n ≤ k + 2`) or RSS = 0.
    pub aicc: Option<f64>,
    /// Bayesian (Schwarz) information criterion; `None` when the log-likelihood is.
    pub bic: Option<f64>,
}

/// Per-observation regression diagnostics.
///
/// Entries are NaN where the quantity is undefined (leverage `hᵢᵢ = 1`, or `s = 0`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResidualDiagnostics {
    /// Leverage `hᵢᵢ`, the diagonal of the hat matrix `H = X(XᵀX)⁻¹Xᵀ`.
    pub leverage: Vec<f64>,
    /// Internally studentized residuals `rᵢ = eᵢ / (s √(1 − hᵢᵢ))`.
    pub standardized_residuals: Vec<f64>,
    /// Externally studentized (deleted) residuals
    /// `tᵢ = rᵢ √((n − k − 1) / (n − k − rᵢ²))`.
    pub studentized_residuals: Vec<f64>,
    /// Cook's distance `Dᵢ = rᵢ² hᵢᵢ / (k (1 − hᵢᵢ))`.
    pub cooks_distance: Vec<f64>,
}

/// Predicted mean response with confidence and prediction intervals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prediction {
    /// Predicted mean `ŷ₀ = x₀ᵀβ̂`.
    pub fitted: f64,
    /// Standard error of the mean prediction `s √(x₀ᵀ(XᵀX)⁻¹x₀)`.
    pub std_error_mean: f64,
    /// Confidence interval for the mean response.
    pub confidence_interval: (f64, f64),
    /// Prediction interval for a new observation, using `s √(1 + x₀ᵀ(XᵀX)⁻¹x₀)`.
    pub prediction_interval: (f64, f64),
}

/// A fitted ordinary-least-squares model. Construct with [`fit_linear_model`].
#[derive(Debug, Clone)]
pub struct LinearModelFit {
    parameters: Vec<ParameterEstimate>,
    anova: AnovaTable,
    summary: FitSummary,
    covariance: Matrix,
    fitted: Vec<f64>,
    residuals: Vec<f64>,
    // State retained for prediction and diagnostics.
    intercept: bool,
    confidence_level: f64,
    t_critical: f64,
    predictor_means: Vec<f64>,
    column_scales: Vec<f64>,
    qr: QrDecomposition,
}

/// Fit `y = Xβ (+ β₀) + ε` by ordinary least squares.
///
/// `x` is the `n × p` matrix of predictors (one column per predictor; do **not**
/// include a column of ones — set [`OlsOptions::intercept`] instead).
///
/// # Errors
/// * [`StatError::DimensionMismatch`] if `y.len() != x.nrows()` or the number of
///   predictor names does not match the number of columns.
/// * [`StatError::NonFiniteInput`] if `x` or `y` contains NaN or ±∞.
/// * [`StatError::InsufficientData`] unless `n > k` (at least one residual degree
///   of freedom is needed to estimate σ²).
/// * [`StatError::InvalidArgument`] for an invalid confidence level or a model with
///   no terms at all.
/// * [`StatError::RankDeficient`] if a predictor is constant (with an intercept) or
///   the predictors are linearly dependent to within the rank tolerance.
///
/// # Example
/// ```
/// use stat_core::Matrix;
/// use stat_linear::{fit_linear_model, OlsOptions};
///
/// let x = Matrix::from_columns(&[[1.0, 2.0, 3.0, 4.0, 5.0]])?;
/// let y = [2.1, 3.9, 6.2, 7.8, 10.1];
/// let fit = fit_linear_model(&x, &y, &OlsOptions::default())?;
/// let slope = &fit.parameters()[1];
/// assert!((slope.estimate - 1.99).abs() < 1e-12);
/// assert!(fit.summary().r_squared > 0.99);
/// # Ok::<(), stat_core::StatError>(())
/// ```
pub fn fit_linear_model(x: &Matrix, y: &[f64], options: &OlsOptions) -> Result<LinearModelFit> {
    let (n, p) = (x.nrows(), x.ncols());
    validate(x, y, options)?;
    let k = p + usize::from(options.intercept);
    if n <= k {
        return Err(StatError::InsufficientData { required: k + 1, found: n });
    }

    // 1. Center (with intercept).
    let (predictor_means, y_mean) = if options.intercept {
        let means = (0..p).map(|j| mean(x.col(j)).unwrap_or(0.0)).collect::<Vec<_>>();
        (means, mean(y).unwrap_or(0.0))
    } else {
        (vec![0.0; p], 0.0)
    };
    let response: Vec<f64> = y.iter().map(|&v| v - y_mean).collect();

    // 2. Power-of-two column scaling. `scaled` is X / D exactly; `centered` is
    //    fl(X − 1x̄ᵀ) / D, which carries the rounding error of the centering.
    let mut scaled = x.clone();
    let mut centered = x.clone();
    let mut column_scales = Vec::with_capacity(p);
    for j in 0..p {
        let m = predictor_means[j];
        centered.col_mut(j).iter_mut().for_each(|v| *v -= m);
        let norm = norm2(centered.col(j));
        if norm == 0.0 {
            return Err(StatError::RankDeficient { rank: k - 1, columns: k });
        }
        let scale = power_of_two_near(norm);
        centered.col_mut(j).iter_mut().for_each(|v| *v /= scale);
        scaled.col_mut(j).iter_mut().for_each(|v| *v /= scale);
        column_scales.push(scale);
    }
    let scaled_means: Vec<f64> = predictor_means.iter().zip(&column_scales).map(|(m, s)| m / s).collect();

    // 3. Pivoted QR of the centered design, and rank check.
    let qr = QrDecomposition::new(centered)?;
    let tol = options.rank_tolerance.unwrap_or_else(|| qr.default_rank_tolerance());
    let rank = qr.rank(tol);
    if rank < p {
        return Err(StatError::RankDeficient { rank: rank + usize::from(options.intercept), columns: k });
    }

    // 4. Solve the centered problem, then refine against the exact design
    //    [1, X/D] (scaled coordinates; intercept first when present).
    let slopes_scaled = if p > 0 { qr.solve_least_squares(&response)? } else { Vec::new() };
    let mut beta: Vec<f64> = if options.intercept {
        std::iter::once(y_mean - dot(&scaled_means, &slopes_scaled)).chain(slopes_scaled).collect()
    } else {
        slopes_scaled
    };
    let exact = ExactDesign { x: &scaled, means: options.intercept.then_some(scaled_means.as_slice()) };
    if options.refinement_steps > 0 {
        refine_augmented(&qr, &exact, y, &mut beta, options.refinement_steps)?;
    }
    let off = usize::from(options.intercept);
    let slopes: Vec<f64> = beta[off..].iter().zip(&column_scales).map(|(b, s)| b / s).collect();
    let refined_intercept = options.intercept.then(|| beta[0]);
    let residuals = exact.residuals(y, &beta);
    let fitted: Vec<f64> = y.iter().zip(&residuals).map(|(yi, ei)| yi - ei).collect();

    // 5. Sums of squares, each accumulated directly (no subtraction of totals).
    let ss_error = dot(&residuals, &residuals);
    let ss_model = sum_iter(response.iter().zip(&residuals).map(|(r, e)| {
        let v = r - e;
        v * v
    }));
    let ss_total = dot(&response, &response);
    let df_model = p;
    let df_error = n - k;
    let df_total = if options.intercept { n - 1 } else { n };
    let anova_model = AnovaRow::new(df_model, ss_model);
    let anova_error = AnovaRow::new(df_error, ss_error);
    let mse = ss_error / df_error as f64;
    let (f_statistic, f_p_value) = match anova_model.mean_square {
        Some(msm) if mse > 0.0 => {
            let f = msm / mse;
            let dist = FisherSnedecor::new(df_model as f64, df_error as f64)?;
            (Some(f), Some(dist.sf(f)?))
        }
        _ => (None, None),
    };
    let anova = AnovaTable {
        model: anova_model,
        error: anova_error,
        total: AnovaRow::new(df_total, ss_total),
        f_statistic,
        p_value: f_p_value,
        centered: options.intercept,
    };

    // 6. Covariance of the estimates: σ̂² (XᵀX)⁻¹, ordered [intercept?, x1..xp].
    let s = mse.sqrt();
    let inv_scaled = if p > 0 { qr.inverse_gram()? } else { Matrix::zeros(0, 0) };
    let mut covariance = Matrix::zeros(k, k);
    for i in 0..p {
        for j in 0..p {
            covariance[(i + off, j + off)] = mse * inv_scaled[(i, j)] / (column_scales[i] * column_scales[j]);
        }
    }
    let intercept_estimate = if let Some(b0) = refined_intercept {
        // Var(β̂₀) = σ²(1/n + x̄ᵀ(XcᵀXc)⁻¹x̄);  Cov(β̂₀, β̂) = −(XcᵀXc)⁻¹x̄ σ².
        let u = &scaled_means;
        let quad = if p > 0 {
            let w = qr.solve_r_transpose(u)?;
            dot(&w, &w)
        } else {
            0.0
        };
        covariance[(0, 0)] = mse * (1.0 / n as f64 + quad);
        for j in 0..p {
            let cov = -mse * dot(&inv_scaled.row(j), u) / column_scales[j];
            covariance[(0, j + 1)] = cov;
            covariance[(j + 1, 0)] = cov;
        }
        Some(b0)
    } else {
        None
    };

    // 7. Parameter table.
    let t_dist = StudentsT::new(df_error as f64)?;
    let alpha = 1.0 - options.confidence_level;
    let t_critical = t_dist.quantile(1.0 - alpha / 2.0)?;
    let names = term_names(options, p);
    let estimates: Vec<f64> = intercept_estimate.into_iter().chain(slopes.iter().copied()).collect();
    let mut parameters = Vec::with_capacity(k);
    for (j, (name, &estimate)) in names.into_iter().zip(&estimates).enumerate() {
        let std_error = covariance[(j, j)].sqrt();
        let t_statistic = (std_error > 0.0).then(|| estimate / std_error);
        let p_value = t_statistic.map(|t| t_dist.two_sided_p_value(t)).transpose()?;
        parameters.push(ParameterEstimate {
            name,
            estimate,
            std_error,
            t_statistic,
            p_value,
            ci_lower: estimate - t_critical * std_error,
            ci_upper: estimate + t_critical * std_error,
        });
    }

    // 8. Summary statistics.
    let explained_plus_error = ss_model + ss_error;
    let (r_squared, one_minus_r2) = if explained_plus_error > 0.0 {
        (ss_model / explained_plus_error, ss_error / explained_plus_error)
    } else {
        (f64::NAN, f64::NAN)
    };
    let df_adj = (n - usize::from(options.intercept)) as f64;
    let adj_r_squared = 1.0 - one_minus_r2 * df_adj / df_error as f64;
    let nf = n as f64;
    let log_likelihood =
        (ss_error > 0.0).then(|| -0.5 * nf * ((2.0 * std::f64::consts::PI).ln() + (ss_error / nf).ln() + 1.0));
    let n_par = (k + 1) as f64; // coefficients + σ²
    let aic = log_likelihood.map(|ll| -2.0 * ll + 2.0 * n_par);
    let aicc = aic.and_then(|a| (nf - n_par - 1.0 > 0.0).then(|| a + 2.0 * n_par * (n_par + 1.0) / (nf - n_par - 1.0)));
    let bic = log_likelihood.map(|ll| -2.0 * ll + n_par * nf.ln());
    let summary = FitSummary {
        n_observations: n,
        n_parameters: k,
        r_squared,
        adj_r_squared,
        root_mean_square_error: s,
        mean_response: mean(y).unwrap_or(f64::NAN),
        log_likelihood,
        aic,
        aicc,
        bic,
    };

    Ok(LinearModelFit {
        parameters,
        anova,
        summary,
        covariance,
        fitted,
        residuals,
        intercept: options.intercept,
        confidence_level: options.confidence_level,
        t_critical,
        predictor_means,
        column_scales,
        qr,
    })
}

impl LinearModelFit {
    /// Parameter estimates, intercept first (if present), then predictors in column order.
    #[must_use]
    pub fn parameters(&self) -> &[ParameterEstimate] {
        &self.parameters
    }

    /// Coefficient vector β̂ in the same order as [`parameters`](Self::parameters).
    #[must_use]
    pub fn coefficients(&self) -> Vec<f64> {
        self.parameters.iter().map(|p| p.estimate).collect()
    }

    /// Analysis-of-variance table.
    #[must_use]
    pub fn anova(&self) -> &AnovaTable {
        &self.anova
    }

    /// Goodness-of-fit summary.
    #[must_use]
    pub fn summary(&self) -> &FitSummary {
        &self.summary
    }

    /// Estimated covariance matrix `σ̂²(XᵀX)⁻¹` of the coefficients, in the order of
    /// [`parameters`](Self::parameters).
    #[must_use]
    pub fn covariance(&self) -> &Matrix {
        &self.covariance
    }

    /// Fitted values `ŷ`.
    #[must_use]
    pub fn fitted_values(&self) -> &[f64] {
        &self.fitted
    }

    /// Raw residuals `e = y − ŷ`.
    #[must_use]
    pub fn residuals(&self) -> &[f64] {
        &self.residuals
    }

    /// Confidence level used for the intervals.
    #[must_use]
    pub fn confidence_level(&self) -> f64 {
        self.confidence_level
    }

    /// Leverage, studentized residuals and Cook's distance for every observation.
    ///
    /// Leverage is computed from the orthonormal factor `Q₁` of the centered design:
    /// `hᵢᵢ = ‖Q₁ᵀeᵢ‖² (+ 1/n with an intercept)`. A leverage within `n·ε`
    /// of 1 is reported as exactly 1, with NaN standardized and studentized
    /// residuals and Cook's distance (the row alone fixes a coefficient).
    #[must_use]
    pub fn residual_diagnostics(&self) -> ResidualDiagnostics {
        let n = self.summary.n_observations;
        let k = self.summary.n_parameters as f64;
        let df = self.anova.error.df as f64;
        let s = self.summary.root_mean_square_error;
        let base = if self.intercept { 1.0 / n as f64 } else { 0.0 };
        let q = self.qr.thin_q();
        let leverage: Vec<f64> = (0..n)
            .map(|i| {
                let row = q.row(i);
                base + dot(&row, &row)
            })
            .collect();
        let mut standardized = Vec::with_capacity(n);
        let mut studentized = Vec::with_capacity(n);
        let mut cooks = Vec::with_capacity(n);
        // A leverage within n·ε of 1 is 1 up to rounding (a row that alone
        // determines a coefficient, e.g. the only row of a factor level):
        // its residual is 0 and the scaled residuals are undefined (NaN),
        // whichever side of 1 the rounding fell.
        let near_one = n as f64 * f64::EPSILON;
        let leverage: Vec<f64> = leverage.into_iter().map(|h| if 1.0 - h <= near_one { 1.0 } else { h }).collect();
        for (&e, &h) in self.residuals.iter().zip(&leverage) {
            let one_minus_h = 1.0 - h;
            let r = if one_minus_h > 0.0 && s > 0.0 { e / (s * one_minus_h.sqrt()) } else { f64::NAN };
            let denom = df - r * r;
            let t = if df > 1.0 && denom > 0.0 { r * ((df - 1.0) / denom).sqrt() } else { f64::NAN };
            standardized.push(r);
            studentized.push(t);
            cooks.push(r * r * h / (k * one_minus_h));
        }
        ResidualDiagnostics {
            leverage,
            standardized_residuals: standardized,
            studentized_residuals: studentized,
            cooks_distance: cooks,
        }
    }

    /// Predict the mean response at new predictor values (one row per point,
    /// same columns as the fitted `X`), with confidence and prediction intervals
    /// at the fit's confidence level.
    ///
    /// `Var(ŷ₀) = s²(1/n + (x₀ − x̄)ᵀ(XcᵀXc)⁻¹(x₀ − x̄))` with an intercept, or
    /// `s² x₀ᵀ(XᵀX)⁻¹x₀` without; the quadratic form is evaluated as `‖R⁻ᵀu‖²`.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if `x_new` has the wrong number of columns;
    /// [`StatError::NonFiniteInput`] if it contains NaN or ±∞.
    pub fn predict(&self, x_new: &Matrix) -> Result<Vec<Prediction>> {
        let p = self.column_scales.len();
        if x_new.ncols() != p {
            return Err(StatError::DimensionMismatch {
                context: "prediction columns vs. fitted predictors",
                expected: p,
                found: x_new.ncols(),
            });
        }
        if !x_new.is_finite() {
            return Err(StatError::NonFiniteInput { context: "prediction matrix" });
        }
        let s = self.summary.root_mean_square_error;
        let n = self.summary.n_observations as f64;
        let coef = self.coefficients();
        let (b0, slopes) = if self.intercept { (coef[0], &coef[1..]) } else { (0.0, &coef[..]) };
        let mut out = Vec::with_capacity(x_new.nrows());
        for i in 0..x_new.nrows() {
            let row = x_new.row(i);
            let fitted = b0 + dot(&row, slopes);
            let u: Vec<f64> =
                row.iter().zip(&self.predictor_means).zip(&self.column_scales).map(|((x, m), c)| (x - m) / c).collect();
            let quad = if p > 0 {
                let w = self.qr.solve_r_transpose(&u)?;
                dot(&w, &w)
            } else {
                0.0
            };
            let base = if self.intercept { 1.0 / n } else { 0.0 };
            let se_mean = s * (base + quad).sqrt();
            let se_pred = s * (1.0 + base + quad).sqrt();
            let (hm, hp) = (self.t_critical * se_mean, self.t_critical * se_pred);
            out.push(Prediction {
                fitted,
                std_error_mean: se_mean,
                confidence_interval: (fitted - hm, fitted + hm),
                prediction_interval: (fitted - hp, fitted + hp),
            });
        }
        Ok(out)
    }
}

fn validate(x: &Matrix, y: &[f64], options: &OlsOptions) -> Result<()> {
    if y.len() != x.nrows() {
        return Err(StatError::DimensionMismatch {
            context: "response length vs. design rows",
            expected: x.nrows(),
            found: y.len(),
        });
    }
    if !x.is_finite() {
        return Err(StatError::NonFiniteInput { context: "predictor matrix" });
    }
    if !y.iter().all(|v| v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "response vector" });
    }
    if !(options.confidence_level > 0.0 && options.confidence_level < 1.0) {
        return Err(StatError::InvalidArgument {
            message: format!("confidence level must lie in (0, 1) (got {})", options.confidence_level),
        });
    }
    if !options.predictor_names.is_empty() && options.predictor_names.len() != x.ncols() {
        return Err(StatError::DimensionMismatch {
            context: "predictor names vs. design columns",
            expected: x.ncols(),
            found: options.predictor_names.len(),
        });
    }
    if x.ncols() == 0 && !options.intercept {
        return Err(StatError::InvalidArgument {
            message: "model has no terms: supply predictors or enable the intercept".into(),
        });
    }
    Ok(())
}

fn term_names(options: &OlsOptions, p: usize) -> Vec<String> {
    let predictors = if options.predictor_names.is_empty() {
        (1..=p).map(|j| format!("x{j}")).collect()
    } else {
        options.predictor_names.clone()
    };
    if options.intercept { std::iter::once(INTERCEPT_NAME.to_owned()).chain(predictors).collect() } else { predictors }
}

/// The exact (uncentered, power-of-two scaled) design `A = [1, X/D]`, or `X/D`
/// alone without an intercept. Coefficient vectors are ordered `[β₀?, β₁…β_p]`.
struct ExactDesign<'a> {
    x: &'a Matrix,
    /// Column means of `X/D` when the model has an intercept.
    means: Option<&'a [f64]>,
}

impl ExactDesign<'_> {
    fn has_intercept(&self) -> bool {
        self.means.is_some()
    }

    /// `y − r − Aβ` per row, with one compensated dot product per row
    /// (pass `r = None` for the plain residual `y − Aβ`).
    fn system_residual(&self, y: &[f64], r: Option<&[f64]>, beta: &[f64]) -> Vec<f64> {
        let p = self.x.ncols();
        let off = usize::from(self.has_intercept());
        // terms = [y, r, 1?, x_i1 … x_ip],  coefs = [1, −1, −β₀?, −β₁ … −β_p]
        let mut coefs = vec![1.0, -1.0];
        coefs.extend(beta.iter().map(|b| -b));
        let mut terms = vec![0.0; coefs.len()];
        if off == 1 {
            terms[2] = 1.0;
        }
        (0..self.x.nrows())
            .map(|i| {
                terms[0] = y[i];
                terms[1] = r.map_or(0.0, |r| r[i]);
                for j in 0..p {
                    terms[2 + off + j] = self.x[(i, j)];
                }
                dot(&terms, &coefs)
            })
            .collect()
    }

    fn residuals(&self, y: &[f64], beta: &[f64]) -> Vec<f64> {
        self.system_residual(y, None, beta)
    }

    /// `Aᵀ v`, one compensated dot product per column.
    fn transpose_times(&self, v: &[f64]) -> Vec<f64> {
        let intercept = self.has_intercept().then(|| sum_iter(v.iter().copied()));
        intercept.into_iter().chain((0..self.x.ncols()).map(|j| dot(self.x.col(j), v))).collect()
    }
}

/// Iterative refinement of the least-squares solution through the augmented system
///
/// ```text
/// ⎡ I   A ⎤ ⎡ r ⎤   ⎡ y ⎤
/// ⎣ Aᵀ  0 ⎦ ⎣ β ⎦ = ⎣ 0 ⎦
/// ```
///
/// (Björck 1967, "Iterative refinement of linear least squares solutions I",
/// BIT 7; Björck 1996 §2.9.3). Each step forms the residuals of this system,
/// `f = y − r − Aβ` and `g = −Aᵀr`, with compensated dot products on the **exact**
/// design `A`, and solves for corrections using the QR factors of the centered
/// design as an approximate factorization. Because the system residuals are
/// evaluated on the exact data, the fixed point is the least-squares solution of
/// the original problem: neither the rounding committed while centering nor the
/// `κ²‖r‖` error term of a plain QR solve survives, which matters for
/// ill-conditioned problems with large residuals.
///
/// With an intercept, `A = [1, Xc] T` where `Xc = X − 1x̄ᵀ` and
/// `T = [[1, x̄ᵀ], [0, I]]`. Writing `γ = Tδβ`, the correction system for
/// `B = [1, Xc]` has right-hand side `(f, T⁻ᵀg) = (f, [g₀; g_b − x̄g₀])`, and since
/// `1 ⟂ Xc`, `B` factors as `[1/√n, Q] · diag(√n, R)`. Without an intercept the
/// QR factors of `X` are used directly:
/// `h = R⁻ᵀPᵀg`, `d = Qᵀf`, `δβ = P R⁻¹(d₁ − h)`, `δr = Q[h; d₂]`.
fn refine_augmented(
    qr: &QrDecomposition,
    design: &ExactDesign<'_>,
    y: &[f64],
    beta: &mut [f64],
    steps: usize,
) -> Result<()> {
    let p = design.x.ncols();
    let n = y.len() as f64;
    let sqrt_n = n.sqrt();
    let mut r = design.residuals(y, beta);
    for _ in 0..steps {
        let mut f = design.system_residual(y, Some(&r), beta);
        let g: Vec<f64> = design.transpose_times(&r).into_iter().map(|v| -v).collect();

        let (delta_beta, delta_r) = if let Some(means) = design.means {
            let g0 = g[0];
            let g_b: Vec<f64> = g[1..].iter().zip(means).map(|(gj, m)| gj - m * g0).collect();
            let h0 = g0 / sqrt_n;
            let h_b = qr.solve_r_transpose(&g_b)?;
            // Component of f along 1/√n, then the rest through Qᵀ.
            let f_mean = sum_iter(f.iter().copied()) / n;
            let d0 = f_mean * sqrt_n;
            f.iter_mut().for_each(|v| *v -= f_mean);
            qr.apply_qt(&mut f)?;
            let rhs: Vec<f64> = f[..p].iter().zip(&h_b).map(|(d, h)| d - h).collect();
            let gamma_b = qr.solve_r(&rhs)?;
            let gamma0 = (d0 - h0) / sqrt_n;
            f[..p].copy_from_slice(&h_b);
            qr.apply_q(&mut f)?;
            let shift = h0 / sqrt_n;
            f.iter_mut().for_each(|v| *v += shift);
            let delta0 = gamma0 - dot(means, &gamma_b);
            (std::iter::once(delta0).chain(gamma_b).collect::<Vec<_>>(), f)
        } else {
            let h = qr.solve_r_transpose(&g)?;
            qr.apply_qt(&mut f)?;
            let rhs: Vec<f64> = f[..p].iter().zip(&h).map(|(d, h)| d - h).collect();
            let delta = qr.solve_r(&rhs)?;
            f[..p].copy_from_slice(&h);
            qr.apply_q(&mut f)?;
            (delta, f)
        };

        beta.iter_mut().zip(&delta_beta).for_each(|(b, d)| *b += d);
        r.iter_mut().zip(&delta_r).for_each(|(ri, d)| *ri += d);
    }
    Ok(())
}

/// The power of two closest (in log scale) to `v > 0`; dividing by it is exact.
fn power_of_two_near(v: f64) -> f64 {
    2.0_f64.powi(v.log2().round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f64, b: f64, tol: f64) {
        let scale = b.abs().max(1.0);
        assert!((a - b).abs() <= tol * scale, "{a} vs {b}");
    }

    /// Simple regression with a hand-computable answer:
    /// x = 1..5, y = 2, 4, 5, 4, 5 ⇒ β₁ = 0.6, β₀ = 2.2, RSS = 2.4.
    fn simple() -> (Matrix, Vec<f64>) {
        (Matrix::from_columns(&[[1.0, 2.0, 3.0, 4.0, 5.0]]).unwrap(), vec![2.0, 4.0, 5.0, 4.0, 5.0])
    }

    #[test]
    fn simple_regression_by_hand() {
        let (x, y) = simple();
        let fit = fit_linear_model(&x, &y, &OlsOptions::default()).unwrap();
        let [b0, b1] = [&fit.parameters()[0], &fit.parameters()[1]];
        assert_eq!(b0.name, INTERCEPT_NAME);
        assert_close(b0.estimate, 2.2, 1e-14);
        assert_close(b1.estimate, 0.6, 1e-14);
        // Sxx = 10, s² = 2.4/3 = 0.8 ⇒ SE(β₁) = √0.08, SE(β₀) = √(0.8(1/5 + 9/10)).
        assert_close(b1.std_error, 0.08_f64.sqrt(), 1e-14);
        assert_close(b0.std_error, (0.8 * 1.1_f64).sqrt(), 1e-14);
        let a = fit.anova();
        assert_close(a.error.sum_of_squares, 2.4, 1e-14);
        assert_close(a.model.sum_of_squares, 3.6, 1e-14);
        assert_close(a.total.sum_of_squares, 6.0, 1e-14);
        assert_eq!((a.model.df, a.error.df, a.total.df), (1, 3, 4));
        assert_close(a.f_statistic.unwrap(), 4.5, 1e-13);
        assert_close(fit.summary().r_squared, 0.6, 1e-14);
        assert_close(fit.summary().adj_r_squared, 1.0 - 0.4 * 4.0 / 3.0, 1e-14);
        // F = t² for a single predictor; their p-values agree.
        assert_close(a.p_value.unwrap(), b1.p_value.unwrap(), 1e-12);
    }

    #[test]
    fn confidence_interval_uses_t_quantile() {
        let (x, y) = simple();
        let fit = fit_linear_model(&x, &y, &OlsOptions::default()).unwrap();
        let b1 = &fit.parameters()[1];
        // t_{0.975, 3} = 3.182446305284263.
        let half = 3.182_446_305_284_263 * b1.std_error;
        assert_close(b1.ci_lower, 0.6 - half, 1e-12);
        assert_close(b1.ci_upper, 0.6 + half, 1e-12);
    }

    #[test]
    fn information_criteria_follow_definitions() {
        let (x, y) = simple();
        let s = *fit_linear_model(&x, &y, &OlsOptions::default()).unwrap().summary();
        let n = 5.0_f64;
        let ll = -0.5 * n * ((2.0 * std::f64::consts::PI).ln() + (2.4_f64 / n).ln() + 1.0);
        assert_close(s.log_likelihood.unwrap(), ll, 1e-14);
        assert_close(s.aic.unwrap(), -2.0 * ll + 6.0, 1e-14);
        assert_close(s.bic.unwrap(), -2.0 * ll + 3.0 * n.ln(), 1e-14);
        // n − K − 1 = 5 − 3 − 1 = 1.
        assert_close(s.aicc.unwrap(), -2.0 * ll + 6.0 + 24.0, 1e-14);
    }

    #[test]
    fn residuals_are_orthogonal_and_diagnostics_consistent() {
        let x =
            Matrix::from_rows(&[[1.0, 3.0], [2.0, 1.0], [3.0, 4.0], [4.0, 1.0], [5.0, 5.0], [6.0, 9.0], [7.0, 2.0]])
                .unwrap();
        let y = [3.1, 2.9, 6.8, 4.1, 9.2, 13.5, 7.7];
        let fit = fit_linear_model(&x, &y, &OlsOptions::default()).unwrap();
        let e = fit.residuals();
        assert!(sum_iter(e.iter().copied()).abs() < 1e-13);
        for j in 0..2 {
            assert!(dot(x.col(j), e).abs() < 1e-12);
        }
        let d = fit.residual_diagnostics();
        // Trace of the hat matrix equals the number of parameters.
        assert_close(sum_iter(d.leverage.iter().copied()), 3.0, 1e-13);
        assert!(d.leverage.iter().all(|&h| (1.0 / 7.0..=1.0).contains(&h)));
        // Cook's distance identity: Dᵢ = eᵢ² hᵢᵢ / (k s² (1 − hᵢᵢ)²).
        let s2 = fit.summary().root_mean_square_error.powi(2);
        for i in 0..7 {
            let h = d.leverage[i];
            let expected = e[i] * e[i] * h / (3.0 * s2 * (1.0 - h).powi(2));
            assert_close(d.cooks_distance[i], expected, 1e-12);
        }
    }

    #[test]
    fn leverage_matches_explicit_hat_matrix() {
        let x = Matrix::from_rows(&[[0.5, 1.0], [1.5, -2.0], [2.0, 0.0], [3.5, 1.0], [4.0, 3.0]]).unwrap();
        let y = [1.0, 2.0, 2.5, 4.0, 3.5];
        let fit = fit_linear_model(&x, &y, &OlsOptions::default()).unwrap();
        // Build H = X₁ (X₁ᵀX₁)⁻¹ X₁ᵀ explicitly with X₁ = [1 X].
        let mut cols = vec![vec![1.0; 5]];
        cols.extend((0..2).map(|j| x.col(j).to_vec()));
        let x1 = Matrix::from_columns(&cols).unwrap();
        let inv = QrDecomposition::new(x1.clone()).unwrap().inverse_gram().unwrap();
        let h = x1.matmul(&inv).unwrap().matmul(&x1.transpose()).unwrap();
        let d = fit.residual_diagnostics();
        for i in 0..5 {
            assert_close(d.leverage[i], h[(i, i)], 1e-13);
        }
    }

    #[test]
    fn covariance_matches_inverse_gram_of_full_design() {
        let x = Matrix::from_rows(&[
            [1960.0, 2.0],
            [1961.0, 3.5],
            [1962.0, 3.0],
            [1963.0, 5.0],
            [1964.0, 4.0],
            [1965.0, 7.0],
        ])
        .unwrap();
        let y = [10.0, 12.5, 12.0, 15.5, 14.0, 19.0];
        let fit = fit_linear_model(&x, &y, &OlsOptions::default()).unwrap();
        let mut cols = vec![vec![1.0; 6]];
        cols.extend((0..2).map(|j| x.col(j).to_vec()));
        let inv = QrDecomposition::new(Matrix::from_columns(&cols).unwrap()).unwrap().inverse_gram().unwrap();
        let s2 = fit.summary().root_mean_square_error.powi(2);
        for i in 0..3 {
            for j in 0..3 {
                assert_close(fit.covariance()[(i, j)], s2 * inv[(i, j)], 1e-8);
            }
        }
    }

    #[test]
    fn through_origin_uses_uncentered_sums() {
        let x = Matrix::from_columns(&[[4.0, 5.0, 6.0]]).unwrap();
        let y = [3.0, 4.0, 4.0];
        let fit = fit_linear_model(&x, &y, &OlsOptions::without_intercept()).unwrap();
        assert_eq!(fit.parameters().len(), 1);
        assert_close(fit.parameters()[0].estimate, 56.0 / 77.0, 1e-14);
        assert!(!fit.anova().centered);
        assert_eq!(fit.anova().total.df, 3);
        assert_close(fit.anova().total.sum_of_squares, 41.0, 1e-14);
    }

    #[test]
    fn prediction_at_design_points_matches_fitted_and_leverage() {
        let (x, y) = simple();
        let fit = fit_linear_model(&x, &y, &OlsOptions::default()).unwrap();
        let preds = fit.predict(&x).unwrap();
        let d = fit.residual_diagnostics();
        let s = fit.summary().root_mean_square_error;
        for (i, pr) in preds.iter().enumerate() {
            assert_close(pr.fitted, fit.fitted_values()[i], 1e-14);
            // se(ŷᵢ) = s √hᵢᵢ at observed points.
            assert_close(pr.std_error_mean, s * d.leverage[i].sqrt(), 1e-13);
            assert!(pr.prediction_interval.0 < pr.confidence_interval.0);
        }
        assert!(fit.predict(&Matrix::zeros(1, 2)).is_err());
    }

    #[test]
    fn perfect_fit_has_undefined_test_statistics() {
        let x = Matrix::from_columns(&[[1.0, 2.0, 3.0, 4.0]]).unwrap();
        let y = [3.0, 5.0, 7.0, 9.0];
        let fit = fit_linear_model(&x, &y, &OlsOptions::default()).unwrap();
        assert_close(fit.parameters()[1].estimate, 2.0, 1e-15);
        assert!(fit.anova().error.sum_of_squares < 1e-28);
        if fit.anova().error.sum_of_squares == 0.0 {
            assert!(fit.anova().f_statistic.is_none());
            assert!(fit.parameters()[1].t_statistic.is_none());
            assert!(fit.summary().aic.is_none());
        }
    }

    #[test]
    fn rejects_invalid_input() {
        let (x, y) = simple();
        let o = OlsOptions::default();
        assert!(matches!(fit_linear_model(&x, &y[..4], &o), Err(StatError::DimensionMismatch { .. })));
        let mut bad = y.clone();
        bad[2] = f64::NAN;
        assert!(matches!(fit_linear_model(&x, &bad, &o), Err(StatError::NonFiniteInput { .. })));
        let tiny = Matrix::from_columns(&[[1.0, 2.0]]).unwrap();
        assert!(matches!(fit_linear_model(&tiny, &[1.0, 2.0], &o), Err(StatError::InsufficientData { .. })));
        let o_bad = OlsOptions { confidence_level: 1.0, ..OlsOptions::default() };
        assert!(fit_linear_model(&x, &y, &o_bad).is_err());
        let o_names = OlsOptions { predictor_names: vec!["a".into(), "b".into()], ..OlsOptions::default() };
        assert!(fit_linear_model(&x, &y, &o_names).is_err());
    }

    #[test]
    fn rejects_collinear_and_constant_predictors() {
        let x = Matrix::from_columns(&[[1.0, 2.0, 3.0, 4.0, 5.0], [2.0, 4.0, 6.0, 8.0, 10.0]]).unwrap();
        let y = [1.0, 3.0, 2.0, 5.0, 4.0];
        assert!(matches!(fit_linear_model(&x, &y, &OlsOptions::default()), Err(StatError::RankDeficient { .. })));
        let constant = Matrix::from_columns(&[[3.0; 5]]).unwrap();
        assert!(matches!(
            fit_linear_model(&constant, &y, &OlsOptions::default()),
            Err(StatError::RankDeficient { .. })
        ));
    }

    #[test]
    fn intercept_only_model() {
        let x = Matrix::zeros(4, 0);
        let y = [1.0, 2.0, 3.0, 6.0];
        let fit = fit_linear_model(&x, &y, &OlsOptions::default()).unwrap();
        assert_eq!(fit.parameters().len(), 1);
        assert_close(fit.parameters()[0].estimate, 3.0, 1e-15);
        // SE of the mean: s/√n with s² = 14/3.
        assert_close(fit.parameters()[0].std_error, (14.0_f64 / 3.0 / 4.0).sqrt(), 1e-14);
        assert!(fit.anova().f_statistic.is_none());
    }

    #[test]
    fn custom_names_are_used() {
        let (x, y) = simple();
        let o = OlsOptions { predictor_names: vec!["dose".into()], ..OlsOptions::default() };
        let fit = fit_linear_model(&x, &y, &o).unwrap();
        assert_eq!(fit.parameters()[1].name, "dose");
    }

    /// The only row of a level (an indicator that is 1 once) has leverage 1
    /// exactly; rounding used to leave 1 − h at ±1e-16 and the scaled
    /// residuals at 0 or NaN depending on its sign.
    #[test]
    fn leverage_one_rows_get_undefined_scaled_residuals() {
        let only = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let x = [1.3, 2.1, 2.9, 4.2, 5.0, 3.3];
        let y = [2.0, 3.9, 6.1, 8.2, 9.8, 1.0];
        let fit =
            fit_linear_model(&Matrix::from_columns(&[&x[..], &only[..]]).unwrap(), &y, &OlsOptions::default()).unwrap();
        let d = fit.residual_diagnostics();
        assert_eq!(d.leverage[5], 1.0);
        assert!(d.standardized_residuals[5].is_nan() && d.studentized_residuals[5].is_nan());
        assert!(d.cooks_distance[5].is_nan());
        assert!(d.studentized_residuals[..5].iter().all(|v| v.is_finite()));
    }
}
