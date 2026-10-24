//! Error type shared by every crate in the engine.
//!
//! The variants are hand-written rather than derived with `thiserror` because the
//! build environment this was first written in had no access to crates.io. The
//! shape (one enum, `Display` + `std::error::Error`) is exactly what `thiserror`
//! would generate, so swapping it in later is a mechanical change.

use std::fmt;

/// Every failure mode a statistical routine in this workspace can report.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StatError {
    /// Two inputs that must agree in size do not.
    DimensionMismatch {
        /// What was being compared, e.g. `"response length vs. design rows"`.
        context: &'static str,
        /// The size that was required.
        expected: usize,
        /// The size that was supplied.
        found: usize,
    },
    /// Too few observations to estimate the requested quantity
    /// (for example, OLS needs more observations than parameters to estimate σ²).
    InsufficientData {
        /// Minimum number of observations required.
        required: usize,
        /// Number of observations supplied.
        found: usize,
    },
    /// An input contained NaN or ±∞.
    NonFiniteInput {
        /// Which input was affected.
        context: &'static str,
    },
    /// The design matrix does not have full column rank, so the parameters are not
    /// identifiable. `rank` is the numerical rank detected by the pivoted QR.
    RankDeficient {
        /// Detected numerical rank.
        rank: usize,
        /// Number of columns (parameters) requested.
        columns: usize,
    },
    /// An argument was outside its valid domain (e.g. a probability outside (0, 1)).
    InvalidArgument {
        /// Human-readable description of the violated requirement.
        message: String,
    },
    /// An iterative algorithm failed to reach its tolerance.
    ConvergenceFailure {
        /// The routine that failed.
        routine: &'static str,
        /// Iterations performed before giving up.
        iterations: usize,
    },
    /// The computation was stopped by its caller (see [`crate::interrupt`]).
    Cancelled,
}

impl fmt::Display for StatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionMismatch { context, expected, found } => {
                write!(f, "dimension mismatch ({context}): expected {expected}, found {found}")
            }
            Self::InsufficientData { required, found } => {
                write!(f, "insufficient data: at least {required} observations required, found {found}")
            }
            Self::NonFiniteInput { context } => {
                write!(f, "non-finite value (NaN or infinity) in {context}")
            }
            Self::RankDeficient { rank, columns } => {
                write!(f, "design matrix is rank deficient: numerical rank {rank} < {columns} columns")
            }
            // The message says what was wrong in plain words; a category
            // prefix only adds noise where people read it.
            Self::InvalidArgument { message } => write!(f, "{message}"),
            Self::ConvergenceFailure { routine, iterations } => {
                write!(f, "{routine} failed to converge after {iterations} iterations")
            }
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for StatError {}

/// Convenience alias used throughout the workspace.
pub type Result<T> = std::result::Result<T, StatError>;

/// Build an [`StatError::InvalidArgument`] from anything printable.
pub(crate) fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_informative() {
        let e = StatError::RankDeficient { rank: 2, columns: 3 };
        assert_eq!(e.to_string(), "design matrix is rank deficient: numerical rank 2 < 3 columns");
        let e = StatError::DimensionMismatch { context: "y", expected: 4, found: 3 };
        assert!(e.to_string().contains("expected 4, found 3"));
        // Shown to people as is: no "invalid argument:" category prefix.
        let e = StatError::InvalidArgument { message: "give at least one group".into() };
        assert_eq!(e.to_string(), "give at least one group");
    }

    #[test]
    fn is_std_error() {
        fn takes_error(_: &dyn std::error::Error) {}
        takes_error(&StatError::NonFiniteInput { context: "x" });
    }
}
