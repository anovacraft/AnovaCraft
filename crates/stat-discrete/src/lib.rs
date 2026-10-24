//! # stat-discrete
//!
//! Categorical data analysis: two-way contingency tables with the classical
//! tests of independence (below), generalized linear models — logistic, probit,
//! complementary log-log, Poisson, normal and Gamma regression — in [`glm`], and
//! dose–response (probit) analysis with fiducial limits in [`dose_response`],
//! and ordinal (proportional-odds) and nominal (baseline-category) logistic
//! regression for responses with more than two categories in [`polytomous`].
//! Multi-way tables (counts, percentages and summary statistics by nested
//! classifications) are in [`tabulation`]; stratified tables
//! (Cochran–Mantel–Haenszel tests, the Mantel–Haenszel common odds ratio,
//! Breslow–Day homogeneity) in [`stratified`]; Fisher's exact test for
//! `r × c` tables (network algorithm) in [`exact_rxc`].
//!
//! For an `r × c` table of counts `nᵢⱼ` with row totals `nᵢ.`, column totals
//! `n.ⱼ` and grand total `n`, the expected counts under independence are
//! `Eᵢⱼ = nᵢ. n.ⱼ / n`, and
//!
//! * Pearson's statistic `X² = Σ (nᵢⱼ − Eᵢⱼ)² / Eᵢⱼ` (Pearson 1900),
//! * the likelihood-ratio statistic `G² = 2 Σ nᵢⱼ ln(nᵢⱼ / Eᵢⱼ)` (cells with
//!   `nᵢⱼ = 0` contribute 0),
//!
//! are both referred to χ² with `(r − 1)(c − 1)` degrees of freedom (Agresti,
//! *Categorical Data Analysis*, 3rd ed., 2013, §3.2). Cramér's
//! `V = √(X² / (n · (min(r, c) − 1)))` measures strength of association.
//!
//! The χ² approximation is doubtful when expected counts are small; the result
//! reports the smallest expected count and the share of cells below 5 so callers
//! can warn (Cochran's guideline: no more than 20% of cells below 5, none below 1).

pub mod dose_response;
pub mod exact_rxc;
pub mod glm;
pub mod negative_binomial;
pub mod multiple_response;
pub mod polytomous;
pub mod stratified;
pub mod tabulation;
pub mod treatment_effects;
pub use dose_response::{DoseGroup, DoseResponseFit, DoseResponseOptions, DoseScale, EffectiveDose, fit_dose_response};
pub use exact_rxc::{ExactMethod, ExactOptions, ExactTest, fisher_exact_rxc};
pub use glm::{Family, GlmCoefficient, GlmFit, GlmOptions, LikelihoodRatioTest, Link, fit_glm, likelihood_ratio_test};
pub use negative_binomial::{
    NegativeBinomialCoefficient, NegativeBinomialFit, NegativeBinomialOptions, fit_negative_binomial,
    negative_binomial_lr_test,
};
pub use polytomous::{
    NominalEquation, NominalLogitFit, OrdinalLogitFit, PolytomousOptions, PolytomousStatistics, ScoreTest,
    fit_nominal_logistic, fit_ordinal_logistic,
};
pub use treatment_effects::{
    BalanceRow, EffectEstimate, EffectMethod, EffectScale, Estimand, IntervalMethod, MatchingOptions, OutcomeKind,
    PropensityModel, TreatmentEffectFit, TreatmentEffectOptions, covariate_balance, estimate_treatment_effect,
    fit_propensity, heterogeneity_test,
};

use stat_core::distributions::{ChiSquared, ContinuousDistribution};
use stat_core::{Result, StatError};

/// A two-way table of counts with labelled rows and columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContingencyTable {
    /// Row level labels.
    pub row_labels: Vec<String>,
    /// Column level labels.
    pub column_labels: Vec<String>,
    /// `counts[i][j]`: observations in row level `i` and column level `j`.
    pub counts: Vec<Vec<u64>>,
}

