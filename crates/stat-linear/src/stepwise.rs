//! Stepwise selection of model effects (Efroymson 1960; Draper & Smith,
//! *Applied Regression Analysis*, 3rd ed., ch. 15; Venables & Ripley, *Modern
//! Applied Statistics with S*, §6.8).
//!
//! Effects are blocks of design columns (all of a categorical effect's columns
//! enter or leave together); the intercept is always in the model.
//!
//! * **p-value criterion** — an effect enters when its partial F-test against
//!   the current model has `p < α_enter` (the smallest such p first) and leaves
//!   when its test within the current model has `p > α_remove` (the largest
//!   first). `F = [(RSS_without − RSS_with)/df] / [RSS_with/(n − p_with)]`.
//! * **AIC / BIC** — `n·ln(RSS/n) + k·p`, with `p` the number of coefficients
//!   (intercept included) and `k = 2` (AIC) or `ln n` (BIC), as in R's
//!   `extractAIC`; each step takes the move that lowers the criterion most.
//!
//! Directions: **forward** from the empty model, **backward** from the full
//! model, **mixed** from the empty model, checking after every entry whether an
//! effect should leave (p-values) or considering entries and removals together
//! (AIC/BIC). Selection is a heuristic search; the final model's p-values do not
//! account for the selection.
//!
//! Only estimable models are visited: a move is skipped when the model it leads
//! to is rank deficient (the effect is aliased with effects already in it) or
//! leaves no residual degrees of freedom. So forward and mixed searches work
//! with more candidates than observations, or with aliased candidates. A
//! backward search needs a starting model: when the full model is not
//! estimable, it starts from the largest estimable model built by adding the
//! effects in order (list main effects first), and reports the rest as set aside.
//!
//! **Heredity** ([`stepwise_selection_with_parents`]): with *strong* heredity
//! an interaction or power may enter only when all its parents are in the
//! model, and an effect may not leave while an effect built on it remains;
//! with *weak* heredity one parent in the model suffices (Chipman 1996; see
//! [`crate::hierarchy`]). Backward searches remove higher-order effects before
//! the effects they are built from.

use crate::ols::{OlsOptions, fit_linear_model};
use stat_core::distributions::{ContinuousDistribution, FisherSnedecor};
use stat_core::summation::{dot, mean};
use stat_core::{Matrix, Result, StatError};
use std::ops::Range;

/// Search direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepDirection {
    /// Add effects one at a time to the empty model.
    Forward,
    /// Remove effects one at a time from the full model.
    Backward,
    /// Start empty; effects may enter and leave.
    Mixed,
}

impl StepDirection {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Forward => "Forward",
            Self::Backward => "Backward",
            Self::Mixed => "Mixed (forward and backward)",
        }
    }
}

/// Selection criterion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepCriterion {
    /// Partial F-test p-values.
    PValue {
        /// Enter when `p < enter`.
        enter: f64,
        /// Leave when `p > remove` (at least `enter`).
        remove: f64,
    },
    /// Akaike information criterion.
    Aic,
    /// Bayesian (Schwarz) information criterion.
    Bic,
}

/// Which parent effects an interaction or power needs in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Heredity {
    /// Every parent (a hierarchical model at every step).
    #[default]
    Strong,
    /// At least one parent.
    Weak,
    /// No restriction.
    None,
}

impl Heredity {
    /// Every option.
    pub const ALL: [Heredity; 3] = [Heredity::Strong, Heredity::Weak, Heredity::None];

    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Strong => "Strong (all parent effects)",
            Self::Weak => "Weak (at least one parent effect)",
            Self::None => "None",
        }
    }

    /// Name used in saved files.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Weak => "weak",
            Self::None => "none",
        }
    }

    /// The heredity with this [`Heredity::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.key() == key)
    }
}

/// Options for [`stepwise_selection`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepwiseOptions {
    /// Direction.
    pub direction: StepDirection,
    /// Criterion.
    pub criterion: StepCriterion,
    /// Most steps taken (a guard against cycling).
    pub max_steps: usize,
    /// Parent effects required (used with [`stepwise_selection_with_parents`]).
    pub heredity: Heredity,
}

