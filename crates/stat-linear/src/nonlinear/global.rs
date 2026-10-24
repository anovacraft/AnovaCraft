//! Global (shared-parameter) fitting: several data sets fitted together, each
//! with its own copy of some parameters and one common value of the others
//! (Motulsky & Christopoulos, *Fitting Models to Biological Data Using Linear
//! and Nonlinear Regression*, Oxford University Press 2004, ch. 37–38).
//!
//! # Model
//! Data set `g` has `n_g` observations and a model `f_g(x; φ_g)` with the
//! same `K` parameters. The global parameter vector `θ` holds each shared
//! parameter once and each unshared one once per data set; `φ_g = M_g θ`
//! picks data set `g`'s values (`map[g][k]` is the index in `θ` of its
//! `k`-th parameter). The data sets are stacked, so the fit minimises
//! `Σ_g Σ_i (y_gi − f_g(x_gi; M_g θ))²` and every statistic of
//! [`fit_nonlinear`](super::fit_nonlinear) — standard errors, profile
//! intervals, bands — refers to `θ`, with one residual variance for all
//! data sets. The Jacobian is `∂f_g/∂θ = (∂f_g/∂φ_g) M_g`: analytic when
//! every data set's model has analytic derivatives.
//!
//! Sharing every parameter fits one curve to all the data; sharing none fits
//! the data sets separately but with a pooled variance (the same estimates as
//! separate fits).

use super::ModelFunction;
use stat_core::{Matrix, Result, StatError};

/// Several data sets' models over one global parameter vector.
pub struct GlobalModel<'a> {
    parts: Vec<Box<dyn ModelFunction + 'a>>,
    map: Vec<Vec<usize>>,
    n_global: usize,
    offsets: Vec<usize>,
    local: Vec<Vec<f64>>,
}

impl std::fmt::Debug for GlobalModel<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalModel").field("map", &self.map).field("n_global", &self.n_global).finish_non_exhaustive()
    }
}

/// The parameter a global position belongs to, and its data set (`None` when shared).
pub type Owner = (usize, Option<usize>);

/// The global parameter layout for `n_groups` data sets of a model with
/// `shared.len()` parameters: `(map, owner)` where `map[g][k]` is the global
/// index of data set `g`'s parameter `k`, and `owner[i]` is
/// `(k, None)` for a shared global parameter `i` or `(k, Some(g))` for data
/// set `g`'s copy. Global order: for each model parameter in turn, its one
/// shared value or its copies in data-set order.
#[must_use]
pub fn layout(shared: &[bool], n_groups: usize) -> (Vec<Vec<usize>>, Vec<Owner>) {
    let mut owner = Vec::new();
    let mut map = vec![vec![0; shared.len()]; n_groups];
    for (k, &sh) in shared.iter().enumerate() {
        if sh {
            for m in &mut map {
                m[k] = owner.len();
            }
            owner.push((k, None));
        } else {
            for (g, m) in map.iter_mut().enumerate() {
                m[k] = owner.len();
                owner.push((k, Some(g)));
            }
        }
    }
    (map, owner)
}

impl<'a> GlobalModel<'a> {
    /// Data sets `parts` (observations stacked in this order), with
    /// `map[g][k]` the global index of data set `g`'s parameter `k`.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if `map` has a row per data set of
    /// unequal lengths, or an index outside `0..n_global`;
    /// [`StatError::InvalidArgument`] with no data sets.
    pub fn new(parts: Vec<Box<dyn ModelFunction + 'a>>, map: Vec<Vec<usize>>, n_global: usize) -> Result<Self> {
        if parts.is_empty() {
            return Err(StatError::InvalidArgument { message: "a global fit needs at least one data set".into() });
        }
        if map.len() != parts.len() {
            return Err(StatError::DimensionMismatch {
                context: "parameter map vs. data sets",
                expected: parts.len(),
                found: map.len(),
            });
        }
        let k = map[0].len();
        if map.iter().any(|m| m.len() != k || m.iter().any(|&i| i >= n_global)) {
            return Err(StatError::DimensionMismatch { context: "parameter map", expected: n_global, found: k });
        }
        let mut offsets = Vec::with_capacity(parts.len() + 1);
        let mut total = 0;
        for p in &parts {
            offsets.push(total);
            total += p.n_observations();
        }
        offsets.push(total);
        let local = vec![vec![0.0; k]; parts.len()];
        Ok(Self { parts, map, n_global, offsets, local })
    }

    /// Number of global parameters.
    #[must_use]
    pub fn n_global(&self) -> usize {
        self.n_global
    }

    fn fill_local(&mut self, theta: &[f64]) {
        for (loc, m) in self.local.iter_mut().zip(&self.map) {
            for (v, &i) in loc.iter_mut().zip(m) {
                *v = theta[i];
            }
        }
    }
}

impl ModelFunction for GlobalModel<'_> {
    fn n_observations(&self) -> usize {
        self.offsets[self.parts.len()]
    }

    fn values(&mut self, theta: &[f64], out: &mut [f64]) -> bool {
        self.fill_local(theta);
        for g in 0..self.parts.len() {
            let (a, b) = (self.offsets[g], self.offsets[g + 1]);
            if !self.parts[g].values(&self.local[g], &mut out[a..b]) {
                return false;
            }
        }
        true
    }

    fn derivatives(&mut self, theta: &[f64], out: &mut Matrix) -> Option<bool> {
        self.fill_local(theta);
        let k = self.map[0].len();
        for v in out.as_mut_slice() {
            *v = 0.0;
        }
        for g in 0..self.parts.len() {
            let (a, b) = (self.offsets[g], self.offsets[g + 1]);
            let mut jac = Matrix::zeros(b - a, k);
            if !self.parts[g].derivatives(&self.local[g], &mut jac)? {
                return Some(false);
            }
            for (j, &i) in self.map[g].iter().enumerate() {
                for r in 0..(b - a) {
                    out[(a + r, i)] += jac[(r, j)];
                }
            }
        }
        Some(true)
    }
}
