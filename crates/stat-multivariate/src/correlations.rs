//! Correlation matrices of several continuous variables.
//!
//! For every pair of variables the routine reports
//!
//! * **Pearson's r** with its t test of ρ = 0 (`t = r √((n−2)/(1−r²))` on
//!   `n − 2` df) and Fisher's z interval `tanh(atanh r ± z_{1−γ/2}/√(n−3))`
//!   (Fisher 1921), computed by [`stat_core::correlation::pearson`];
//! * **Spearman's ρₛ** (Pearson's r of average ranks), with the same t test
//!   and interval ([`stat_core::correlation::spearman`]);
//! * the **covariance** (divisor `n − 1`).
//!
//! Missing values (NaN) are handled one of two ways:
//!
//! * **pairwise** — each pair uses every row that has both values, so pairs
//!   can rest on different rows and counts;
//! * **listwise** — only rows complete in every variable are used, so every
//!   pair shares one set of rows.
//!
//! **Partial correlations** — each pair's correlation with every other
//! variable held fixed — always use the complete rows. With `P = R⁻¹` the
//! inverse of their correlation matrix, `r_{ij·rest} = −P_{ij}/√(P_{ii}P_{jj})`
//! (Whittaker, *Graphical Models in Applied Multivariate Statistics*, 1990,
//! §5.7). Its t test has `n − p` degrees of freedom: `n − 2` less one for each
//! of the `p − 2` variables partialled out (Anderson, *An Introduction to
//! Multivariate Statistical Analysis*, 3rd ed., §4.3).

use stat_core::correlation::{Correlation, pearson, spearman};
use stat_core::distributions::StudentsT;
use stat_core::summation::sum_iter;
use stat_core::{Result, StatError};

use crate::spd::{cholesky, inverse};

/// Which rows a pair of variables uses when values are missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissingRows {
    /// Every row with both values of the pair.
    #[default]
    Pairwise,
    /// Only rows with a value in every variable.
    Listwise,
}

impl MissingRows {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pairwise => "Pairwise (rows with both values)",
            Self::Listwise => "Listwise (rows complete in every column)",
        }
    }

    /// Stable key used in project files and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Pairwise => "pairwise",
            Self::Listwise => "listwise",
        }
    }

    /// The choice with this [`MissingRows::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Pairwise, Self::Listwise].into_iter().find(|m| m.key() == key)
    }
}

/// Statistics of one pair of variables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairCorrelation {
    /// Rows used by the pair.
    pub n: usize,
    /// Pearson's r with test and interval (`None` when fewer than three rows
    /// or a variable is constant over the pair's rows).
    pub pearson: Option<Correlation>,
    /// Spearman's ρₛ (`None` as for Pearson).
    pub spearman: Option<Correlation>,
    /// Sample covariance (NaN with fewer than two rows).
    pub covariance: f64,
}

/// Partial correlations of every pair given all the other variables.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialCorrelations {
    /// Complete rows used.
    pub n: usize,
    /// Degrees of freedom of each test, `n − p`.
    pub df: f64,
    /// `p × p` estimates (1 on the diagonal).
    pub estimates: Vec<Vec<f64>>,
    /// Two-sided p-values (`None` on the diagonal and where |r| = 1).
    pub p_values: Vec<Vec<Option<f64>>>,
}

/// Per-variable summaries and every pair's correlations.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrelationMatrix {
    /// How missing values were handled.
    pub missing: MissingRows,
    /// Confidence level of the intervals.
    pub level: f64,
    /// Rows with a value in every variable.
    pub complete_rows: usize,
    /// Values used for each variable's summaries.
    pub counts: Vec<usize>,
    /// Each variable's mean.
    pub means: Vec<f64>,
    /// Each variable's standard deviation (divisor `n − 1`).
    pub std_devs: Vec<f64>,
    /// `pairs[i][j]` for `i ≠ j` (symmetric); `None` on the diagonal.
    pub pairs: Vec<Vec<Option<PairCorrelation>>>,
    /// Partial correlations, when the complete rows outnumber the variables
    /// and their correlation matrix is nonsingular (`None` otherwise, and
    /// always for two variables, where they equal the ordinary correlation).
    pub partial: Option<PartialCorrelations>,
}

impl CorrelationMatrix {
    /// Number of variables.
    #[must_use]
    pub fn n_variables(&self) -> usize {
        self.means.len()
    }