impl ContingencyTable {
    /// Cross-classify paired labels. Levels are ordered by first appearance.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if the slices differ in length.
    pub fn from_pairs<S: AsRef<str>>(rows: &[S], columns: &[S]) -> Result<Self> {
        if rows.len() != columns.len() {
            return Err(StatError::DimensionMismatch {
                context: "row labels vs. column labels",
                expected: rows.len(),
                found: columns.len(),
            });
        }
        let mut row_labels: Vec<String> = Vec::new();
        let mut column_labels: Vec<String> = Vec::new();
        let index = |levels: &mut Vec<String>, v: &str| {
            levels.iter().position(|l| l == v).unwrap_or_else(|| {
                levels.push(v.to_owned());
                levels.len() - 1
            })
        };
        let mut cells: Vec<(usize, usize)> = Vec::with_capacity(rows.len());
        for (r, c) in rows.iter().zip(columns) {
            cells.push((index(&mut row_labels, r.as_ref()), index(&mut column_labels, c.as_ref())));
        }
        let mut counts = vec![vec![0u64; column_labels.len()]; row_labels.len()];
        for (i, j) in cells {
            counts[i][j] += 1;
        }
        Ok(Self { row_labels, column_labels, counts })
    }

    /// Row totals (saturating at `u64::MAX`; see [`checked_totals`](Self::checked_totals)).
    #[must_use]
    pub fn row_totals(&self) -> Vec<u64> {
        self.counts.iter().map(|r| r.iter().fold(0, |a: u64, &c| a.saturating_add(c))).collect()
    }

    /// Column totals (saturating; a cell missing from a ragged row counts as 0).
    #[must_use]
    pub fn column_totals(&self) -> Vec<u64> {
        (0..self.column_labels.len())
            .map(|j| self.counts.iter().fold(0, |a: u64, r| a.saturating_add(r.get(j).copied().unwrap_or(0))))
            .collect()
    }

    /// Grand total (saturating).
    #[must_use]
    pub fn total(&self) -> u64 {
        self.row_totals().iter().fold(0, |a: u64, &c| a.saturating_add(c))
    }

    /// Row totals, column totals and the grand total, checked.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] when the counts do not form a grid
    /// matching the labels, or a total exceeds `u64::MAX`.
    pub fn checked_totals(&self) -> Result<(Vec<u64>, Vec<u64>, u64)> {
        let c = self.column_labels.len();
        if self.counts.len() != self.row_labels.len() || self.counts.iter().any(|r| r.len() != c) {
            return Err(StatError::InvalidArgument {
                message: "the table's counts do not match its row and column labels".into(),
            });
        }
        let overflow = || StatError::InvalidArgument { message: "the table's counts are too large to total".into() };
        fn sum(v: impl IntoIterator<Item = u64>) -> Option<u64> {
            v.into_iter().try_fold(0u64, u64::checked_add)
        }
        let rows: Vec<u64> =
            self.counts.iter().map(|r| sum(r.iter().copied()).ok_or_else(overflow)).collect::<Result<_>>()?;
        let cols: Vec<u64> =
            (0..c).map(|j| sum(self.counts.iter().map(|r| r[j])).ok_or_else(overflow)).collect::<Result<_>>()?;
        let total = sum(rows.iter().copied()).ok_or_else(overflow)?;
        Ok((rows, cols, total))
    }
}

/// Result of testing a two-way table for independence.
#[derive(Debug, Clone, PartialEq)]
pub struct IndependenceTest {
    /// Expected counts under independence.
    pub expected: Vec<Vec<f64>>,
    /// Degrees of freedom `(r − 1)(c − 1)`.
    pub df: usize,
    /// Pearson's X².
    pub pearson_chi_square: f64,
    /// Upper-tail p-value of X².
    pub pearson_p_value: f64,
    /// Likelihood-ratio G².
    pub likelihood_ratio: f64,
    /// Upper-tail p-value of G².
    pub likelihood_ratio_p_value: f64,
    /// Cramér's V.
    pub cramers_v: f64,
    /// Smallest expected count.
    pub min_expected: f64,
    /// Fraction of cells with expected count below 5.
    pub fraction_expected_below_5: f64,
}

impl IndependenceTest {
    /// `true` when Cochran's guideline for the χ² approximation is violated
    /// (more than 20% of expected counts below 5, or any below 1).
    #[must_use]
    pub fn small_expected_counts(&self) -> bool {
        self.fraction_expected_below_5 > 0.2 || self.min_expected < 1.0
    }
}

