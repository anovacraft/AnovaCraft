//! Principal component analysis via the singular value decomposition.
//!
//! For an `n × p` data matrix the columns are centred (and, for a
//! correlation-based analysis, divided by their standard deviations) to give
//! `Z`. With the thin SVD `Z = U Σ Vᵀ` ([`stat_core::svd()`]):
//!
//! * the **eigenvalues** (component variances) are `λₖ = σₖ² / (n − 1)`, the
//!   eigenvalues of the covariance or correlation matrix `ZᵀZ / (n − 1)`;
//! * the **eigenvectors** (loadings, unit length) are the columns of `V`;
//! * the **scores** are `Z V = U Σ`, uncorrelated with variances `λₖ`;
//! * the **proportion of variance** is `λₖ / Σλ`.
//!
//! Working from the SVD of `Z` rather than forming the covariance matrix avoids
//! squaring its condition number (Jolliffe, *Principal Component Analysis*, 2nd
//! ed., §3.5). Each eigenvector's sign is chosen so that its largest-magnitude
//! element is positive. For a correlation analysis, `vⱼₖ √λₖ` is the
//! correlation between variable `j` and component `k`.

use stat_core::{Matrix, Result, StatError, svd};

/// Result of a principal component analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct Pca {
    /// Column means.
    pub means: Vec<f64>,
    /// Column scale divisors (standard deviations for a correlation analysis, else 1).
    pub scales: Vec<f64>,
    /// Whether the analysis used correlations (standardized columns).
    pub standardized: bool,
    /// Component variances, decreasing.
    pub eigenvalues: Vec<f64>,
    /// Eigenvectors as columns (`p × r`).
    pub loadings: Matrix,
    /// Component scores (`n × r`).
    pub scores: Matrix,
}

impl Pca {
    /// Number of components `r = min(n − 1, p)`.
    #[must_use]
    pub fn n_components(&self) -> usize {
        self.eigenvalues.len()
    }

    /// Proportion of total variance for each component.
    #[must_use]
    pub fn proportions(&self) -> Vec<f64> {
        let total: f64 = self.eigenvalues.iter().sum();
        self.eigenvalues.iter().map(|l| if total > 0.0 { l / total } else { f64::NAN }).collect()
    }

    /// Cumulative proportions.
    #[must_use]
    pub fn cumulative(&self) -> Vec<f64> {
        let mut acc = 0.0;
        self.proportions()
            .into_iter()
            .map(|p| {
                acc += p;
                acc
            })
            .collect()
    }

    /// `vⱼₖ √λₖ`: for a correlation analysis, the correlation of variable `j`
    /// with component `k`; for a covariance analysis, its covariance.
    #[must_use]
    pub fn scaled_loadings(&self) -> Matrix {
        let mut m = self.loadings.clone();
        for k in 0..m.ncols() {
            let s = self.eigenvalues[k].max(0.0).sqrt();
            for j in 0..m.nrows() {
                m[(j, k)] *= s;
            }
        }
        m
    }

    /// Scores of new observations (rows of `x`, same columns as the data).
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] for the wrong number of columns.
    pub fn transform(&self, x: &Matrix) -> Result<Matrix> {
        let p = self.means.len();
        if x.ncols() != p {
            return Err(StatError::DimensionMismatch { context: "PCA columns", expected: p, found: x.ncols() });
        }
        let r = self.n_components();
        let mut out = Matrix::zeros(x.nrows(), r);
        for i in 0..x.nrows() {
            for k in 0..r {
                out[(i, k)] =
                    (0..p).map(|j| (x[(i, j)] - self.means[j]) / self.scales[j] * self.loadings[(j, k)]).sum();
            }
        }
        Ok(out)
    }
}