    /// Pearson's r of variables `i` and `j` (1 when `i == j`; NaN when it
    /// could not be computed).
    #[must_use]
    pub fn pearson_r(&self, i: usize, j: usize) -> f64 {
        if i == j {
            return 1.0;
        }
        self.pairs[i][j].and_then(|p| p.pearson).map_or(f64::NAN, |c| c.estimate)
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

fn mean_sd(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    if v.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let m = sum_iter(v.iter().copied()) / n;
    let sd = if v.len() > 1 { (sum_iter(v.iter().map(|x| (x - m) * (x - m))) / (n - 1.0)).sqrt() } else { f64::NAN };
    (m, sd)
}

/// Correlations of the variables in `columns` (equal lengths; NaN marks a
/// missing value) with intervals at confidence `level`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two variables, columns of
/// unequal length, infinite values or `level ∉ (0, 1)`.
pub fn correlation_matrix(columns: &[Vec<f64>], missing: MissingRows, level: f64) -> Result<CorrelationMatrix> {
    let p = columns.len();
    if p < 2 {
        return Err(invalid("choose at least two columns"));
    }
    let n_rows = columns[0].len();
    if columns.iter().any(|c| c.len() != n_rows) {
        return Err(StatError::DimensionMismatch {
            context: "correlation columns",
            expected: n_rows,
            found: columns.iter().map(Vec::len).find(|&l| l != n_rows).unwrap_or(0),
        });
    }
    if columns.iter().flatten().any(|v| v.is_infinite()) {
        return Err(StatError::NonFiniteInput { context: "correlation input" });
    }
    if !(level > 0.0 && level < 1.0) {
        return Err(invalid(format!("confidence level must lie in (0, 1) (got {level})")));
    }
    let complete: Vec<usize> = (0..n_rows).filter(|&r| columns.iter().all(|c| !c[r].is_nan())).collect();
    let rows_of = |vars: &[usize]| -> Vec<usize> {
        match missing {
            MissingRows::Listwise => complete.clone(),
            MissingRows::Pairwise => (0..n_rows).filter(|&r| vars.iter().all(|&j| !columns[j][r].is_nan())).collect(),
        }
    };
    let mut counts = Vec::with_capacity(p);
    let mut means = Vec::with_capacity(p);
    let mut std_devs = Vec::with_capacity(p);
    for j in 0..p {
        let v: Vec<f64> = rows_of(&[j]).iter().map(|&r| columns[j][r]).collect();
        let (m, sd) = mean_sd(&v);
        counts.push(v.len());
        means.push(m);
        std_devs.push(sd);
    }
    let mut pairs = vec![vec![None; p]; p];
    for i in 0..p {
        for j in i + 1..p {
            let rows = rows_of(&[i, j]);
            let x: Vec<f64> = rows.iter().map(|&r| columns[i][r]).collect();
            let y: Vec<f64> = rows.iter().map(|&r| columns[j][r]).collect();
            let summary = pearson(&x, &y, level).ok();
            let covariance = summary.map_or_else(
                || {
                    if x.len() < 2 {
                        return f64::NAN;
                    }
                    let (mx, my) = (mean_sd(&x).0, mean_sd(&y).0);
                    sum_iter(x.iter().zip(&y).map(|(a, b)| (a - mx) * (b - my))) / (x.len() as f64 - 1.0)
                },
                |s| s.covariance,
            );
            let pair = PairCorrelation {
                n: rows.len(),
                pearson: summary.map(|s| s.correlation),
                spearman: spearman(&x, &y, level).ok(),
                covariance,
            };
            pairs[i][j] = Some(pair);
            pairs[j][i] = Some(pair);
        }
    }
    let partial = if p > 2 { partial_correlations(columns, &complete)? } else { None };
    Ok(CorrelationMatrix { missing, level, complete_rows: complete.len(), counts, means, std_devs, pairs, partial })
}

/// Partial correlations from the complete `rows` (`None` when there are no
/// more rows than variables or their correlation matrix is singular).
fn partial_correlations(columns: &[Vec<f64>], rows: &[usize]) -> Result<Option<PartialCorrelations>> {
    let p = columns.len();
    let n = rows.len();
    if n <= p {
        return Ok(None);
    }
    let data: Vec<Vec<f64>> = rows.iter().map(|&r| columns.iter().map(|c| c[r]).collect()).collect();
    let mean = crate::spd::mean_vector(&data, p);
    let cov = crate::spd::covariance(&data, &mean);
    if cov.iter().enumerate().any(|(i, r)| r[i] <= 0.0) {
        return Ok(None);
    }
    let corr: Vec<Vec<f64>> =
        (0..p).map(|i| (0..p).map(|j| cov[i][j] / (cov[i][i] * cov[j][j]).sqrt()).collect()).collect();
    let Some(l) = cholesky(&corr) else { return Ok(None) };
    let inv = inverse(&l);
    let df = (n - p) as f64;
    let t_dist = StudentsT::new(df)?;
    let mut estimates = vec![vec![1.0; p]; p];
    let mut p_values = vec![vec![None; p]; p];
    for i in 0..p {
        for j in 0..p {
            if i == j {
                continue;
            }
            let r = (-inv[i][j] / (inv[i][i] * inv[j][j]).sqrt()).clamp(-1.0, 1.0);
            estimates[i][j] = r;
            if r.abs() < 1.0 {
                let t = r * (df / (1.0 - r * r)).sqrt();
                p_values[i][j] = Some(t_dist.two_sided_p_value(t)?);
            }
        }
    }
    Ok(Some(PartialCorrelations { n, df, estimates, p_values }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAN: f64 = f64::NAN;

    fn data() -> Vec<Vec<f64>> {
        vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
            vec![2.0, 1.0, 4.0, 3.0, 7.0, 8.0, 6.0, 9.0],
            vec![5.0, 3.0, 4.0, 1.0, 2.0, 2.0, 0.0, 1.0],
        ]
    }

    #[test]
    fn pearson_pairs_match_the_bivariate_routine() {
        let m = correlation_matrix(&data(), MissingRows::Pairwise, 0.95).unwrap();
        // r(x, y) = 0.8964214570007952 (exact rational reference in
        // stat_core::correlation's tests, same data).
        assert!((m.pearson_r(0, 1) - 0.896_421_457_000_795_2).abs() < 1e-14);
        assert_eq!(m.pearson_r(1, 0), m.pearson_r(0, 1));
        assert_eq!(m.pearson_r(2, 2), 1.0);
        assert_eq!(m.complete_rows, 8);
        // Means and SDs by the definitions: mean(z) = 18/8, Σ(z − z̄)² = 19.5.
        assert!((m.means[2] - 2.25).abs() < 1e-15);
        assert!((m.std_devs[2] - (19.5_f64 / 7.0).sqrt()).abs() < 1e-14);
        // Covariance of x and z: Σ(x − 4.5)(z − 2.25) = −24, / 7.
        let c = m.pairs[0][2].unwrap().covariance;
        assert!((c - (-24.0 / 7.0)).abs() < 1e-14);
    }

    #[test]
    fn partial_correlation_matches_the_three_variable_formula() {
        // r_{xy·z} = (r_xy − r_xz r_yz) / √((1 − r_xz²)(1 − r_yz²)), a formula
        // independent of the matrix inverse (Yule 1907).
        let m = correlation_matrix(&data(), MissingRows::Pairwise, 0.95).unwrap();
        let (rxy, rxz, ryz) = (m.pearson_r(0, 1), m.pearson_r(0, 2), m.pearson_r(1, 2));
        let want = (rxy - rxz * ryz) / ((1.0 - rxz * rxz) * (1.0 - ryz * ryz)).sqrt();
        let partial = m.partial.as_ref().unwrap();
        assert!((partial.estimates[0][1] - want).abs() < 1e-13, "{} vs {want}", partial.estimates[0][1]);
        assert_eq!(partial.df, 5.0);
        // The test: t = r √(df / (1 − r²)) on n − p = 5 df.
        let t = want * (5.0 / (1.0 - want * want)).sqrt();
        let p = StudentsT::new(5.0).unwrap().two_sided_p_value(t).unwrap();
        assert!((partial.p_values[0][1].unwrap() - p).abs() < 1e-12);
        assert!(partial.p_values[2][2].is_none());
    }

    #[test]
    fn pairwise_and_listwise_rows_differ_with_missing_values() {
        let mut d = data();
        d[2][0] = NAN; // z missing in row 0
        d[1][7] = NAN; // y missing in row 7
        let pw = correlation_matrix(&d, MissingRows::Pairwise, 0.95).unwrap();
        let lw = correlation_matrix(&d, MissingRows::Listwise, 0.95).unwrap();
        assert_eq!(pw.pairs[0][1].unwrap().n, 7);
        assert_eq!(pw.pairs[0][2].unwrap().n, 7);
        assert_eq!(pw.pairs[1][2].unwrap().n, 6);
        assert_eq!(pw.counts, [8, 7, 7]);
        assert_eq!(lw.complete_rows, 6);
        assert!(lw.pairs[0][1].iter().all(|p| p.n == 6));
        assert_eq!(lw.counts, [6, 6, 6]);
        // Pairwise x–y uses rows 0–6: same r as the subset by hand.
        let x = &d[0][..7];
        let y = &d[1][..7];
        let r = pearson(x, y, 0.95).unwrap().correlation.estimate;
        assert!((pw.pearson_r(0, 1) - r).abs() < 1e-15);
        assert_eq!(pw.partial.as_ref().unwrap().n, 6, "partials use complete rows");
    }

    #[test]
    fn spearman_and_degenerate_cases() {
        let d = vec![vec![1.0, 2.0, 3.0, 4.0, 5.0], vec![1.0, 4.0, 9.0, 16.0, 25.0], vec![3.0, 3.0, 3.0, 3.0, 3.0]];
        let m = correlation_matrix(&d, MissingRows::Pairwise, 0.95).unwrap();
        // A monotone relation has ρₛ = 1 exactly.
        assert!((m.pairs[0][1].unwrap().spearman.unwrap().estimate - 1.0).abs() < 1e-15);
        // A constant column has no correlation but a zero covariance.
        assert!(m.pairs[0][2].unwrap().pearson.is_none());
        assert_eq!(m.pairs[0][2].unwrap().covariance, 0.0);
        assert!(m.partial.is_none(), "singular correlation matrix");
        assert!(correlation_matrix(&d[..1], MissingRows::Pairwise, 0.95).is_err());
        assert!(correlation_matrix(&d, MissingRows::Pairwise, 1.0).is_err());
        assert!(correlation_matrix(&[vec![1.0], vec![1.0, 2.0]], MissingRows::Pairwise, 0.95).is_err());
        assert_eq!(MissingRows::from_key("listwise"), Some(MissingRows::Listwise));
    }
}