/// Adjusted (standardized) residuals of each cell under independence,
/// `(O − E) / √(E (1 − rᵢ/N)(1 − cⱼ/N))`, approximately standard normal
/// when the rows and columns are independent (Haberman, *Biometrics* 29,
/// 1973; Agresti, *Categorical Data Analysis*, 3rd ed., §2.4.5). Cells in an
/// empty row or column, or whose variance is zero, are NaN.
///
/// # Errors
/// A ragged table, or totals beyond `u64::MAX`.
pub fn adjusted_residuals(table: &ContingencyTable) -> Result<Vec<Vec<f64>>> {
    let (rows, cols, n) = table.checked_totals()?;
    let n = n as f64;
    Ok(rows
        .iter()
        .enumerate()
        .map(|(i, &r)| {
            cols.iter()
                .enumerate()
                .map(|(j, &c)| {
                    let (r, c) = (r as f64, c as f64);
                    let e = r * c / n;
                    let v = e * (1.0 - r / n) * (1.0 - c / n);
                    if v > 0.0 { (table.counts[i][j] as f64 - e) / v.sqrt() } else { f64::NAN }
                })
                .collect()
        })
        .collect())
}

/// Test a contingency table for independence of its rows and columns.
///
/// # Errors
/// [`StatError::InvalidArgument`] if the table has fewer than two non-empty
/// rows or columns (the test needs at least a 2 × 2 table with positive margins),
/// is ragged, or has totals beyond `u64::MAX`.
pub fn test_independence(table: &ContingencyTable) -> Result<IndependenceTest> {
    let (rows, cols, n) = table.checked_totals()?;
    let n = n as f64;
    if rows.iter().filter(|&&r| r > 0).count() < 2 || cols.iter().filter(|&&c| c > 0).count() < 2 {
        return Err(StatError::InvalidArgument {
            message: "a test of independence needs at least two non-empty rows and two non-empty columns".into(),
        });
    }
    // Drop empty margins: they carry no information and would give E = 0.
    let keep_r: Vec<usize> = (0..rows.len()).filter(|&i| rows[i] > 0).collect();
    let keep_c: Vec<usize> = (0..cols.len()).filter(|&j| cols[j] > 0).collect();
    let mut expected = vec![vec![0.0; cols.len()]; rows.len()];
    let (mut x2, mut g2, mut min_e, mut below5) = (0.0, 0.0, f64::INFINITY, 0usize);
    for &i in &keep_r {
        for &j in &keep_c {
            let e = rows[i] as f64 * cols[j] as f64 / n;
            expected[i][j] = e;
            let o = table.counts[i][j] as f64;
            x2 += (o - e) * (o - e) / e;
            if o > 0.0 {
                g2 += o * (o / e).ln();
            }
            min_e = min_e.min(e);
            below5 += usize::from(e < 5.0);
        }
    }
    g2 *= 2.0;
    let df = (keep_r.len() - 1) * (keep_c.len() - 1);
    let chi = ChiSquared::new(df as f64)?;
    let k = keep_r.len().min(keep_c.len()) as f64 - 1.0;
    Ok(IndependenceTest {
        expected,
        df,
        pearson_chi_square: x2,
        pearson_p_value: chi.sf(x2)?,
        likelihood_ratio: g2,
        likelihood_ratio_p_value: chi.sf(g2.max(0.0))?,
        cramers_v: (x2 / (n * k)).sqrt(),
        min_expected: min_e,
        fraction_expected_below_5: below5 as f64 / (keep_r.len() * keep_c.len()) as f64,
    })
}

#[cfg(test)]
mod tests {
    /// R 4.3.3 `chisq.test(m)$stdres` for m = [12 5 9; 3 14 8; 6 2 11].
    #[test]
    #[allow(clippy::excessive_precision)]
    fn adjusted_residuals_match_r() {
        let t = ContingencyTable {
            row_labels: vec!["a".into(), "b".into(), "c".into()],
            column_labels: vec!["x".into(), "y".into(), "z".into()],
            counts: vec![vec![12, 5, 9], vec![3, 14, 8], vec![6, 2, 11]],
        };
        let r = adjusted_residuals(&t).unwrap();
        let expected = [
            2.267125964709535,
            -1.5114173098063566,
            -0.70690071771686935,
            -2.4494897427831779,
            3.5381518506868126,
            -1.0183501544346312,
            0.17595383576944659,
            -2.170097307823176,
            1.865350413124218,
        ];
        for (v, e) in r.iter().flatten().zip(expected) {
            assert!((v - e).abs() < 1e-12, "{v} vs {e}");
        }
    }