/// Principal components of the columns of `x` (`n` observations × `p` variables).
///
/// # Errors
/// [`StatError::InsufficientData`] for fewer than two observations;
/// [`StatError::NonFiniteInput`] for NaN or ±∞; [`StatError::InvalidArgument`]
/// if a column is constant in a correlation analysis.
pub fn principal_components(x: &Matrix, standardize: bool) -> Result<Pca> {
    let (n, p) = (x.nrows(), x.ncols());
    if n < 2 {
        return Err(StatError::InsufficientData { required: 2, found: n });
    }
    if p == 0 {
        return Err(StatError::InvalidArgument { message: "choose at least one variable".into() });
    }
    if !x.is_finite() {
        return Err(StatError::NonFiniteInput { context: "PCA data" });
    }
    let means: Vec<f64> = (0..p).map(|j| stat_core::summation::mean(x.col(j)).unwrap_or(0.0)).collect();
    let mut z = x.clone();
    let mut scales = vec![1.0; p];
    for j in 0..p {
        let m = means[j];
        z.col_mut(j).iter_mut().for_each(|v| *v -= m);
        if standardize {
            let ss: f64 = z.col(j).iter().map(|v| v * v).sum();
            let sd = (ss / (n - 1) as f64).sqrt();
            if sd == 0.0 {
                return Err(StatError::InvalidArgument {
                    message: format!("variable {} is constant, so it cannot be standardized", j + 1),
                });
            }
            scales[j] = sd;
            z.col_mut(j).iter_mut().for_each(|v| *v /= sd);
        }
    }
    let d = svd(&z)?;
    let r = (n - 1).min(p);
    let eigenvalues: Vec<f64> = d.singular_values[..r].iter().map(|s| s * s / (n - 1) as f64).collect();
    let mut loadings = Matrix::zeros(p, r);
    let mut scores = Matrix::zeros(n, r);
    for k in 0..r {
        // Sign: largest-magnitude loading positive. (The SVD's own sign rule
        // applies to the shorter side, which is U when n < p.)
        let big = (0..p).map(|j| d.v[(j, k)]).fold(0.0f64, |acc, x| if x.abs() > acc.abs() { x } else { acc });
        let sign = if big < 0.0 { -1.0 } else { 1.0 };
        for j in 0..p {
            loadings[(j, k)] = sign * d.v[(j, k)];
        }
        for i in 0..n {
            scores[(i, k)] = sign * d.u[(i, k)] * d.singular_values[k];
        }
    }
    Ok(Pca { means, scales, standardized: standardize, eigenvalues, loadings, scores })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn data() -> Matrix {
        Matrix::from_rows(&[
            [2.5, 2.4, 1.2],
            [0.5, 0.7, 0.3],
            [2.2, 2.9, 1.1],
            [1.9, 2.2, 0.8],
            [3.1, 3.0, 1.6],
            [2.3, 2.7, 1.0],
            [2.0, 1.6, 0.9],
            [1.0, 1.1, 0.4],
            [1.5, 1.6, 0.5],
            [1.1, 0.9, 0.7],
        ])
        .unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * b.abs().max(1.0)
    }

    /// Reference values: scikit-learn 1.9 `PCA` on the same centred (and
    /// standardized) data, eigenvector signs normalized the same way.
    #[test]
    fn matches_reference_covariance_and_correlation_pca() {
        let cov = principal_components(&data(), false).unwrap();
        for (a, b) in cov.eigenvalues.iter().zip([1.421_017_56, 0.060_566_18, 0.009_860_7]) {
            assert!((a - b).abs() < 1e-8, "{a} vs {b}");
        }
        for (j, b) in [0.647_579_66, 0.694_798_49, 0.312_882_79].iter().enumerate() {
            assert!((cov.loadings[(j, 0)] - b).abs() < 1e-8);
        }
        for (k, b) in [0.896_790_2, -0.184_893_27, -0.016_787_16].iter().enumerate() {
            assert!((cov.scores[(0, k)] - b).abs() < 1e-8, "score {k}");
        }
        let cor = principal_components(&data(), true).unwrap();
        for (a, b) in cor.eigenvalues.iter().zip([2.829_457_01, 0.139_891_78, 0.030_651_22]) {
            assert!((a - b).abs() < 1e-8);
        }
        assert!(close(cor.eigenvalues.iter().sum::<f64>(), 3.0), "correlation eigenvalues sum to p");
        for (j, b) in [0.588_021_71, 0.568_773_99, 0.575_088_35].iter().enumerate() {
            assert!((cor.loadings[(j, 0)] - b).abs() < 1e-8);
        }
        assert!((cor.cumulative()[2] - 1.0).abs() < 1e-14);
    }

    #[test]
    fn scores_are_uncorrelated_with_component_variances() {
        let p = principal_components(&data(), true).unwrap();
        let n = 10.0;
        for a in 0..3 {
            for b in 0..3 {
                let c: f64 = p.scores.col(a).iter().zip(p.scores.col(b)).map(|(x, y)| x * y).sum::<f64>() / (n - 1.0);
                let e = if a == b { p.eigenvalues[a] } else { 0.0 };
                assert!((c - e).abs() < 1e-12);
            }
        }
        // transform() reproduces the scores; scaled loadings are correlations.
        let t = p.transform(&data()).unwrap();
        assert!((t[(3, 1)] - p.scores[(3, 1)]).abs() < 1e-12);
        let s = p.scaled_loadings();
        let row_ss: f64 = (0..3).map(|k| s[(0, k)] * s[(0, k)]).sum();
        assert!((row_ss - 1.0).abs() < 1e-12, "a variable's squared correlations sum to 1");
    }

    /// With fewer observations than variables (the SVD then works on the
    /// transpose), each eigenvector's largest-magnitude element is still
    /// positive, and the components match NumPy 2 (`numpy.linalg.eigh` of
    /// the covariance matrix, signs normalized the same way).
    #[test]
    fn wide_data_keeps_the_sign_convention() {
        let x = Matrix::from_rows(&[[1.0, 4.0, -2.0, 3.0], [2.0, 1.0, 0.0, 5.0], [6.0, 2.0, 1.0, 1.0]]).unwrap();
        let pca = principal_components(&x, false).unwrap();
        assert_eq!(pca.n_components(), 2);
        for (a, b) in pca.eigenvalues.iter().zip(EIGENVALUES) {
            assert!((a - b).abs() < 1e-10, "{a} vs {b}");
        }
        for k in 0..2 {
            for j in 0..4 {
                assert!((pca.loadings[(j, k)] - LOADINGS[k][j]).abs() < 1e-10, "loading {j}, {k}");
            }
            // Scores still equal the centred data times the loadings.
            let t = pca.transform(&x).unwrap();
            for i in 0..3 {
                assert!((t[(i, k)] - pca.scores[(i, k)]).abs() < 1e-10);
            }
        }
    }

    const EIGENVALUES: [f64; 2] = [11.361_001_748_086_121, 4.305_664_918_580_546];
    const LOADINGS: [[f64; 4]; 2] = [
        [0.784_732_982_139_198_8, -0.158_315_420_858_870_85, 0.387_067_142_823_699_25, -0.457_503_443_929_656_7],
        [-0.029_819_563_513_453_41, 0.689_773_887_788_358_5, -0.382_885_771_698_905_9, -0.613_776_232_178_905_2],
    ];

    #[test]
    fn errors() {
        assert!(principal_components(&Matrix::zeros(1, 3), false).is_err());
        let mut c = data();
        c.col_mut(1).iter_mut().for_each(|v| *v = 2.0);
        assert!(principal_components(&c, true).is_err());
        assert!(principal_components(&c, false).is_ok(), "a constant column is fine for covariances");
    }
}
