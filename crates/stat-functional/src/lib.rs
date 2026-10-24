//! # stat-functional
//!
//! Functional data analysis (Ramsay & Silverman, *Functional Data Analysis*,
//! 2nd ed., Springer 2005): each observational unit is a curve `xᵢ(t)`
//! sampled at discrete times, possibly different for every curve.
//!
//! * [`basis`] — B-spline and Fourier basis systems: values and derivatives,
//!   inner-product (Gram) and roughness-penalty matrices.
//! * [`smooth`] — curves from discrete observations by penalized least
//!   squares in a basis, the smoothing parameter fixed or chosen by
//!   generalized cross-validation (GCV).
//! * [`fdata`] — a sample of curves as basis coefficients: evaluation,
//!   mean and pointwise standard deviation.
//! * [`fpca`] — functional principal components: eigenfunctions of the
//!   sample covariance operator, scores and reconstructions.
//! * [`register`] — registration (alignment of curves in time): landmark
//!   registration with monotone cubic warping functions, and continuous
//!   registration by the minimum-eigenvalue criterion of Ramsay & Li (1998).

pub mod basis;
pub mod fdata;
pub mod fpca;
mod linalg;
pub mod register;
pub mod smooth;

pub use basis::{BSplineBasis, Basis, FourierBasis, Roughness};
pub use fdata::FunctionalData;
pub use fpca::{Fpca, functional_pca};
pub use register::{
    ContinuousOptions, ContinuousRegistration, LandmarkKind, LandmarkSpec, Pchip, Warp, apply_warps, find_landmarks,
    landmark_warps, register_continuous,
};
pub use smooth::{CurveData, SmoothOptions, SmoothResult, Smoothing, smooth_curves};
pub use stat_core::error::{Result, StatError};
