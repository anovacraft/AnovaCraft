//! Best-subsets regression: the best models of each size among all subsets
//! of the candidate effects (Furnival & Wilson 1974, *Technometrics* 16;
//! Miller, *Subset Selection in Regression*, 2nd ed., 2002, ch. 3).
//!
//! Effects are blocks of design columns, as in [`crate::stepwise`]; size
//! counts effects, and every model has an intercept. Among models of one
//! size, R², adjusted R², Mallows' Cp, AICc and BIC all order models as the
//! residual sum of squares does, so the search keeps the `m` smallest RSS of
//! each size and computes the criteria afterwards:
//!
//! * `R² = 1 − RSS/TSS`, `R²_adj = 1 − (RSS/(n − p))/(TSS/(n − 1))`, with `p`
//!   coefficients (intercept included);
//! * `Cp = RSS/σ̂² − n + 2p` (Mallows 1973), `σ̂²` the mean squared error of
//!   the model with every candidate;
//! * `AICc` and `BIC` exactly as [`crate::fit_linear_model`] reports them
//!   (`AIC = n ln(2π RSS/n) + n + 2(p + 1)`, `AICc = AIC + 2(p+1)(p+2)/(n − p − 2)`,
//!   `BIC = n ln(2π RSS/n) + n + (p + 1) ln n`).
//!
//! **Branch and bound.** The search walks the tree of include/exclude
//! decisions effect by effect. Every model below a node contains the effects
//! included so far (`I`) and lies inside `I ∪ U` (`U` the undecided effects),
//! so its RSS is at least `RSS(I ∪ U)`: adding columns never increases the
//! residual sum of squares. A node is pruned when, for every size it can
//! still reach, the list of best models of that size is full and its worst
//! RSS is at most that bound. This is the bound of Furnival & Wilson's
//! "leaps and bounds"; the result is the same as exhaustive enumeration.
//!
//! Forced effects are in every model. With heredity, a model is admitted only
//! when each interaction or power has all (strong) or one (weak) of its
//! parent effects. Models that are not estimable (aliased columns, or no
//! residual degrees of freedom) are skipped.

use crate::ols::{OlsOptions, fit_linear_model};
use crate::stepwise::Heredity;
use stat_core::qr::norm2;
use stat_core::summation::{mean, sum_iter};
use stat_core::{Matrix, QrDecomposition, Result, StatError};
use std::ops::Range;

/// Options for [`best_subsets`].
#[derive(Debug, Clone, PartialEq)]
pub struct BestSubsetsOptions {
    /// Models kept for each size.
    pub models_per_size: usize,
    /// Largest number of effects in a model (`None`: all candidates).
    pub max_size: Option<usize>,
    /// Effects in every model.
    pub forced: Vec<usize>,
    /// Parent effects an interaction or power needs.
    pub heredity: Heredity,
}

impl Default for BestSubsetsOptions {
    fn default() -> Self {
        Self { models_per_size: 3, max_size: None, forced: Vec::new(), heredity: Heredity::Strong }
    }
}

/// One model found by [`best_subsets`].
#[derive(Debug, Clone, PartialEq)]
pub struct SubsetModel {
    /// Effects in the model, in index order.
    pub effects: Vec<usize>,
    /// Number of coefficients (intercept included).
    pub parameters: usize,
    /// Residual sum of squares.
    pub rss: f64,
    /// R².
    pub r_squared: f64,
    /// Adjusted R².
    pub adj_r_squared: f64,
    /// Mallows' Cp (NaN when the model with every candidate leaves no error df).
    pub cp: f64,
    /// Corrected AIC (NaN when undefined, including a constant response).
    pub aicc: f64,
    /// Bayesian information criterion (NaN when RSS = 0 or the response is
    /// constant).
    pub bic: f64,
}

impl SubsetModel {
    /// Number of effects.
    #[must_use]
    pub fn size(&self) -> usize {
        self.effects.len()
    }
}