impl Default for StepwiseOptions {
    fn default() -> Self {
        Self {
            direction: StepDirection::Mixed,
            criterion: StepCriterion::PValue { enter: 0.15, remove: 0.15 },
            max_steps: 100,
            heredity: Heredity::Strong,
        }
    }
}

/// Entering or leaving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAction {
    /// The effect entered the model.
    Enter,
    /// The effect left the model.
    Remove,
}

/// One step of the search.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRecord {
    /// What happened.
    pub action: StepAction,
    /// Effect index.
    pub effect: usize,
    /// p-value of the effect's partial F-test at that step.
    pub p_value: f64,
    /// AIC (or BIC when that is the criterion) of the model after the step.
    pub criterion: f64,
    /// R² of the model after the step.
    pub r_squared: f64,
}

/// Result of [`stepwise_selection`].
#[derive(Debug, Clone, PartialEq)]
pub struct StepwiseResult {
    /// Selected effects, in index order.
    pub selected: Vec<usize>,
    /// The steps taken.
    pub steps: Vec<StepRecord>,
    /// Criterion value of the starting model.
    pub start_criterion: f64,
    /// Effects left out of a backward search's starting model because, with the
    /// effects before them, the model was not estimable. Empty otherwise.
    pub set_aside: Vec<usize>,
}

/// Select effects for `y` from the columns of `x`, where effect `i` occupies
/// columns `effects[i]`.
///
/// # Errors
/// [`StatError::InvalidArgument`] for mismatched sizes, overlapping or
/// out-of-range effect columns, no effects, or p-value thresholds outside
/// (0, 1) or with `remove < enter`; [`StatError::NonFiniteInput`] for NaN or
/// infinite data; numerical failures of a fit other than rank deficiency.
pub fn stepwise_selection(
    x: &Matrix,
    y: &[f64],
    effects: &[Range<usize>],
    options: &StepwiseOptions,
) -> Result<StepwiseResult> {
    stepwise_selection_with_parents(x, y, effects, &vec![Vec::new(); effects.len()], options)
}

