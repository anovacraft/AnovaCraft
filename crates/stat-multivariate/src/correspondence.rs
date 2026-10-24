//! Simple correspondence analysis of a two-way table of counts.
//!
//! For an `r × c` table `N` with total `n`, correspondence matrix `P = N/n`,
//! row masses `rᵢ` and column masses `cⱼ`, the standardized residuals
//!
//! ```text
//! S = D_r^{-1/2} (P − r cᵀ) D_c^{-1/2}
//! ```
//!
//! have the SVD `S = U Σ Vᵀ` ([`stat_core::svd()`]). The **principal inertias**
//! are `λₖ = σₖ²`; they sum to the total inertia `χ²/n`, where `χ²` is
//! Pearson's statistic for independence. Row and column **principal
//! coordinates** are `F = D_r^{-1/2} U Σ` and `G = D_c^{-1/2} V Σ`; rows (or
//! columns) with similar profiles lie close together, and a symmetric map plots
//! both on the first two dimensions (Greenacre, *Correspondence Analysis in
//! Practice*, 3rd ed., chs. 4–8; Benzécri 1973). At most `min(r, c) − 1`
//! dimensions carry inertia. Each dimension's sign is chosen so that the
//! largest-magnitude column coordinate is positive.

use stat_core::{Matrix, Result, StatError, svd};

/// Result of a correspondence analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrespondenceAnalysis {
    /// Principal inertias, decreasing.
    pub inertias: Vec<f64>,
    /// Row masses.
    pub row_masses: Vec<f64>,
    /// Column masses.
    pub column_masses: Vec<f64>,
    /// Row principal coordinates (`r × dims`).
    pub row_coordinates: Matrix,
    /// Column principal coordinates (`c × dims`).
    pub column_coordinates: Matrix,
    /// Grand total of the table.
    pub total: f64,
}

impl CorrespondenceAnalysis {
    /// Total inertia `Σλₖ = χ²/n`.
    #[must_use]
    pub fn total_inertia(&self) -> f64 {
        self.inertias.iter().sum()
    }

    /// Pearson's χ² for independence, `n · total inertia`.
    #[must_use]
    pub fn chi_square(&self) -> f64 {
        self.total * self.total_inertia()
    }

    /// Share of the total inertia on each dimension.
    #[must_use]
    pub fn proportions(&self) -> Vec<f64> {
        let t = self.total_inertia();
        self.inertias.iter().map(|l| if t > 0.0 { l / t } else { f64::NAN }).collect()
    }
}

