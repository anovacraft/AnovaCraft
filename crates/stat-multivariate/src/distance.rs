//! Distance (dissimilarity) matrices between rows of a data set.
//!
//! For rows `x` and `y` over `p` variables:
//!
//! * **Euclidean** — `√Σₖ (xₖ − yₖ)²`.
//! * **Manhattan** (city block) — `Σₖ |xₖ − yₖ|`.
//! * **Bray–Curtis** — `Σₖ |xₖ − yₖ| / Σₖ (xₖ + yₖ)` for non-negative data
//!   such as abundances (Bray & Curtis 1957, *Ecological Monographs* 27;
//!   Legendre & Legendre 2012, *Numerical Ecology*, 3rd ed., §7.4,
//!   coefficient D14). Two rows that are both entirely zero are at
//!   distance 0.
//! * **Jaccard** — on presence/absence (a nonzero value is “present”),
//!   `1 − a / (a + b + c)` with `a` the variables present in both rows and
//!   `b`, `c` those present in only one (Jaccard 1901; Legendre & Legendre
//!   §7.3, coefficient S7). Double absences are ignored; two rows with
//!   nothing present are at distance 0.
//! * **Hamming** — the *number* of variables on which the rows differ.
//! * **Simple matching** — the *proportion* of variables on which the rows
//!   differ, `1 − m / p`, one minus Sokal and Michener's matching
//!   coefficient (Legendre & Legendre §7.3, coefficient S1).
//! * **Gower** — the general coefficient for mixed variable types (Gower
//!   1971, “A general coefficient of similarity and some of its
//!   properties”, *Biometrics* 27): `d(x, y) = Σₖ wₖ δₖ / Σₖ wₖ`, where
//!   for a numeric or ordinal variable `δₖ = |xₖ − yₖ| / Rₖ` with `Rₖ`
//!   the range of the variable over all rows (ordinal level codes are used
//!   as ranks, as R's `cluster::daisy` does; Kaufman & Rousseeuw 1990,
//!   *Finding Groups in Data*, ch. 1), for a nominal variable `δₖ` is 0
//!   when the levels agree and 1 otherwise, and for an asymmetric binary
//!   variable (nonzero = present) `δₖ` is the mismatch indicator but the
//!   variable is skipped (`wₖ = 0`) when both rows are absent. A variable
//!   missing in either row is skipped as well, so the average runs over
//!   the variables available for that pair. A variable with zero range
//!   contributes 0.
//!
//! Gower (1966, “Some distance properties of latent root and vector
//! methods used in multivariate analysis”, *Biometrika* 53) showed which of
//! these give Euclidean configurations; see [`crate::pcoa`] for principal
//! coordinates of any of them.

use stat_core::{Result, StatError};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// How the distance between two rows is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistanceMetric {
    /// Straight-line distance, `√Σ (x − y)²`.
    Euclidean,
    /// City-block distance, `Σ |x − y|`.
    Manhattan,
    /// Gower's coefficient for mixed numeric, ordinal, nominal and
    /// asymmetric binary variables; allows missing values.
    Gower,
    /// One minus Jaccard's similarity of presence/absence profiles.
    Jaccard,
    /// Bray–Curtis dissimilarity of non-negative profiles.
    BrayCurtis,
    /// Number of variables that differ.
    Hamming,
    /// Proportion of variables that differ.
    SimpleMatching,
}

impl DistanceMetric {
    /// Every metric, in menu order.
    pub const ALL: [Self; 7] = [
        Self::Euclidean,
        Self::Manhattan,
        Self::Gower,
        Self::Jaccard,
        Self::BrayCurtis,
        Self::Hamming,
        Self::SimpleMatching,
    ];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Euclidean => "Euclidean",
            Self::Manhattan => "Manhattan (city block)",
            Self::Gower => "Gower (mixed types)",
            Self::Jaccard => "Jaccard (presence/absence)",
            Self::BrayCurtis => "Bray–Curtis",
            Self::Hamming => "Hamming (count of differences)",
            Self::SimpleMatching => "Simple matching (share of differences)",
        }
    }

    /// Stable key used in saved requests and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Euclidean => "euclidean",
            Self::Manhattan => "manhattan",
            Self::Gower => "gower",
            Self::Jaccard => "jaccard",
            Self::BrayCurtis => "bray-curtis",
            Self::Hamming => "hamming",
            Self::SimpleMatching => "simple-matching",
        }
    }

    /// The metric with this [`key`](Self::key), if any.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    /// Whether the metric accepts missing values (only Gower does).
    #[must_use]
    pub fn allows_missing(self) -> bool {
        self == Self::Gower
    }
}