    use super::*;

    fn table(counts: Vec<Vec<u64>>) -> ContingencyTable {
        ContingencyTable {
            row_labels: (0..counts.len()).map(|i| format!("r{i}")).collect(),
            column_labels: (0..counts[0].len()).map(|j| format!("c{j}")).collect(),
            counts,
        }
    }

    /// 2 × 3 example; reference values from exact rational arithmetic.
    #[test]
    fn statistics_match_exact_reference() {
        let t = test_independence(&table(vec![vec![20, 15, 25], vec![30, 25, 10]])).unwrap();
        assert_eq!(t.df, 2);
        let expected = [[24.0, 19.2, 16.8], [26.0, 20.8, 18.2]];
        for (row, want) in t.expected.iter().zip(expected) {
            for (got, want) in row.iter().zip(want) {
                assert!((got - want).abs() < 1e-12);
            }
        }
        assert!((t.pearson_chi_square - 10.745_764_652_014_651).abs() < 1e-12);
        assert!((t.likelihood_ratio - 10.981_644_817_958_16).abs() < 1e-12);
        assert!((t.cramers_v - 0.293_199_790_614_040_56).abs() < 1e-14);
        // df = 2: the p-value is exactly exp(−X²/2).
        assert!((t.pearson_p_value - (-t.pearson_chi_square / 2.0).exp()).abs() < 1e-15);
        assert!(!t.small_expected_counts());
        assert_eq!(t.min_expected, 16.8);
    }

    #[test]
    fn independent_table_has_zero_statistics() {
        let t = test_independence(&table(vec![vec![10, 20], vec![30, 60]])).unwrap();
        assert!(t.pearson_chi_square.abs() < 1e-12 && t.likelihood_ratio.abs() < 1e-12);
        assert!((t.pearson_p_value - 1.0).abs() < 1e-12);
        assert!(t.cramers_v.abs() < 1e-6);
    }

    #[test]
    fn from_pairs_and_margins() {
        let rows = ["a", "b", "a", "a", "b"];
        let cols = ["x", "x", "y", "x", "y"];
        let t = ContingencyTable::from_pairs(&rows, &cols).unwrap();
        assert_eq!(t.row_labels, vec!["a", "b"]);
        assert_eq!(t.column_labels, vec!["x", "y"]);
        assert_eq!(t.counts, vec![vec![2, 1], vec![1, 1]]);
        assert_eq!((t.row_totals(), t.column_totals(), t.total()), (vec![3, 2], vec![3, 2], 5));
        assert!(ContingencyTable::from_pairs(&["a"], &["x", "y"]).is_err());
    }

    #[test]
    fn small_counts_and_degenerate_tables() {
        let t = test_independence(&table(vec![vec![3, 1], vec![1, 3]])).unwrap();
        assert!(t.small_expected_counts(), "all expected counts are 2");
        assert!(test_independence(&table(vec![vec![5, 7]])).is_err(), "one row");
        // An empty column is ignored rather than producing E = 0.
        let t = test_independence(&table(vec![vec![5, 0, 7], vec![6, 0, 2]])).unwrap();
        assert_eq!(t.df, 1);
        assert!(t.pearson_chi_square.is_finite());
    }

    #[test]
    fn ragged_table_is_rejected_not_a_panic() {
        let t = ContingencyTable {
            row_labels: vec!["a".into(), "b".into()],
            column_labels: vec!["x".into(), "y".into()],
            counts: vec![vec![1, 2], vec![3]],
        };
        assert!(test_independence(&t).is_err());
    }

    /// A ragged table (a row shorter than the labels) and counts whose totals
    /// overflow u64 are errors, not a panic or a silent wrap.
    #[test]
    fn ragged_and_overflowing_tables() {
        let mut ragged = table(vec![vec![1, 2], vec![3, 4]]);
        ragged.counts[1].pop();
        assert_eq!(ragged.column_totals(), vec![4, 2], "no panic: the missing cell counts as 0");
        assert!(ragged.checked_totals().is_err());
        assert!(test_independence(&ragged).is_err());
        let huge = table(vec![vec![u64::MAX, 1], vec![1, 1]]);
        assert_eq!(huge.total(), u64::MAX);
        assert!(huge.checked_totals().is_err());
        assert!(test_independence(&huge).is_err());
    }
}