/// Correspondence analysis of `counts[i][j]` (rows × columns).
///
/// # Errors
/// [`StatError::InvalidArgument`] for fewer than two rows or columns, a ragged
/// table, negative or non-finite counts, or a row or column that sums to zero.
pub fn correspondence_analysis(counts: &[Vec<f64>]) -> Result<CorrespondenceAnalysis> {
    let r = counts.len();
    let c = counts.first().map_or(0, Vec::len);
    if r < 2 || c < 2 {
        return Err(StatError::InvalidArgument {
            message: "correspondence analysis needs at least a 2 × 2 table".into(),
        });
    }
    if counts.iter().any(|row| row.len() != c) {
        return Err(StatError::InvalidArgument { message: "every row needs the same number of columns".into() });
    }
    if counts.iter().flatten().any(|v| !v.is_finite() || *v < 0.0) {
        return Err(StatError::InvalidArgument { message: "counts must be finite and non-negative".into() });
    }
    let total: f64 = counts.iter().flatten().sum();
    let rows: Vec<f64> = counts.iter().map(|row| row.iter().sum::<f64>() / total).collect();
    let cols: Vec<f64> = (0..c).map(|j| counts.iter().map(|row| row[j]).sum::<f64>() / total).collect();
    if rows.iter().chain(&cols).any(|m| m.is_nan() || *m <= 0.0) {
        return Err(StatError::InvalidArgument { message: "every row and column needs a positive total".into() });
    }
    let mut s = Matrix::zeros(r, c);
    for i in 0..r {
        for j in 0..c {
            let expected = rows[i] * cols[j];
            s[(i, j)] = (counts[i][j] / total - expected) / expected.sqrt();
        }
    }
    let d = svd(&s)?;
    let dims = (r.min(c) - 1).min(d.singular_values.len());
    let inertias: Vec<f64> = d.singular_values[..dims].iter().map(|v| v * v).collect();
    let mut row_coordinates = Matrix::zeros(r, dims);
    let mut column_coordinates = Matrix::zeros(c, dims);
    for k in 0..dims {
        let sv = d.singular_values[k];
        // Sign: largest-magnitude column coordinate positive.
        let big = (0..c).map(|j| d.v[(j, k)]).fold(0.0f64, |acc, x| if x.abs() > acc.abs() { x } else { acc });
        let sign = if big < 0.0 { -1.0 } else { 1.0 };
        for i in 0..r {
            row_coordinates[(i, k)] = sign * d.u[(i, k)] * sv / rows[i].sqrt();
        }
        for j in 0..c {
            column_coordinates[(j, k)] = sign * d.v[(j, k)] * sv / cols[j].sqrt();
        }
    }
    Ok(CorrespondenceAnalysis {
        inertias,
        row_masses: rows,
        column_masses: cols,
        row_coordinates,
        column_coordinates,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Greenacre's staff-smoking table (staff group × smoking level).
    fn smoking() -> Vec<Vec<f64>> {
        vec![
            vec![4.0, 2.0, 3.0, 2.0],
            vec![4.0, 3.0, 7.0, 4.0],
            vec![25.0, 10.0, 12.0, 4.0],
            vec![18.0, 24.0, 33.0, 13.0],
            vec![10.0, 6.0, 7.0, 2.0],
        ]
    }

    /// The published principal inertias of this table are 0.0748, 0.0100 and
    /// 0.0004, total 0.0852 (Greenacre 1984, *Theory and Applications of
    /// Correspondence Analysis*); the total inertia equals χ²/n.
    #[test]
    fn smoking_data_inertias() {
        let ca = correspondence_analysis(&smoking()).unwrap();
        assert_eq!(ca.inertias.len(), 3);
        for (a, b) in ca.inertias.iter().zip([0.074_759_105_9, 0.010_017_180_5, 0.000_413_574_08]) {
            assert!((a - b).abs() < 1e-8 * b, "{a} vs {b}");
        }
        assert!((ca.total_inertia() - 0.085_189_860_477_840_7).abs() < 1e-12);
        // χ² from first principles.
        let t = smoking();
        let n: f64 = t.iter().flatten().sum();
        let mut chi = 0.0;
        for row in &t {
            for (j, &o) in row.iter().enumerate() {
                let e = row.iter().sum::<f64>() * t.iter().map(|r| r[j]).sum::<f64>() / n;
                chi += (o - e) * (o - e) / e;
            }
        }
        assert!((ca.chi_square() - chi).abs() < 1e-9);
    }

    #[test]
    fn coordinates_are_centred_and_reproduce_inertia() {
        let ca = correspondence_analysis(&smoking()).unwrap();
        for k in 0..3 {
            // Mass-weighted coordinates are centred, and their weighted squares sum to λₖ.
            let centre: f64 = (0..5).map(|i| ca.row_masses[i] * ca.row_coordinates[(i, k)]).sum();
            assert!(centre.abs() < 1e-12);
            let inertia: f64 = (0..4).map(|j| ca.column_masses[j] * ca.column_coordinates[(j, k)].powi(2)).sum();
            assert!((inertia - ca.inertias[k]).abs() < 1e-12);
        }
        assert!((ca.proportions()[0] - 0.877_6).abs() < 1e-3, "87.8% on the first dimension");
    }

    #[test]
    fn rejects_bad_tables() {
        assert!(correspondence_analysis(&[vec![1.0, 2.0]]).is_err());
        assert!(correspondence_analysis(&[vec![1.0, 2.0], vec![3.0]]).is_err());
        assert!(correspondence_analysis(&[vec![1.0, 0.0], vec![3.0, 0.0]]).is_err(), "empty column");
        assert!(correspondence_analysis(&[vec![1.0, -2.0], vec![3.0, 4.0]]).is_err());
    }
}