/// How a variable enters Gower's coefficient. The other metrics use the
/// values as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariableKind {
    /// Continuous values: range-scaled absolute difference.
    Numeric,
    /// Ordered level codes: range-scaled absolute difference of the codes.
    Ordinal,
    /// Unordered level codes: 0 if equal, 1 otherwise.
    Nominal,
    /// Presence (nonzero) / absence (0); joint absences are ignored.
    AsymmetricBinary,
}

/// One variable: its values for every row and its kind.
#[derive(Debug, Clone, PartialEq)]
pub struct DistanceColumn {
    /// One value per row; `NaN` marks a missing value. Ordinal and nominal
    /// values are level codes.
    pub values: Vec<f64>,
    /// How the variable is compared.
    pub kind: VariableKind,
}

/// A symmetric `n × n` matrix of non-negative distances with a zero
/// diagonal, stored row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct DistanceMatrix {
    n: usize,
    values: Vec<f64>,
}

impl DistanceMatrix {
    /// Wrap `values` (row-major, `n × n`).
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] when `n` is 0, `values` is not
    /// `n²` long, a distance is negative, the diagonal is not zero, or the
    /// matrix is not symmetric to `1e-8` of its largest entry;
    /// [`StatError::NonFiniteInput`] for NaN or ±∞.
    pub fn new(n: usize, values: Vec<f64>) -> Result<Self> {
        if n == 0 {
            return Err(invalid("a distance matrix needs at least one object"));
        }
        if values.len() != n * n {
            return Err(StatError::DimensionMismatch {
                context: "distance matrix entries vs. n²",
                expected: n * n,
                found: values.len(),
            });
        }
        if values.iter().any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "distance matrix" });
        }
        let scale = values.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        for i in 0..n {
            if values[i * n + i] != 0.0 {
                return Err(invalid("the distance matrix needs zeros on its diagonal"));
            }
            for j in 0..i {
                let (a, b) = (values[i * n + j], values[j * n + i]);
                if a < 0.0 || b < 0.0 {
                    return Err(invalid("distances cannot be negative"));
                }
                if (a - b).abs() > 1e-8 * scale {
                    return Err(invalid(format!(
                        "the distance matrix is not symmetric (rows {} and {} differ)",
                        i + 1,
                        j + 1
                    )));
                }
            }
        }
        Ok(Self { n, values })
    }

    /// Number of objects.
    #[must_use]
    pub fn n(&self) -> usize {
        self.n
    }

    /// Distance between objects `i` and `j`.
    ///
    /// # Panics
    /// When `i` or `j` is not below [`n`](Self::n).
    #[must_use]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        assert!(i < self.n && j < self.n, "distance index out of range");
        self.values[i * self.n + j]
    }

    /// The entries, row-major.
    #[must_use]
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// The matrix as `n` rows.
    #[must_use]
    pub fn rows(&self) -> Vec<Vec<f64>> {
        self.values.chunks(self.n).map(<[f64]>::to_vec).collect()
    }
}