/// A criterion for choosing among sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubsetCriterion {
    /// Largest R² (always the largest model).
    RSquared,
    /// Largest adjusted R².
    AdjustedRSquared,
    /// Smallest Mallows' Cp.
    Cp,
    /// Smallest AICc.
    Aicc,
    /// Smallest BIC.
    Bic,
}

impl SubsetCriterion {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RSquared => "R²",
            Self::AdjustedRSquared => "Adjusted R²",
            Self::Cp => "Mallows' Cp",
            Self::Aicc => "AICc",
            Self::Bic => "BIC",
        }
    }

    /// The criterion's value for `m`, oriented so that smaller is better.
    #[must_use]
    pub fn score(self, m: &SubsetModel) -> f64 {
        match self {
            Self::RSquared => -m.r_squared,
            Self::AdjustedRSquared => -m.adj_r_squared,
            Self::Cp => m.cp,
            Self::Aicc => m.aicc,
            Self::Bic => m.bic,
        }
    }
}

/// Result of [`best_subsets`].
#[derive(Debug, Clone, PartialEq)]
pub struct BestSubsetsResult {
    /// The best models of each size: by size, then by RSS.
    pub models: Vec<SubsetModel>,
    /// Number of observations.
    pub n: usize,
    /// Total sum of squares about the mean.
    pub tss: f64,
    /// `σ̂²` used for Cp (NaN when not available).
    pub sigma_squared: f64,
    /// Least-squares fits evaluated by the search.
    pub fits_evaluated: usize,
}

impl BestSubsetsResult {
    /// The model with the best value of `criterion` (ties: the smaller model).
    #[must_use]
    pub fn best_by(&self, criterion: SubsetCriterion) -> Option<&SubsetModel> {
        self.models
            .iter()
            .filter(|m| !criterion.score(m).is_nan())
            .min_by(|a, b| criterion.score(a).total_cmp(&criterion.score(b)).then(a.size().cmp(&b.size())))
    }
}

struct Search<'a> {
    x: &'a Matrix,
    y: &'a [f64],
    effects: &'a [Range<usize>],
    parents: &'a [Vec<usize>],
    heredity: Heredity,
    per_size: usize,
    min_size: usize,
    max_size: usize,
    /// Best `(rss, effects)` of each size, ascending.
    kept: Vec<Vec<(f64, Vec<usize>)>>,
    fits: usize,
}

impl Search<'_> {
    /// RSS of the model with `set` and an intercept; `None` when it is not
    /// estimable.
    fn rss(&mut self, set: &[usize]) -> Result<Option<f64>> {
        self.fits += 1;
        let n = self.y.len();
        let y_mean = mean(self.y).unwrap_or(0.0);
        let mut centered_y: Vec<f64> = self.y.iter().map(|v| v - y_mean).collect();
        let cols: Vec<usize> = set.iter().flat_map(|&e| self.effects[e].clone()).collect();
        if cols.is_empty() {
            return Ok(Some(sum_iter(centered_y.iter().map(|v| v * v))));
        }
        if cols.len() + 1 >= n {
            return Ok(None);
        }
        let mut design = Matrix::zeros(n, cols.len());
        for (k, &j) in cols.iter().enumerate() {
            let c = self.x.col(j);
            let m = mean(c).unwrap_or(0.0);
            let out = design.col_mut(k);
            for (o, v) in out.iter_mut().zip(c) {
                *o = v - m;
            }
            let norm = norm2(design.col(k));
            if norm == 0.0 {
                return Ok(None);
            }
            design.col_mut(k).iter_mut().for_each(|v| *v /= norm);
        }
        let qr = QrDecomposition::new(design)?;
        if qr.rank(qr.default_rank_tolerance()) < cols.len() {
            return Ok(None);
        }
        qr.apply_qt(&mut centered_y)?;
        Ok(Some(sum_iter(centered_y[cols.len()..].iter().map(|v| v * v))))
    }

    fn admissible(&self, set: &[usize]) -> bool {
        set.iter().all(|&e| {
            let p = &self.parents[e];
            match self.heredity {
                Heredity::None => true,
                _ if p.is_empty() => true,
                Heredity::Strong => p.iter().all(|q| set.contains(q)),
                Heredity::Weak => p.iter().any(|q| set.contains(q)),
            }
        })
    }

    fn worst(&self, size: usize) -> Option<f64> {
        let list = &self.kept[size];
        (list.len() >= self.per_size).then(|| list.last().map_or(f64::INFINITY, |m| m.0))
    }

    fn record(&mut self, rss: f64, set: Vec<usize>) {
        let size = set.len();
        let list = &mut self.kept[size];
        let pos = list.partition_point(|m| m.0 <= rss);
        if pos < self.per_size {
            list.insert(pos, (rss, set));
            list.truncate(self.per_size);
        }
    }

    /// Explore every model `I ⊆ S ⊆ I ∪ U`.
    fn explore(&mut self, included: &mut Vec<usize>, undecided: &[usize]) -> Result<()> {
        if included.len() > self.max_size {
            return Ok(());
        }
        let mut all = included.clone();
        all.extend_from_slice(undecided);
        all.sort_unstable();
        let reach = (included.len().max(self.min_size))..=(all.len().min(self.max_size));
        if reach.is_empty() {
            return Ok(());
        }
        let bound = self.rss(&all)?;
        if undecided.is_empty() {
            if let Some(rss) = bound {
                if self.admissible(&all) {
                    self.record(rss, all);
                }
            }
            return Ok(());
        }
        if let Some(b) = bound {
            if reach.clone().all(|s| self.worst(s).is_some_and(|w| w <= b)) {
                return Ok(());
            }
        }
        let (&v, rest) = undecided.split_first().expect("non-empty");
        included.push(v);
        self.explore(included, rest)?;
        included.pop();
        self.explore(included, rest)
    }
}

