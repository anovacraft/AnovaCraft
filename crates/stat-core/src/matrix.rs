//! A minimal dense, column-major `f64` matrix.
//!
//! Column-major storage is chosen because the dominant workloads (Householder QR,
//! least squares, per-variable statistics) walk down columns. The type is
//! deliberately small: it is the interchange format between crates, not a
//! general linear-algebra library. When a full backend such as `faer` is adopted,
//! conversion to and from this type is a single contiguous copy.

use crate::error::{Result, StatError};

/// Dense matrix of `f64` stored in column-major order.
///
/// Element `(i, j)` (row `i`, column `j`) lives at `data[j * nrows + i]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    nrows: usize,
    ncols: usize,
    data: Vec<f64>,
}

impl Matrix {
    /// An `nrows × ncols` matrix of zeros.
    #[must_use]
    pub fn zeros(nrows: usize, ncols: usize) -> Self {
        Self { nrows, ncols, data: vec![0.0; nrows * ncols] }
    }

    /// The `n × n` identity matrix.
    #[must_use]
    pub fn identity(n: usize) -> Self {
        let mut m = Self::zeros(n, n);
        for i in 0..n {
            m[(i, i)] = 1.0;
        }
        m
    }

    /// Build from column-major data.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if `data.len() != nrows * ncols`.
    pub fn from_col_major(nrows: usize, ncols: usize, data: Vec<f64>) -> Result<Self> {
        if data.len() != nrows * ncols {
            return Err(StatError::DimensionMismatch {
                context: "matrix data length vs. nrows * ncols",
                expected: nrows * ncols,
                found: data.len(),
            });
        }
        Ok(Self { nrows, ncols, data })
    }

    /// Build from a slice of equal-length columns.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if the columns differ in length.
    pub fn from_columns<C: AsRef<[f64]>>(columns: &[C]) -> Result<Self> {
        let nrows = columns.first().map_or(0, |c| c.as_ref().len());
        let mut data = Vec::with_capacity(nrows * columns.len());
        for c in columns {
            let c = c.as_ref();
            if c.len() != nrows {
                return Err(StatError::DimensionMismatch { context: "column length", expected: nrows, found: c.len() });
            }
            data.extend_from_slice(c);
        }
        Ok(Self { nrows, ncols: columns.len(), data })
    }

    /// Build from a slice of equal-length rows.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if the rows differ in length.
    pub fn from_rows<R: AsRef<[f64]>>(rows: &[R]) -> Result<Self> {
        let ncols = rows.first().map_or(0, |r| r.as_ref().len());
        let mut m = Self::zeros(rows.len(), ncols);
        for (i, r) in rows.iter().enumerate() {
            let r = r.as_ref();
            if r.len() != ncols {
                return Err(StatError::DimensionMismatch { context: "row length", expected: ncols, found: r.len() });
            }
            for (j, &v) in r.iter().enumerate() {
                m[(i, j)] = v;
            }
        }
        Ok(m)
    }

    /// Number of rows.
    #[must_use]
    pub fn nrows(&self) -> usize {
        self.nrows
    }

    /// Number of columns.
    #[must_use]
    pub fn ncols(&self) -> usize {
        self.ncols
    }

    /// Column `j` as a contiguous slice.
    ///
    /// # Panics
    /// If `j >= ncols`.
    #[must_use]
    pub fn col(&self, j: usize) -> &[f64] {
        assert!(j < self.ncols, "column index {j} out of bounds ({} columns)", self.ncols);
        &self.data[j * self.nrows..(j + 1) * self.nrows]
    }

    /// Column `j` as a mutable contiguous slice.
    ///
    /// # Panics
    /// If `j >= ncols`.
    pub fn col_mut(&mut self, j: usize) -> &mut [f64] {
        assert!(j < self.ncols, "column index {j} out of bounds ({} columns)", self.ncols);
        &mut self.data[j * self.nrows..(j + 1) * self.nrows]
    }

    /// Row `i` copied into a new vector.
    #[must_use]
    pub fn row(&self, i: usize) -> Vec<f64> {
        (0..self.ncols).map(|j| self[(i, j)]).collect()
    }

    /// Column-major backing storage.
    #[must_use]
    pub fn as_slice(&self) -> &[f64] {
        &self.data
    }

    /// Mutable column-major backing storage.
    pub fn as_mut_slice(&mut self) -> &mut [f64] {
        &mut self.data
    }