/// Distances between every pair of rows of `columns` under `metric`.
///
/// All metrics except [`DistanceMetric::Gower`] use the values as given,
/// whatever each column's [`VariableKind`]; Gower uses the kinds as
/// described in the [module documentation](self).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no columns, columns of different
/// lengths, no rows, a missing value with a metric other than Gower (remove
/// incomplete rows first), a negative value with Bray–Curtis, or — with
/// Gower — a pair of rows that share no variable observed in both;
/// [`StatError::NonFiniteInput`] for ±∞.
pub fn distance_matrix(columns: &[DistanceColumn], metric: DistanceMetric) -> Result<DistanceMatrix> {
    let Some(first) = columns.first() else {
        return Err(invalid("distances need at least one variable"));
    };
    let n = first.values.len();
    if n == 0 {
        return Err(invalid("distances need at least one row"));
    }
    if columns.iter().any(|c| c.values.len() != n) {
        return Err(invalid("every variable needs one value per row"));
    }
    if columns.iter().flat_map(|c| &c.values).any(|v| v.is_infinite()) {
        return Err(StatError::NonFiniteInput { context: "distance data" });
    }
    if !metric.allows_missing() && columns.iter().flat_map(|c| &c.values).any(|v| v.is_nan()) {
        return Err(invalid(format!(
            "the {} distance needs complete rows: remove rows with missing values first",
            metric.label()
        )));
    }
    if metric == DistanceMetric::BrayCurtis && columns.iter().flat_map(|c| &c.values).any(|v| *v < 0.0) {
        return Err(invalid("the Bray–Curtis distance needs non-negative values"));
    }
    let ranges: Vec<f64> = columns
        .iter()
        .map(|c| {
            let (lo, hi) = c
                .values
                .iter()
                .filter(|v| !v.is_nan())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
            if hi > lo { hi - lo } else { 0.0 }
        })
        .collect();
    let p = columns.len() as f64;
    let mut values = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..i {
            let pair = columns.iter().map(|c| (c.values[i], c.values[j]));
            let d = match metric {
                DistanceMetric::Euclidean => pair.map(|(x, y)| (x - y) * (x - y)).sum::<f64>().sqrt(),
                DistanceMetric::Manhattan => pair.map(|(x, y)| (x - y).abs()).sum(),
                DistanceMetric::BrayCurtis => {
                    let (num, den) = pair.fold((0.0, 0.0), |(a, b), (x, y)| (a + (x - y).abs(), b + x + y));
                    if den > 0.0 { num / den } else { 0.0 }
                }
                DistanceMetric::Jaccard => {
                    let (mut both, mut one) = (0usize, 0usize);
                    for (x, y) in pair {
                        match (x != 0.0, y != 0.0) {
                            (true, true) => both += 1,
                            (true, false) | (false, true) => one += 1,
                            (false, false) => {}
                        }
                    }
                    if both + one == 0 { 0.0 } else { one as f64 / (both + one) as f64 }
                }
                DistanceMetric::Hamming => pair.filter(|(x, y)| x != y).count() as f64,
                DistanceMetric::SimpleMatching => pair.filter(|(x, y)| x != y).count() as f64 / p,
                DistanceMetric::Gower => gower_pair(columns, &ranges, i, j)?,
            };
            values[i * n + j] = d;
            values[j * n + i] = d;
        }
    }
    Ok(DistanceMatrix { n, values })
}