/// [`stepwise_selection`] where effect `i`'s parent effects are `parents[i]`
/// (indices into `effects`), restricted by `options.heredity`.
///
/// # Errors
/// As [`stepwise_selection`], and [`StatError::InvalidArgument`] when
/// `parents` does not have one entry per effect or names an effect that does
/// not exist.
pub fn stepwise_selection_with_parents(
    x: &Matrix,
    y: &[f64],
    effects: &[Range<usize>],
    parents: &[Vec<usize>],
    options: &StepwiseOptions,
) -> Result<StepwiseResult> {
    if parents.len() != effects.len() || parents.iter().flatten().any(|&p| p >= effects.len()) {
        return Err(invalid("parents must list existing effects, one list per effect"));
    }
    let n = y.len();
    if x.nrows() != n {
        return Err(invalid("the design and the response have different numbers of rows"));
    }
    if !x.is_finite() || y.iter().any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "stepwise data" });
    }
    if effects.is_empty() {
        return Err(invalid("give at least one candidate effect"));
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
    if let StepCriterion::PValue { enter, remove } = options.criterion {
        if !(enter > 0.0 && enter < 1.0 && remove > 0.0 && remove < 1.0 && remove >= enter) {
            return Err(invalid("p-value thresholds must lie in (0, 1) with α to remove ≥ α to enter"));
        }
    }
    if n < 2 {
        return Err(invalid("at least two observations are needed"));
    }
    let model = Model { x, y, effects, n };
    let k = match options.criterion {
        StepCriterion::Bic => (n as f64).ln(),
        _ => 2.0,
    };
    let mut current: Vec<usize> = Vec::new();
    let mut set_aside = Vec::new();
    if options.direction == StepDirection::Backward {
        for e in 0..effects.len() {
            let mut trial = current.clone();
            trial.push(e);
            if model.rss(&trial)?.is_some() {
                current = trial;
            } else {
                set_aside.push(e);
            }
        }
    }
    let tss = model.rss(&[])?.unwrap_or(0.0);
    let mut current_rss = model.rss(&current)?.unwrap_or(tss);
    let start_criterion = model.criterion(current_rss, &current, k);
    let mut steps = Vec::new();
    let with = |set: &[usize], e: usize| {
        let mut s = set.to_vec();
        s.push(e);
        s.sort_unstable();
        s
    };
    let without = |set: &[usize], e: usize| set.iter().copied().filter(|&x| x != e).collect::<Vec<_>>();
    // Heredity: may `e` enter, or leave, the model `set`?
    let may_enter = |set: &[usize], e: usize| match options.heredity {
        Heredity::None => true,
        _ if parents[e].is_empty() => true,
        Heredity::Strong => parents[e].iter().all(|p| set.contains(p)),
        Heredity::Weak => parents[e].iter().any(|p| set.contains(p)),
    };
    let may_leave = |set: &[usize], e: usize| match options.heredity {
        Heredity::None => true,
        Heredity::Strong => !set.iter().any(|&c| parents[c].contains(&e)),
        // No remaining child may lose its last parent.
        Heredity::Weak => {
            !set.iter().any(|&c| parents[c].contains(&e) && !parents[c].iter().any(|&p| p != e && set.contains(&p)))
        }
    };
    let forward = options.direction != StepDirection::Backward;
    let backward = options.direction != StepDirection::Forward;
    while steps.len() < options.max_steps {
        // Candidate moves: (action, effect, rss after, p-value).
        let mut adds = Vec::new();
        if forward {
            for e in (0..effects.len()).filter(|&e| !current.contains(&e) && may_enter(&current, e)) {
                let set = with(&current, e);
                // Skip entries that would make the model inestimable.
                if let Some(rss) = model.rss(&set)? {
                    adds.push((e, rss, model.p_value(current_rss, rss, &set, e)?));
                }
            }
        }
        let mut removes = Vec::new();
        if backward && !current.is_empty() {
            for &e in current.iter().filter(|&&e| may_leave(&current, e)) {
                let set = without(&current, e);
                // A sub-model of an estimable model is estimable.
                let rss = model.rss(&set)?.unwrap_or(tss);
                removes.push((e, rss, model.p_value(rss, current_rss, &current, e)?));
            }
        }
        let chosen = match options.criterion {
            StepCriterion::PValue { enter, remove } => {
                // Mixed: try a removal first (after an entry); otherwise enter.
                let worst = removes.iter().copied().filter(|r| r.2 > remove).max_by(|a, b| a.2.total_cmp(&b.2));
                let best = adds.iter().copied().filter(|a| a.2 < enter).min_by(|a, b| a.2.total_cmp(&b.2));
                match (options.direction, worst, best) {
                    (StepDirection::Forward, _, Some(a)) => Some((StepAction::Enter, a)),
                    (StepDirection::Backward, Some(r), _) => Some((StepAction::Remove, r)),
                    (StepDirection::Mixed, Some(r), _) if !steps.is_empty() => Some((StepAction::Remove, r)),
                    (StepDirection::Mixed, _, Some(a)) => Some((StepAction::Enter, a)),
                    _ => None,
                }
            }
            StepCriterion::Aic | StepCriterion::Bic => {
                let now = model.criterion(current_rss, &current, k);
                let score = |set: &[usize], rss: f64| model.criterion(rss, set, k);
                let mut best: Option<(StepAction, (usize, f64, f64), f64)> = None;
                for &a in &adds {
                    let c = score(&with(&current, a.0), a.1);
                    if c < now - 1e-10 && best.as_ref().is_none_or(|b| c < b.2) {
                        best = Some((StepAction::Enter, a, c));
                    }
                }
                for &r in &removes {
                    let c = score(&without(&current, r.0), r.1);
                    if c < now - 1e-10 && best.as_ref().is_none_or(|b| c < b.2) {
                        best = Some((StepAction::Remove, r, c));
                    }
                }
                best.map(|(action, m, _)| (action, m))
            }
        };
        let Some((action, (effect, rss, p_value))) = chosen else { break };
        current = match action {
            StepAction::Enter => with(&current, effect),
            StepAction::Remove => without(&current, effect),
        };
        current_rss = rss;
        steps.push(StepRecord {
            action,
            effect,
            p_value,
            criterion: model.criterion(rss, &current, k),
            r_squared: if tss > 0.0 { 1.0 - rss / tss } else { f64::NAN },
        });
    }
    Ok(StepwiseResult { selected: current, steps, start_criterion, set_aside })
}