    /// `true` if every element is finite.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.data.iter().all(|v| v.is_finite())
    }

    /// The transpose.
    #[must_use]
    pub fn transpose(&self) -> Self {
        let mut t = Self::zeros(self.ncols, self.nrows);
        for j in 0..self.ncols {
            for i in 0..self.nrows {
                t[(j, i)] = self[(i, j)];
            }
        }
        t
    }

    /// Matrix–vector product `A x`.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if `x.len() != ncols`.
    pub fn mul_vec(&self, x: &[f64]) -> Result<Vec<f64>> {
        if x.len() != self.ncols {
            return Err(StatError::DimensionMismatch {
                context: "vector length vs. matrix columns",
                expected: self.ncols,
                found: x.len(),
            });
        }
        let mut y = vec![0.0; self.nrows];
        for (j, &xj) in x.iter().enumerate() {
            for (yi, &aij) in y.iter_mut().zip(self.col(j)) {
                *yi += aij * xj;
            }
        }
        Ok(y)
    }

    /// Matrix product `A B`.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if `self.ncols() != other.nrows()`.
    pub fn matmul(&self, other: &Self) -> Result<Self> {
        if self.ncols != other.nrows {
            return Err(StatError::DimensionMismatch {
                context: "inner dimensions of matrix product",
                expected: self.ncols,
                found: other.nrows,
            });
        }
        let mut c = Self::zeros(self.nrows, other.ncols);
        for j in 0..other.ncols {
            for k in 0..self.ncols {
                let bkj = other[(k, j)];
                if bkj == 0.0 {
                    continue;
                }
                let a_k = &self.data[k * self.nrows..(k + 1) * self.nrows];
                let c_j = &mut c.data[j * self.nrows..(j + 1) * self.nrows];
                for (cij, &aik) in c_j.iter_mut().zip(a_k) {
                    *cij += aik * bkj;
                }
            }
        }
        Ok(c)
    }
}

impl std::ops::Index<(usize, usize)> for Matrix {
    type Output = f64;
    fn index(&self, (i, j): (usize, usize)) -> &f64 {
        assert!(i < self.nrows && j < self.ncols, "index ({i}, {j}) out of bounds");
        &self.data[j * self.nrows + i]
    }
}

impl std::ops::IndexMut<(usize, usize)> for Matrix {
    fn index_mut(&mut self, (i, j): (usize, usize)) -> &mut f64 {
        assert!(i < self.nrows && j < self.ncols, "index ({i}, {j}) out of bounds");
        &mut self.data[j * self.nrows + i]
    }
}

/// The power-of-two exponent `k` that brings a matrix whose largest entry is
/// `max_abs` near unit scale, or `None` when it is already within
/// `10^{±150}` (where squares and products of entries cannot overflow or
/// underflow) or is zero or not finite. Scaling by `2^k` is exact, so
/// decompositions of well-scaled matrices are unchanged bit for bit.
pub(crate) fn unit_scale_exponent(max_abs: f64) -> Option<i32> {
    if !(max_abs > 0.0 && max_abs.is_finite()) || (1e-150..=1e150).contains(&max_abs) {
        return None;
    }
    Some(-(max_abs.log2().floor() as i32))
}

/// `x · 2^k`, in two steps so that `2^k` itself never overflows.
pub(crate) fn scale_by_power_of_two(x: f64, k: i32) -> f64 {
    let half = k / 2;
    x * 2f64.powi(half) * 2f64.powi(k - half)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_columns_agree() {
        let a = Matrix::from_rows(&[[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]).unwrap();
        let b = Matrix::from_columns(&[[1.0, 3.0, 5.0], [2.0, 4.0, 6.0]]).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.col(1), &[2.0, 4.0, 6.0]);
        assert_eq!(a.row(2), vec![5.0, 6.0]);
    }

    #[test]
    fn ragged_input_is_rejected() {
        assert!(Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0]]).is_err());
        assert!(Matrix::from_columns(&[vec![1.0, 2.0], vec![3.0]]).is_err());
        assert!(Matrix::from_col_major(2, 2, vec![1.0; 3]).is_err());
    }

    #[test]
    fn products() {
        let a = Matrix::from_rows(&[[1.0, 2.0], [3.0, 4.0]]).unwrap();
        assert_eq!(a.mul_vec(&[1.0, 1.0]).unwrap(), vec![3.0, 7.0]);
        let p = a.matmul(&Matrix::identity(2)).unwrap();
        assert_eq!(p, a);
        let at = a.transpose();
        assert_eq!(at[(0, 1)], 3.0);
        assert!(a.mul_vec(&[1.0]).is_err());
    }
}