/// Gower's dissimilarity of rows `i` and `j`.
fn gower_pair(columns: &[DistanceColumn], ranges: &[f64], i: usize, j: usize) -> Result<f64> {
    let (mut sum, mut weight) = (0.0, 0.0);
    for (c, &range) in columns.iter().zip(ranges) {
        let (x, y) = (c.values[i], c.values[j]);
        if x.is_nan() || y.is_nan() {
            continue;
        }
        let delta = match c.kind {
            VariableKind::Numeric | VariableKind::Ordinal => {
                if range > 0.0 {
                    (x - y).abs() / range
                } else {
                    0.0
                }
            }
            VariableKind::Nominal => f64::from(u8::from(x != y)),
            VariableKind::AsymmetricBinary => {
                let (a, b) = (x != 0.0, y != 0.0);
                if !a && !b {
                    continue;
                }
                f64::from(u8::from(a != b))
            }
        };
        sum += delta;
        weight += 1.0;
    }
    if weight == 0.0 {
        return Err(invalid(format!(
            "rows {} and {} have no variable observed in both, so their Gower distance is undefined",
            j + 1,
            i + 1
        )));
    }
    Ok(sum / weight)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(values: &[f64], kind: VariableKind) -> DistanceColumn {
        DistanceColumn { values: values.to_vec(), kind }
    }

    fn data() -> Vec<DistanceColumn> {
        vec![
            col(&[1.0, 0.0, 3.0, 2.0, 5.0, 0.0], VariableKind::Numeric),
            col(&[0.0, 0.0, 4.0, 1.0, 2.0, 2.0], VariableKind::Numeric),
            col(&[2.5, 1.0, 0.0, 0.0, 3.0, 1.0], VariableKind::Numeric),
        ]
    }

    #[test]
    fn keys_round_trip() {
        for m in DistanceMetric::ALL {
            assert_eq!(DistanceMetric::from_key(m.key()), Some(m));
            assert!(!m.label().is_empty());
        }
        assert_eq!(DistanceMetric::from_key("cosine"), None);
    }

    #[test]
    fn every_metric_is_symmetric_with_zero_diagonal() {
        for m in DistanceMetric::ALL {
            let d = distance_matrix(&data(), m).unwrap();
            for i in 0..d.n() {
                assert_eq!(d.get(i, i), 0.0);
                for j in 0..d.n() {
                    assert_eq!(d.get(i, j), d.get(j, i));
                    assert!(d.get(i, j) >= 0.0);
                }
            }
            assert!(DistanceMatrix::new(d.n(), d.values().to_vec()).is_ok());
            assert_eq!(d.rows().len(), 6);
        }
    }

    #[test]
    fn euclidean_and_manhattan_obey_the_triangle_inequality() {
        for m in [DistanceMetric::Euclidean, DistanceMetric::Manhattan] {
            let d = distance_matrix(&data(), m).unwrap();
            for i in 0..6 {
                for j in 0..6 {
                    for k in 0..6 {
                        assert!(d.get(i, j) <= d.get(i, k) + d.get(k, j) + 1e-12);
                    }
                }
            }
        }
    }

    #[test]
    fn hand_computed_values() {
        let c = data();
        let e = distance_matrix(&c, DistanceMetric::Euclidean).unwrap();
        assert!((e.get(0, 1) - (1.0_f64 + 0.0 + 2.25).sqrt()).abs() < 1e-15);
        let bc = distance_matrix(&c, DistanceMetric::BrayCurtis).unwrap();
        // |1−0|+|0−0|+|2.5−1| over 1+0+0+0+2.5+1.
        assert!((bc.get(0, 1) - 2.5 / 4.5).abs() < 1e-15);
        let j = distance_matrix(&c, DistanceMetric::Jaccard).unwrap();
        // Row 0 present {0, 2}, row 1 present {2}: a = 1, b + c = 1.
        assert!((j.get(0, 1) - 0.5).abs() < 1e-15);
        let h = distance_matrix(&c, DistanceMetric::Hamming).unwrap();
        assert_eq!(h.get(0, 1), 2.0);
        let s = distance_matrix(&c, DistanceMetric::SimpleMatching).unwrap();
        assert!((s.get(0, 1) - 2.0 / 3.0).abs() < 1e-15);
    }

    #[test]
    fn gower_skips_missing_values_and_joint_absences() {
        let nan = f64::NAN;
        let c = vec![
            col(&[0.0, 10.0, nan], VariableKind::Numeric),
            col(&[1.0, 3.0, 2.0], VariableKind::Ordinal),
            col(&[1.0, 1.0, 2.0], VariableKind::Nominal),
            col(&[0.0, 0.0, 1.0], VariableKind::AsymmetricBinary),
        ];
        let d = distance_matrix(&c, DistanceMetric::Gower).unwrap();
        // Rows 0, 1: numeric 1, ordinal 1, nominal 0, binary skipped → 2/3.
        assert!((d.get(0, 1) - 2.0 / 3.0).abs() < 1e-15);
        // Rows 0, 2: numeric missing, ordinal 0.5, nominal 1, binary 1 → 2.5/3.
        assert!((d.get(0, 2) - 2.5 / 3.0).abs() < 1e-15);
        let lonely = vec![col(&[1.0, nan], VariableKind::Numeric), col(&[nan, 2.0], VariableKind::Numeric)];
        assert!(distance_matrix(&lonely, DistanceMetric::Gower).is_err());
    }

    #[test]
    fn rejects_bad_input() {
        let nan = vec![col(&[1.0, f64::NAN], VariableKind::Numeric)];
        assert!(distance_matrix(&nan, DistanceMetric::Euclidean).is_err());
        let neg = vec![col(&[1.0, -1.0], VariableKind::Numeric)];
        assert!(distance_matrix(&neg, DistanceMetric::BrayCurtis).is_err());
        assert!(distance_matrix(&[], DistanceMetric::Euclidean).is_err());
        let ragged = vec![col(&[1.0, 2.0], VariableKind::Numeric), col(&[1.0], VariableKind::Numeric)];
        assert!(distance_matrix(&ragged, DistanceMetric::Euclidean).is_err());
        assert!(DistanceMatrix::new(2, vec![0.0, 1.0, 2.0, 0.0]).is_err());
        assert!(DistanceMatrix::new(2, vec![0.0, -1.0, -1.0, 0.0]).is_err());
        assert!(DistanceMatrix::new(2, vec![1.0, 1.0, 1.0, 0.0]).is_err());
        assert!(DistanceMatrix::new(2, vec![0.0, 1.0, 1.0]).is_err());
    }
}
