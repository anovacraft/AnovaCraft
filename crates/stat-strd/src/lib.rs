//! # stat-strd
//!
//! The NIST Statistical Reference Datasets (StRD) for linear least squares
//! regression, with their certified values, and the log relative error (LRE)
//! accuracy metric used to score software against them.
//!
//! The StRD were produced by the NIST Statistical Engineering and Mathematical and
//! Computational Sciences Divisions; the certified values were computed with
//! 500-digit arithmetic and are reported to 15 significant digits. They are a work
//! of the U.S. Government and are in the public domain.
//! <https://www.itl.nist.gov/div898/strd/>
//!
//! All eleven linear-regression datasets are included. Filip is parsed from NIST's
//! original `Filip.dat` (bundled in `data/`) by [`parse_strd_linear`]; the others
//! were transcribed from the published dataset listings.
//!
//! The 27 nonlinear regression problems are NIST's own files in `data/nls/`
//! (`scripts/fetch-nist-nls.sh` downloads them again); [`nls`] parses them and their
//! model expressions, and `tests/nls_certified.rs` scores the fits.
//!
//! # Accuracy metric
//! For a computed value `x` and certified value `c`, the log relative error is
//! `LRE = −log₁₀(|x − c| / |c|)` (or `−log₁₀|x|` when `c = 0`): roughly the number
//! of correct significant digits (McCullough 1998, "Assessing the reliability of
//! statistical software: Part I", *The American Statistician* 52(4)).

pub mod anova;
mod datasets;
mod nist_format;
pub mod nls;
pub mod univariate;

pub use anova::{AnovaDataset, CertifiedAnova, anova_datasets, parse_strd_anova};
pub use datasets::linear_regression_datasets;
pub use nist_format::{ParsedStrd, parse_strd_linear};
use stat_core::Matrix;
pub use univariate::{UnivariateDataset, univariate_datasets};

/// NIST's three-level difficulty rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Difficulty {
    /// Lower difficulty.
    Lower,
    /// Average difficulty.
    Average,
    /// Higher difficulty (ill-conditioned).
    Higher,
}

/// Certified regression results for one dataset.
#[derive(Debug, Clone, PartialEq)]
pub struct CertifiedValues {
    /// `(estimate, standard deviation of estimate)` for each parameter, intercept
    /// first if the model has one.
    pub parameters: Vec<(f64, f64)>,
    /// Residual standard deviation.
    pub residual_sd: f64,
    /// R² (uncentered for the no-intercept datasets).
    pub r_squared: f64,
    /// Regression degrees of freedom.
    pub df_regression: usize,
    /// Residual degrees of freedom.
    pub df_residual: usize,
    /// Regression sum of squares.
    pub ss_regression: f64,
    /// Residual sum of squares.
    pub ss_residual: f64,
    /// Overall F statistic; `None` where NIST reports it as undefined (exact fits).
    pub f_statistic: Option<f64>,
}

/// One linear-regression reference problem.
#[derive(Debug, Clone)]
pub struct LinearRegressionDataset {
    /// NIST dataset name, e.g. `"Longley"`.
    pub name: &'static str,
    /// NIST difficulty rating.
    pub difficulty: Difficulty,
    /// Whether the certified model includes an intercept.
    pub intercept: bool,
    /// Predictor matrix (polynomial terms already expanded; no column of ones).
    pub predictors: Matrix,
    /// Response vector.
    pub response: Vec<f64>,
    /// Certified results.
    pub certified: CertifiedValues,
}

/// Maximum LRE reported: certified values carry 15 significant digits.
pub const MAX_LRE: f64 = 15.0;

/// Log relative error of `computed` against `certified`, capped at [`MAX_LRE`].
///
/// Returns `0.0` when the computed value is non-finite or has fewer than zero
/// correct digits.
#[must_use]
pub fn log_relative_error(computed: f64, certified: f64) -> f64 {
    if !computed.is_finite() {
        return 0.0;
    }
    let err = if certified == 0.0 { computed.abs() } else { ((computed - certified) / certified).abs() };
    if err == 0.0 { MAX_LRE } else { (-err.log10()).clamp(0.0, MAX_LRE) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lre_counts_digits() {
        assert_eq!(log_relative_error(1.0, 1.0), MAX_LRE);
        assert!((log_relative_error(1.000_001, 1.0) - 6.0).abs() < 1e-6);
        assert!((log_relative_error(1e-9, 0.0) - 9.0).abs() < 1e-9);
        assert_eq!(log_relative_error(f64::NAN, 1.0), 0.0);
        assert_eq!(log_relative_error(5.0, 1.0), 0.0);
    }

    #[test]
    fn datasets_are_well_formed() {
        let all = linear_regression_datasets();
        assert_eq!(all.len(), 11);
        for d in &all {
            let n = d.response.len();
            assert_eq!(d.predictors.nrows(), n, "{}", d.name);
            let k = d.predictors.ncols() + usize::from(d.intercept);
            assert_eq!(d.certified.parameters.len(), k, "{}", d.name);
            assert_eq!(d.certified.df_residual, n - k, "{}", d.name);
            assert_eq!(d.certified.df_regression, d.predictors.ncols(), "{}", d.name);
        }
    }
}
