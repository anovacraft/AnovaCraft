//! # stat-core
//!
//! Numerical foundation of the statistical engine. Everything here is implemented
//! from published mathematical definitions (references are given in each module):
//!
//! * [`matrix`] — a small column-major dense matrix used as the interchange type.
//! * [`summation`] — compensated sums and dot products (error-free transformations).
//! * [`qr`] — Householder QR with column pivoting, the basis of least squares.
//! * [`eigen`] — symmetric and symmetric-definite generalized eigenproblems (Jacobi).
//! * [`svd`](mod@svd) — singular value decomposition by one-sided Jacobi rotations.
//! * [`special`] — log-gamma, incomplete beta and gamma functions, `erfc`.
//! * [`box_cox`] — Box–Cox power transformation: profile likelihood, λ̂ and its confidence interval.
//! * [`distributions`] — normal, Student's t, chi-square and Fisher–Snedecor F.
//! * [`noncentral`] — noncentral t, chi-square and F (power calculations).
//! * [`studentized_range`] — the studentized range (multiple comparisons).
//! * [`discrete`] — binomial, Poisson and hypergeometric probabilities.
//! * [`catalog`] — every distribution above under one interface (density,
//!   probabilities, quantiles, moments, seeded random draws).
//! * [`quadrature`] — Gauss–Legendre integration rules.
//! * [`descriptive`] — moments, quantiles (Hyndman–Fan types 1–9), robust
//!   estimators, lag-1 autocorrelation.
//! * [`normality`] — Shapiro–Wilk and Anderson–Darling tests of normality.
//! * [`distribution_fit`] — maximum-likelihood fits of fourteen continuous
//!   families (with and without thresholds), Anderson–Darling goodness of fit
//!   with simulated p-values, and Johnson transformations to normality.
//! * [`ode`] — Dormand–Prince 5(4) adaptive Runge–Kutta solver with dense
//!   output for initial value problems (models defined by differential equations).
//! * [`rng`] — a seedable SplitMix64 generator (designs, random samples,
//!   normal and triangular draws for simulation).
//! * [`optimize`] — Nelder–Mead inside a box, Latin hypercube starts, and
//!   Levenberg–Marquardt nonlinear least squares.
//! * [`correlation`] — Pearson and Spearman correlation with inference.
//! * [`error`] — the [`StatError`] type shared by the workspace.

pub mod box_cox;
pub mod catalog;
pub mod correlation;
pub mod descriptive;
pub mod discrete;
pub mod distribution_fit;
pub mod distributions;
pub mod eigen;
pub mod error;
pub mod interrupt;
pub mod matrix;
pub mod noncentral;
pub mod normality;
pub mod ode;
pub mod optimize;
pub mod qr;
pub mod quadrature;
pub mod ranking;
pub mod rng;
pub mod special;
pub mod studentized_range;
pub mod summation;
pub mod svd;
pub mod text;

pub use distributions::{ChiSquared, ContinuousDistribution, FisherSnedecor, Normal, StudentsT};
pub use error::{Result, StatError};
pub use matrix::Matrix;
pub use noncentral::{NoncentralChiSquared, NoncentralF, NoncentralT};
pub use qr::QrDecomposition;
pub use studentized_range::StudentizedRange;
pub use svd::{Svd, svd};