struct Model<'a> {
    x: &'a Matrix,
    y: &'a [f64],
    effects: &'a [Range<usize>],
    n: usize,
}

impl Model<'_> {
    fn columns(&self, set: &[usize]) -> usize {
        set.iter().map(|&e| self.effects[e].len()).sum()
    }

    /// Residual sum of squares of the model with the effects in `set` and an
    /// intercept; `None` when that model is not estimable (rank deficient, or
    /// no residual degrees of freedom).
    fn rss(&self, set: &[usize]) -> Result<Option<f64>> {
        if set.is_empty() {
            let m = mean(self.y).unwrap_or(0.0);
            let c: Vec<f64> = self.y.iter().map(|v| v - m).collect();
            return Ok(Some(dot(&c, &c)));
        }
        if self.columns(set) + 1 >= self.n {
            return Ok(None);
        }
        let cols: Vec<&[f64]> = set.iter().flat_map(|&e| self.effects[e].clone()).map(|j| self.x.col(j)).collect();
        match fit_linear_model(&Matrix::from_columns(&cols)?, self.y, &OlsOptions::default()) {
            Ok(fit) => Ok(Some(fit.anova().error.sum_of_squares)),
            Err(StatError::RankDeficient { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// `n·ln(RSS/n) + k·(columns + 1)`.
    fn criterion(&self, rss: f64, set: &[usize], k: f64) -> f64 {
        let n = self.n as f64;
        n * (rss.max(f64::MIN_POSITIVE) / n).ln() + k * (self.columns(set) + 1) as f64
    }

    /// p-value of effect `e` given the RSS without and with it (`with_set` contains `e`).
    fn p_value(&self, rss_without: f64, rss_with: f64, with_set: &[usize], e: usize) -> Result<f64> {
        let df1 = self.effects[e].len() as f64;
        let df2 = self.n as f64 - (self.columns(with_set) + 1) as f64;
        if df2 <= 0.0 || rss_with <= 0.0 {
            return Ok(if rss_without > rss_with { 0.0 } else { 1.0 });
        }
        let f = ((rss_without - rss_with).max(0.0) / df1) / (rss_with / df2);
        FisherSnedecor::new(df1, df2)?.sf(f)
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// y depends on x1·x2 only; x1 and x2 alone explain nothing.
    #[test]
    fn heredity_restricts_entries_and_removals() {
        let n = 40;
        let x1: Vec<f64> = (0..n).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let x2: Vec<f64> = (0..n).map(|i| if (i / 2) % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let noise: Vec<f64> = (0..n).map(|i| ((i * 7919) % 13) as f64 / 13.0 - 0.5).collect();
        let y: Vec<f64> = (0..n).map(|i| 3.0 * x1[i] * x2[i] + 0.3 * noise[i]).collect();
        let x12: Vec<f64> = (0..n).map(|i| x1[i] * x2[i]).collect();
        let x = Matrix::from_columns(&[x1, x2, x12]).unwrap();
        let effects = [0..1, 1..2, 2..3];
        let parents = vec![vec![], vec![], vec![0, 1]];
        let run = |direction, heredity| {
            let o = StepwiseOptions {
                direction,
                criterion: StepCriterion::PValue { enter: 0.05, remove: 0.05 },
                max_steps: 50,
                heredity,
            };
            stepwise_selection_with_parents(&x, &y, &effects, &parents, &o).unwrap().selected
        };
        // Unrestricted: the interaction enters alone.
        assert_eq!(run(StepDirection::Forward, Heredity::None), [2]);
        // Strong heredity: it cannot enter without both main effects, which
        // never qualify on their own.
        assert!(run(StepDirection::Forward, Heredity::Strong).is_empty());
        assert!(run(StepDirection::Mixed, Heredity::Weak).is_empty());
        // Backward: the main effects stay while the interaction does.
        assert_eq!(run(StepDirection::Backward, Heredity::Strong), [0, 1, 2]);
        assert_eq!(run(StepDirection::Backward, Heredity::None), [2]);
        // Weak: one parent is enough to keep the interaction.
        assert_eq!(run(StepDirection::Backward, Heredity::Weak).len(), 2);
        let bad = StepwiseOptions::default();
        assert!(stepwise_selection_with_parents(&x, &y, &effects, &[vec![], vec![]], &bad).is_err());
        assert!(stepwise_selection_with_parents(&x, &y, &effects, &[vec![], vec![], vec![5]], &bad).is_err());
        for h in Heredity::ALL {
            assert_eq!(Heredity::from_key(h.key()), Some(h));
        }
    }

    /// Hald's cement data (Draper & Smith, *Applied Regression Analysis*):
    /// heat evolved against four ingredients.
    fn hald() -> (Matrix, Vec<f64>) {
        let x1 = [7.0, 1.0, 11.0, 11.0, 7.0, 11.0, 3.0, 1.0, 2.0, 21.0, 1.0, 11.0, 10.0];
        let x2 = [26.0, 29.0, 56.0, 31.0, 52.0, 55.0, 71.0, 31.0, 54.0, 47.0, 40.0, 66.0, 68.0];
        let x3 = [6.0, 15.0, 8.0, 8.0, 6.0, 9.0, 17.0, 22.0, 18.0, 4.0, 23.0, 9.0, 8.0];
        let x4 = [60.0, 52.0, 20.0, 47.0, 33.0, 22.0, 6.0, 44.0, 22.0, 26.0, 34.0, 12.0, 12.0];
        let y = vec![78.5, 74.3, 104.3, 87.6, 95.9, 109.2, 102.7, 72.5, 93.1, 115.9, 83.8, 113.3, 109.4];
        (Matrix::from_columns(&[&x1[..], &x2[..], &x3[..], &x4[..]]).unwrap(), y)
    }

    fn singles() -> Vec<Range<usize>> {
        (0..4).map(|j| j..j + 1).collect()
    }

    #[test]
    fn mixed_p_value_search_matches_the_textbook() {
        // Draper & Smith: enter X4, X1, X2, then remove X4; final model X1, X2.
        let (x, y) = hald();
        let r = stepwise_selection(&x, &y, &singles(), &StepwiseOptions::default()).unwrap();
        let path: Vec<(StepAction, usize)> = r.steps.iter().map(|s| (s.action, s.effect)).collect();
        assert_eq!(
            path,
            [(StepAction::Enter, 3), (StepAction::Enter, 0), (StepAction::Enter, 1), (StepAction::Remove, 3)]
        );
        assert_eq!(r.selected, [0, 1]);
        // p-values from an independent NumPy/SciPy computation.
        assert!((r.steps[0].p_value - 5.762_318_164_884_981e-4).abs() < 1e-12);
        assert!((r.steps[2].p_value - 0.051_687_348_977_422_456).abs() < 1e-12);
        assert!((r.steps[3].p_value - 0.205_395_438_101_683_98).abs() < 1e-12);
        assert!(r.steps.last().unwrap().r_squared > 0.978);
    }

    #[test]
    fn aic_and_bic_match_r_step() {
        // R: step(lm(y ~ x1 + x2 + x3 + x4)) drops x3, AIC = 24.97.
        let (x, y) = hald();
        let back = StepwiseOptions {
            direction: StepDirection::Backward,
            criterion: StepCriterion::Aic,
            max_steps: 50,
            heredity: Heredity::None,
        };
        let r = stepwise_selection(&x, &y, &singles(), &back).unwrap();
        assert_eq!(r.selected, [0, 1, 3]);
        assert_eq!(r.steps.len(), 1);
        assert!((r.steps[0].criterion - 24.973_883_608_541_04).abs() < 1e-9);
        let fwd = StepwiseOptions { direction: StepDirection::Forward, ..back };
        let f = stepwise_selection(&x, &y, &singles(), &fwd).unwrap();
        assert_eq!(f.selected, [0, 1, 3]);
        assert!((f.steps[0].criterion - 58.851_642_915_534_67).abs() < 1e-9);
        let bic = StepwiseOptions { criterion: StepCriterion::Bic, ..back };
        assert_eq!(stepwise_selection(&x, &y, &singles(), &bic).unwrap().selected, [0, 1]);
        // Mixed AIC from the empty model reaches the same model.
        let both = StepwiseOptions { direction: StepDirection::Mixed, ..back };
        assert_eq!(stepwise_selection(&x, &y, &singles(), &both).unwrap().selected, [0, 1, 3]);
    }

    #[test]
    fn aliased_and_excess_candidates_are_skipped() {
        let (x, y) = hald();
        // x1 twice (exactly aliased) plus x2: the copy can never join x1.
        let x1 = x.col(0).to_vec();
        let doubled: Vec<f64> = x1.iter().map(|v| 2.0 * v).collect();
        let aliased = Matrix::from_columns(&[&x1[..], x.col(1), &doubled[..]]).unwrap();
        let effects = vec![0..1, 1..2, 2..3];
        for direction in [StepDirection::Forward, StepDirection::Mixed, StepDirection::Backward] {
            let o =
                StepwiseOptions { direction, criterion: StepCriterion::Aic, max_steps: 50, heredity: Heredity::None };
            let r = stepwise_selection(&aliased, &y, &effects, &o).unwrap();
            assert!(!(r.selected.contains(&0) && r.selected.contains(&2)), "{direction:?}: {r:?}");
            assert_eq!(r.selected.len(), 2, "{direction:?}");
            assert_eq!(r.set_aside, if direction == StepDirection::Backward { vec![2] } else { vec![] });
        }
        // More candidate columns than observations: 4 originals + 10 noise columns, 13 rows.
        let mut cols: Vec<Vec<f64>> = (0..4).map(|j| x.col(j).to_vec()).collect();
        for k in 0..10 {
            cols.push((0..13).map(|i| (((i * 7 + k * 3) % 11) as f64).sin()).collect());
        }
        let refs: Vec<&[f64]> = cols.iter().map(Vec::as_slice).collect();
        let wide = Matrix::from_columns(&refs).unwrap();
        let effects: Vec<Range<usize>> = (0..14).map(|j| j..j + 1).collect();
        for direction in [StepDirection::Forward, StepDirection::Mixed, StepDirection::Backward] {
            for criterion in [StepCriterion::Aic, StepCriterion::PValue { enter: 0.15, remove: 0.15 }] {
                let o = StepwiseOptions { direction, criterion, max_steps: 100, heredity: Heredity::None };
                let r = stepwise_selection(&wide, &y, &effects, &o).unwrap();
                assert!(r.selected.len() + 1 < 13, "{direction:?} {criterion:?}: residual df remain");
                if direction == StepDirection::Backward {
                    assert!(!r.set_aside.is_empty());
                }
            }
        }
    }

    #[test]
    fn blocks_and_invalid_input() {
        let (x, y) = hald();
        // x1 and x2 as one two-column block: enters as a unit with df 2.
        let blocks = vec![0..2, 2..3, 3..4];
        let r = stepwise_selection(&x, &y, &blocks, &StepwiseOptions::default()).unwrap();
        assert!(r.selected.contains(&0));
        assert!(stepwise_selection(&x, &y, &[], &StepwiseOptions::default()).is_err());
        assert!(stepwise_selection(&x, &y, &[0..2, 1..3], &StepwiseOptions::default()).is_err(), "overlap");
        assert!(
            stepwise_selection(&x, &y, std::slice::from_ref(&(0..9)), &StepwiseOptions::default()).is_err(),
            "range"
        );
        let bad =
            StepwiseOptions { criterion: StepCriterion::PValue { enter: 0.2, remove: 0.1 }, ..Default::default() };
        assert!(stepwise_selection(&x, &y, &singles(), &bad).is_err());
        assert!(stepwise_selection(&x, &y[..5], &singles(), &StepwiseOptions::default()).is_err());
    }

    /// Missing values used to slip through at n = 2 (the RSS floor hid the
    /// NaN); they are now refused up front.
    #[test]
    fn non_finite_data_is_refused() {
        let x = Matrix::from_columns(&[vec![1.0, 2.0]]).unwrap();
        for y in [[1.0, f64::NAN], [f64::INFINITY, 2.0]] {
            let r = stepwise_selection(&x, &y, std::slice::from_ref(&(0..1)), &StepwiseOptions::default());
            assert!(matches!(r, Err(StatError::NonFiniteInput { .. })), "{r:?}");
        }
        let x = Matrix::from_columns(&[vec![1.0, f64::NAN]]).unwrap();
        assert!(
            stepwise_selection(&x, &[1.0, 2.0], std::slice::from_ref(&(0..1)), &StepwiseOptions::default()).is_err()
        );
    }
}