/// The best `options.models_per_size` models of each size for `y` from the
/// columns of `x`, where effect `i` occupies columns `effects[i]` and has
/// parent effects `parents[i]` (pass empty lists for no hierarchy).
///
/// # Errors
/// [`StatError::InvalidArgument`] for mismatched sizes, overlapping or
/// out-of-range effect columns, no effects, `parents` without one entry per
/// effect, forced effects out of range, `models_per_size = 0`, or a maximum
/// size below the number of forced effects; numerical failures of a fit
/// other than rank deficiency.
pub fn best_subsets(
    x: &Matrix,
    y: &[f64],
    effects: &[Range<usize>],
    parents: &[Vec<usize>],
    options: &BestSubsetsOptions,
) -> Result<BestSubsetsResult> {
    let n = y.len();
    if x.nrows() != n {
        return Err(invalid("the design and the response have different numbers of rows"));
    }
    if effects.is_empty() {
        return Err(invalid("give at least one candidate effect"));
    }
    if parents.len() != effects.len() || parents.iter().flatten().any(|&p| p >= effects.len()) {
        return Err(invalid("parents must list existing effects, one list per effect"));
    }
    let mut used = vec![false; x.ncols()];
    for e in effects {
        if e.is_empty() || e.end > x.ncols() {
            return Err(invalid("effect columns are out of range"));
        }
        for j in e.clone() {
            if std::mem::replace(&mut used[j], true) {
                return Err(invalid("effects may not share columns"));
            }
        }
    }
    if !x.is_finite() || y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "best-subsets data" });
    }
    if options.models_per_size == 0 {
        return Err(invalid("keep at least one model of each size"));
    }
    let mut forced = options.forced.clone();
    forced.sort_unstable();
    forced.dedup();
    if forced.iter().any(|&f| f >= effects.len()) {
        return Err(invalid("a forced effect does not exist"));
    }
    let max_size = options.max_size.unwrap_or(effects.len()).min(effects.len());
    if max_size < forced.len() {
        return Err(invalid("the largest model size is below the number of forced effects"));
    }
    if n < 3 {
        return Err(StatError::InsufficientData { required: 3, found: n });
    }
    let mut search = Search {
        x,
        y,
        effects,
        parents,
        heredity: options.heredity,
        per_size: options.models_per_size,
        min_size: forced.len(),
        max_size,
        kept: vec![Vec::new(); effects.len() + 1],
        fits: 0,
    };
    let undecided: Vec<usize> = (0..effects.len()).filter(|e| !forced.contains(e)).collect();
    let mut included = forced.clone();
    search.explore(&mut included, &undecided)?;

    // σ̂² from the model with every candidate, for Cp.
    let all: Vec<usize> = (0..effects.len()).collect();
    let full_columns: usize = effects.iter().map(ExactSizeIterator::len).sum();
    let sigma_squared = match search.rss(&all)? {
        Some(rss) if n > full_columns + 1 => rss / (n - full_columns - 1) as f64,
        _ => f64::NAN,
    };
    let y_mean = mean(y).unwrap_or(0.0);
    let tss = sum_iter(y.iter().map(|v| (v - y_mean) * (v - y_mean)));
    let fits_evaluated = search.fits;
    let mut models = Vec::new();
    for list in &search.kept {
        for (_, set) in list {
            models.push(summarize(x, y, effects, set, tss, sigma_squared)?);
        }
    }
    models.sort_by(|a, b| a.size().cmp(&b.size()).then(a.rss.total_cmp(&b.rss)));
    Ok(BestSubsetsResult { models, n, tss, sigma_squared, fits_evaluated })
}

/// Refit a chosen model with [`fit_linear_model`] so that its statistics are
/// exactly those its linear-model report shows.
fn summarize(
    x: &Matrix,
    y: &[f64],
    effects: &[Range<usize>],
    set: &[usize],
    tss: f64,
    sigma_squared: f64,
) -> Result<SubsetModel> {
    let n = y.len() as f64;
    let cols: Vec<&[f64]> = set.iter().flat_map(|&e| effects[e].clone()).map(|j| x.col(j)).collect();
    let p = cols.len() + 1;
    let (rss, mut aicc, mut bic) = if cols.is_empty() {
        let k = 2.0;
        let ll = -0.5 * n * ((2.0 * std::f64::consts::PI).ln() + (tss / n).ln() + 1.0);
        let aic = -2.0 * ll + 2.0 * k;
        let aicc = if n - k - 1.0 > 0.0 { aic + 2.0 * k * (k + 1.0) / (n - k - 1.0) } else { f64::NAN };
        (tss, aicc, -2.0 * ll + k * n.ln())
    } else {
        let fit = fit_linear_model(&Matrix::from_columns(&cols)?, y, &OlsOptions::default())?;
        let s = fit.summary();
        (fit.anova().error.sum_of_squares, s.aicc.unwrap_or(f64::NAN), s.bic.unwrap_or(f64::NAN))
    };
    if tss.is_nan() || tss <= 0.0 {
        // A constant response: the likelihood is unbounded, so neither
        // criterion is defined (rather than −∞ for the intercept-only model).
        (aicc, bic) = (f64::NAN, f64::NAN);
    }
    let pf = p as f64;
    let r_squared = if tss > 0.0 { 1.0 - rss / tss } else { f64::NAN };
    let adj_r_squared = if tss > 0.0 { 1.0 - (rss / (n - pf)) / (tss / (n - 1.0)) } else { f64::NAN };
    Ok(SubsetModel {
        effects: set.to_vec(),
        parameters: p,
        rss,
        r_squared,
        adj_r_squared,
        cp: rss / sigma_squared - n + 2.0 * pf,
        aicc,
        bic,
    })
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hald's cement data (Draper & Smith, *Applied Regression Analysis*,
    /// 3rd ed., §15.1 and appendix 15A).
    fn hald() -> (Matrix, Vec<f64>) {
        let x1 = [7.0, 1.0, 11.0, 11.0, 7.0, 11.0, 3.0, 1.0, 2.0, 21.0, 1.0, 11.0, 10.0];
        let x2 = [26.0, 29.0, 56.0, 31.0, 52.0, 55.0, 71.0, 31.0, 54.0, 47.0, 40.0, 66.0, 68.0];
        let x3 = [6.0, 15.0, 8.0, 8.0, 6.0, 9.0, 17.0, 22.0, 18.0, 4.0, 23.0, 9.0, 8.0];
        let x4 = [60.0, 52.0, 20.0, 47.0, 33.0, 22.0, 6.0, 44.0, 22.0, 26.0, 34.0, 12.0, 12.0];
        let y = vec![78.5, 74.3, 104.3, 87.6, 95.9, 109.2, 102.7, 72.5, 93.1, 115.9, 83.8, 113.3, 109.4];
        (Matrix::from_columns(&[&x1[..], &x2[..], &x3[..], &x4[..]]).unwrap(), y)
    }

    fn singles(k: usize) -> Vec<Range<usize>> {
        (0..k).map(|j| j..j + 1).collect()
    }

    /// Draper & Smith's table of all subsets: the best two of each size by R²
    /// (in %) and Cp.
    #[test]
    fn hald_subsets_match_the_textbook() {
        let (x, y) = hald();
        let o = BestSubsetsOptions { models_per_size: 2, heredity: Heredity::None, ..Default::default() };
        let r = best_subsets(&x, &y, &singles(4), &vec![vec![]; 4], &o).unwrap();
        let table: [(&[usize], f64, f64); 7] = [
            (&[3], 67.45, 138.7),
            (&[1], 66.63, 142.5),
            (&[0, 1], 97.87, 2.68),
            (&[0, 3], 97.25, 5.50),
            (&[0, 1, 3], 98.234, 3.02),
            (&[0, 1, 2], 98.228, 3.04),
            (&[0, 1, 2, 3], 98.237, 5.00),
        ];
        // Size 0 (the mean) comes first.
        assert_eq!(r.models[0].effects, Vec::<usize>::new());
        assert_eq!(r.models.len(), 8);
        for (m, (effects, r2, cp)) in r.models[1..].iter().zip(table) {
            assert_eq!(m.effects, effects);
            assert!((100.0 * m.r_squared - r2).abs() < 6e-3, "{effects:?}: R² {}", m.r_squared);
            assert!((m.cp - cp).abs() < 0.06, "{effects:?}: Cp {}", m.cp);
        }
        assert_eq!(r.best_by(SubsetCriterion::Cp).unwrap().effects, [0, 1]);
        assert_eq!(r.best_by(SubsetCriterion::RSquared).unwrap().effects, [0, 1, 2, 3]);
        assert_eq!(r.best_by(SubsetCriterion::AdjustedRSquared).unwrap().effects, [0, 1, 3]);
    }

    /// The search agrees with fitting every subset, including the statistics
    /// a linear-model report shows, and prunes most fits away.
    #[test]
    fn branch_and_bound_matches_exhaustive_enumeration() {
        let n = 40;
        let k = 9;
        let cols: Vec<Vec<f64>> = (0..k)
            .map(|j| (0..n).map(|i| (((i * 31 + j * 17) * (j + 3)) % 23) as f64 + 0.1 * j as f64).collect())
            .collect();
        let y: Vec<f64> =
            (0..n).map(|i| 2.0 * cols[0][i] - cols[3][i] + 0.5 * cols[6][i] + ((i * 7) % 5) as f64).collect();
        let x = Matrix::from_columns(&cols).unwrap();
        let o = BestSubsetsOptions { models_per_size: 3, heredity: Heredity::None, ..Default::default() };
        let r = best_subsets(&x, &y, &singles(k), &vec![vec![]; k], &o).unwrap();
        // Exhaustive: every subset's RSS by fit_linear_model.
        let mut by_size: Vec<Vec<(f64, Vec<usize>)>> = vec![Vec::new(); k + 1];
        for mask in 0u32..(1 << k) {
            let set: Vec<usize> = (0..k).filter(|&j| mask >> j & 1 == 1).collect();
            let m = summarize(&x, &y, &singles(k), &set, r.tss, r.sigma_squared).unwrap();
            by_size[set.len()].push((m.rss, set));
        }
        for (size, list) in by_size.iter_mut().enumerate() {
            list.sort_by(|a, b| a.0.total_cmp(&b.0));
            let found: Vec<&SubsetModel> = r.models.iter().filter(|m| m.size() == size).collect();
            assert_eq!(found.len(), list.len().min(3));
            for (m, (rss, set)) in found.iter().zip(list.iter()) {
                assert_eq!(&m.effects, set, "size {size}");
                assert!((m.rss - rss).abs() <= 1e-9 * rss.max(1.0));
            }
        }
        let fit = fit_linear_model(&Matrix::from_columns(&[&cols[0], &cols[3]]).unwrap(), &y, &OlsOptions::default())
            .unwrap();
        let m = r.models.iter().find(|m| m.effects == [0, 3]);
        if let Some(m) = m {
            assert!((m.aicc - fit.summary().aicc.unwrap()).abs() < 1e-9);
            assert!((m.bic - fit.summary().bic.unwrap()).abs() < 1e-9);
            assert!((m.adj_r_squared - fit.summary().adj_r_squared).abs() < 1e-12);
        }
        assert!(r.fits_evaluated < 1 << k, "{} fits", r.fits_evaluated);
    }

    #[test]
    fn forced_effects_heredity_and_errors() {
        let (x1, y) = hald();
        let interaction: Vec<f64> = (0..13).map(|i| x1.col(0)[i] * x1.col(1)[i]).collect();
        let x = Matrix::from_columns(&[x1.col(0), x1.col(1), x1.col(2), x1.col(3), &interaction[..]]).unwrap();
        let parents = vec![vec![], vec![], vec![], vec![], vec![0, 1]];
        let o = BestSubsetsOptions { models_per_size: 5, heredity: Heredity::Strong, ..Default::default() };
        let r = best_subsets(&x, &y, &singles(5), &parents, &o).unwrap();
        for m in &r.models {
            if m.effects.contains(&4) {
                assert!(m.effects.contains(&0) && m.effects.contains(&1), "{:?}", m.effects);
            }
        }
        let weak = BestSubsetsOptions { heredity: Heredity::Weak, ..o.clone() };
        let r = best_subsets(&x, &y, &singles(5), &parents, &weak).unwrap();
        assert!(r.models.iter().any(|m| m.effects.contains(&4) && !m.effects.contains(&1)));
        let forced = BestSubsetsOptions { forced: vec![2], max_size: Some(2), ..o.clone() };
        let r = best_subsets(&x, &y, &singles(5), &parents, &forced).unwrap();
        assert!(r.models.iter().all(|m| m.effects.contains(&2) && m.size() <= 2));
        assert_eq!(r.models[0].effects, [2]);
        let none = vec![vec![]; 5];
        assert!(best_subsets(&x, &y, &[], &[], &o).is_err());
        assert!(best_subsets(&x, &y, &singles(5), &none[..4], &o).is_err());
        assert!(best_subsets(&x, &y, &[0..2, 1..3], &none[..2], &o).is_err());
        let bad = BestSubsetsOptions { forced: vec![0, 1], max_size: Some(1), ..o.clone() };
        assert!(best_subsets(&x, &y, &singles(5), &none, &bad).is_err());
        let zero = BestSubsetsOptions { models_per_size: 0, ..o };
        assert!(best_subsets(&x, &y, &singles(5), &none, &zero).is_err());
    }

    /// A constant response has no defined AICc or BIC (its likelihood is
    /// unbounded); every size reports NaN instead of −∞ for the mean model.
    #[test]
    fn constant_response_has_undefined_criteria() {
        let (x, _) = hald();
        let y = vec![5.0; 13];
        let o = BestSubsetsOptions { models_per_size: 1, heredity: Heredity::None, ..Default::default() };
        let r = best_subsets(&x, &y, &singles(4), &vec![vec![]; 4], &o).unwrap();
        for m in &r.models {
            assert!(m.aicc.is_nan() && m.bic.is_nan(), "{:?}: {} {}", m.effects, m.aicc, m.bic);
        }
    }
}
