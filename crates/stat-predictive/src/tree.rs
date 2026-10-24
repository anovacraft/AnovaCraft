//! Classification and regression trees (Breiman, Friedman, Olshen & Stone,
//! *Classification and Regression Trees*, 1984; "CART").
//!
//! **Growing.** Starting from all training rows, each node is split in two
//! by the single rule that most reduces impurity, until a node is too small
//! (`min_split` rows), too deep (`max_depth`), pure, or no split leaves
//! `min_leaf` rows on each side. Impurity is the sum of squared deviations
//! from the node mean for a continuous response; for a categorical one it is
//! `n·(1 − Σ pₖ²)` (Gini) or `−n·Σ pₖ ln pₖ` (entropy), with `pₖ` the class
//! shares in the node. A continuous predictor splits at a midpoint between
//! adjacent distinct values (`x < t` goes left); a categorical predictor
//! sends a set of levels left. For a continuous response or two classes the
//! best set is found among the levels ordered by their mean response
//! (Breiman et al. 1984, Theorem 4.5); with more classes all subsets are
//! tried for up to [`TreeOptions::max_exhaustive_levels`] levels (at most
//! [`EXHAUSTIVE_LEVELS_LIMIT`]), beyond which levels are ordered by the share
//! of the node's most common class.
//!
//! **Missing predictor values.** A split is chosen on the rows where its
//! predictor is present. Rows missing it then follow *surrogate splits*:
//! rules on other predictors chosen to send rows the same way as the primary
//! split as often as possible, kept only when they agree more often than
//! sending everything the majority way (§5.3 of Breiman et al.). A row
//! missing every surrogate goes the way most rows went.
//!
//! **Pruning.** A large tree overfits. Cost-complexity pruning (Breiman et
//! al., ch. 3) orders the subtrees by `R_α(T) = R(T) + α·|leaves(T)|`, with
//! risk `R` the squared error (continuous) or the number misclassified
//! (categorical). Repeatedly collapsing the "weakest link", the internal
//! node `t` with the smallest `g(t) = (R(t) − R(Tₜ)) / (|leaves(Tₜ)| − 1)`,
//! gives a nested sequence of subtrees, one per [`PruneStep`]; any α selects
//! one of them. The step is chosen on held-out data: validation rows, or
//! k-fold cross-validation ([`cross_validate_pruning`]), where trees grown
//! without each fold are pruned at the geometric midpoints `√(αₖ αₖ₊₁)` and
//! scored on the fold.

use stat_core::{Result, StatError};

use crate::validation::fold_assignment;

/// One predictor's values for every row of the data (rows are addressed by
/// their position in these vectors).
#[derive(Debug, Clone, PartialEq)]
pub enum Feature {
    /// A number per row; NaN is missing.
    Numeric(Vec<f64>),
    /// A level per row (`None` missing), out of `levels`.
    Categorical {
        /// Level of each row.
        codes: Vec<Option<usize>>,
        /// Number of levels.
        levels: usize,
    },
}

/// A predictor value of one row, for routing it down a tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// A continuous predictor's value.
    Number(f64),
    /// A categorical predictor's level.
    Level(usize),
    /// Missing (or a level the tree has not seen).
    Missing,
}

impl Feature {
    /// Row `i`'s value.
    #[must_use]
    pub fn value(&self, i: usize) -> Value {
        match self {
            Self::Numeric(x) => {
                if x[i].is_nan() {
                    Value::Missing
                } else {
                    Value::Number(x[i])
                }
            }
            Self::Categorical { codes, .. } => codes[i].map_or(Value::Missing, Value::Level),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Numeric(x) => x.len(),
            Self::Categorical { codes, .. } => codes.len(),
        }
    }
}

/// The response.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A continuous response (a regression tree).
    Regression(Vec<f64>),
    /// A class per row, out of `n_classes` (a classification tree).
    Classification {
        /// Class of each row.
        classes: Vec<usize>,
        /// Number of classes.
        n_classes: usize,
    },
}

impl Target {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Regression(y) => y.len(),
            Self::Classification { classes, .. } => classes.len(),
        }
    }

    pub(crate) fn n_classes(&self) -> usize {
        match self {
            Self::Regression(_) => 0,
            Self::Classification { n_classes, .. } => *n_classes,
        }
    }
}

/// Impurity measure of a classification tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criterion {
    /// Gini index `1 − Σ pₖ²`.
    Gini,
    /// Entropy `−Σ pₖ ln pₖ`.
    Entropy,
}

/// Growing limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeOptions {
    /// Impurity of a classification tree (ignored for regression).
    pub criterion: Criterion,
    /// Smallest node that may be split.
    pub min_split: usize,
    /// Fewest rows (with the split predictor present) on each side of a split.
    pub min_leaf: usize,
    /// Greatest depth (the root is depth 0).
    pub max_depth: usize,
    /// Surrogate splits kept per node.
    pub max_surrogates: usize,
    /// With three or more classes, categorical predictors with at most this
    /// many levels at a node are split by trying every subset (`2^(L−1) − 1`
    /// of them for `L` levels). Values above [`EXHAUSTIVE_LEVELS_LIMIT`] act
    /// as that limit; nodes with more levels use the ordered shortcut.
    pub max_exhaustive_levels: usize,
    /// Predictors drawn at random as candidates at each node (as in a random
    /// forest); 0, or at least the number of predictors, tries them all.
    pub features_per_split: usize,
    /// Seed of the candidate draws (unused when every predictor is tried).
    pub seed: u64,
}

/// Most levels for which [`TreeOptions::max_exhaustive_levels`] can ask for
/// every subset to be tried: 2¹⁹ − 1 ≈ 5·10⁵ subsets per node. Beyond this
/// the enumeration is impractically slow (and past 64 levels its bit mask
/// would overflow), so levels are ordered instead, as for two classes.
pub const EXHAUSTIVE_LEVELS_LIMIT: usize = 20;

impl Default for TreeOptions {
    fn default() -> Self {
        Self {
            criterion: Criterion::Gini,
            min_split: 20,
            min_leaf: 7,
            max_depth: 10,
            max_surrogates: 5,
            max_exhaustive_levels: 10,
            features_per_split: 0,
            seed: 1,
        }
    }
}

/// Which way a split sends a row.
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    /// `x < t` goes left.
    Below(f64),
    /// `x ≥ t` goes left.
    AtLeast(f64),
    /// Levels marked `true` go left.
    Levels(Vec<bool>),
}

impl Rule {
    /// `Some(true)` if `value` goes left, `None` if the rule cannot place it
    /// (missing, wrong kind, or a level beyond the rule's).
    #[must_use]
    pub fn goes_left(&self, value: Value) -> Option<bool> {
        match (self, value) {
            (Self::Below(t), Value::Number(x)) => Some(x < *t),
            (Self::AtLeast(t), Value::Number(x)) => Some(x >= *t),
            (Self::Levels(left), Value::Level(l)) => left.get(l).copied(),
            _ => None,
        }
    }
}

/// A surrogate for a node's split.
#[derive(Debug, Clone, PartialEq)]
pub struct Surrogate {
    /// Predictor.
    pub feature: usize,
    /// Rule.
    pub rule: Rule,
    /// Share of the node's rows (with both predictors present) it sends the
    /// same way as the primary split.
    pub agreement: f64,
}

/// A node's split.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    /// Predictor.
    pub feature: usize,
    /// Rule on it.
    pub rule: Rule,
    /// Impurity reduction on the rows with the predictor present.
    pub improvement: f64,
    /// Surrogates, best first, for rows missing the predictor.
    pub surrogates: Vec<Surrogate>,
    /// Where a row goes when the predictor and every surrogate are missing.
    pub majority_left: bool,
}

/// A node.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Depth (root 0).
    pub depth: usize,
    /// Training rows in the node.
    pub rows: Vec<usize>,
    /// Mean response (regression; NaN for classification).
    pub mean: f64,
    /// Rows of each class (classification; empty for regression).
    pub class_counts: Vec<usize>,
    /// Risk as a leaf: squared error about the mean, or rows misclassified.
    pub risk: f64,
    /// Split, for a node that was split while growing.
    pub split: Option<Split>,
    /// Left and right children of a split node.
    pub children: Option<(usize, usize)>,
    /// The α at or above which this node is a leaf (its subtree pruned);
    /// infinite for a node never pruned, 0 for a grown leaf.
    pub collapse_alpha: f64,
}

impl Node {
    /// Class probabilities (class shares in the node).
    #[must_use]
    pub fn probabilities(&self) -> Vec<f64> {
        let n = self.class_counts.iter().sum::<usize>().max(1) as f64;
        self.class_counts.iter().map(|&c| c as f64 / n).collect()
    }

    /// Most common class (the first on ties).
    #[must_use]
    pub fn majority_class(&self) -> usize {
        crate::measures::most_probable(&self.probabilities())
    }
}

/// One subtree of the pruning sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PruneStep {
    /// Smallest α giving this subtree.
    pub alpha: f64,
    /// Number of leaves.
    pub leaves: usize,
    /// Training risk (squared error or rows misclassified).
    pub risk: f64,
}

/// A grown tree with its pruning sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    /// Nodes; the root is node 0.
    pub nodes: Vec<Node>,
    /// Number of classes (0 for a regression tree).
    pub n_classes: usize,
    /// Pruning sequence, from the largest subtree (α = 0) to the root alone.
    pub steps: Vec<PruneStep>,
}

/// Running sums of a set of rows.
#[derive(Debug, Clone)]
struct Stats {
    n: f64,
    s: f64,
    ss: f64,
    counts: Vec<f64>,
}

struct Grower<'a> {
    features: &'a [Feature],
    target: &'a Target,
    options: TreeOptions,
    /// Centre for regression sums (the root mean), for accuracy.
    center: f64,
    nodes: Vec<Node>,
    /// Draws the candidate predictors of each node.
    rng: stat_core::rng::Rng,
}

impl Stats {
    fn empty(k: usize) -> Self {
        Self { n: 0.0, s: 0.0, ss: 0.0, counts: vec![0.0; k] }
    }

    fn add(&mut self, other: &Self) {
        self.n += other.n;
        self.s += other.s;
        self.ss += other.ss;
        for (a, b) in self.counts.iter_mut().zip(&other.counts) {
            *a += b;
        }
    }

    fn minus(&self, other: &Self) -> Self {
        Self {
            n: self.n - other.n,
            s: self.s - other.s,
            ss: self.ss - other.ss,
            counts: self.counts.iter().zip(&other.counts).map(|(a, b)| a - b).collect(),
        }
    }
}

impl Grower<'_> {
    fn k(&self) -> usize {
        self.target.n_classes()
    }

    fn push(&self, s: &mut Stats, i: usize) {
        s.n += 1.0;
        match self.target {
            Target::Regression(y) => {
                let d = y[i] - self.center;
                s.s += d;
                s.ss += d * d;
            }
            Target::Classification { classes, .. } => s.counts[classes[i]] += 1.0,
        }
    }

    fn stats(&self, rows: &[usize]) -> Stats {
        let mut s = Stats::empty(self.k());
        for &i in rows {
            self.push(&mut s, i);
        }
        s
    }

    fn impurity(&self, s: &Stats) -> f64 {
        if s.n <= 0.0 {
            return 0.0;
        }
        match self.target {
            Target::Regression(_) => (s.ss - s.s * s.s / s.n).max(0.0),
            Target::Classification { .. } => match self.options.criterion {
                Criterion::Gini => s.n * (1.0 - s.counts.iter().map(|c| (c / s.n).powi(2)).sum::<f64>()),
                Criterion::Entropy => -s.counts.iter().filter(|&&c| c > 0.0).map(|c| c * (c / s.n).ln()).sum::<f64>(),
            },
        }
    }

    fn risk(&self, s: &Stats) -> f64 {
        match self.target {
            Target::Regression(_) => (s.ss - s.s * s.s / s.n.max(1.0)).max(0.0),
            Target::Classification { .. } => s.n - s.counts.iter().copied().fold(0.0, f64::max),
        }
    }

    /// A value ordering the categories for a scan: mean response, or the
    /// share of class `focus`.
    fn order_key(&self, s: &Stats, focus: usize) -> f64 {
        match self.target {
            Target::Regression(_) => s.s / s.n,
            Target::Classification { .. } => s.counts[focus] / s.n,
        }
    }

    fn grow(&mut self, rows: Vec<usize>, depth: usize) -> usize {
        let stats = self.stats(&rows);
        let (mean, class_counts) = match self.target {
            Target::Regression(_) => (self.center + stats.s / stats.n, Vec::new()),
            Target::Classification { .. } => (f64::NAN, stats.counts.iter().map(|&c| c as usize).collect()),
        };
        let index = self.nodes.len();
        self.nodes.push(Node {
            depth,
            rows: Vec::new(),
            mean,
            class_counts,
            risk: self.risk(&stats),
            split: None,
            children: None,
            collapse_alpha: 0.0,
        });
        let impurity = self.impurity(&stats);
        let splittable = rows.len() >= self.options.min_split.max(2)
            && depth < self.options.max_depth
            && impurity > 1e-12 * (1.0 + impurity.abs());
        if splittable {
            let candidates = self.candidates();
            // When no drawn predictor can split the node, try the others.
            let found = self.best_split(&rows, &candidates).or_else(|| {
                let rest: Vec<usize> = (0..self.features.len()).filter(|j| !candidates.contains(j)).collect();
                self.best_split(&rows, &rest)
            });
            if let Some(mut split) = found {
                let present: Vec<(usize, bool)> = rows
                    .iter()
                    .filter_map(|&i| split.rule.goes_left(self.features[split.feature].value(i)).map(|l| (i, l)))
                    .collect();
                let n_left = present.iter().filter(|p| p.1).count();
                split.majority_left = 2 * n_left >= present.len();
                // Surrogates place training rows missing the predictor, and new rows later.
                split.surrogates = self.surrogates(split.feature, &present);
                let (mut left, mut right) = (Vec::new(), Vec::new());
                for &i in &rows {
                    if route(&split, &|j| self.features[j].value(i)) {
                        left.push(i);
                    } else {
                        right.push(i);
                    }
                }
                if !left.is_empty() && !right.is_empty() {
                    let l = self.grow(left, depth + 1);
                    let r = self.grow(right, depth + 1);
                    let node = &mut self.nodes[index];
                    node.split = Some(split);
                    node.children = Some((l, r));
                    node.collapse_alpha = f64::INFINITY;
                }
            }
        }
        self.nodes[index].rows = rows;
        index
    }

    /// The predictors tried at a node: all of them, or a random
    /// `features_per_split` of them (in index order).
    fn candidates(&mut self) -> Vec<usize> {
        let p = self.features.len();
        let m = self.options.features_per_split;
        if m == 0 || m >= p {
            return (0..p).collect();
        }
        let mut all: Vec<usize> = (0..p).collect();
        // Partial Fisher–Yates: the first m entries are a uniform sample.
        for k in 0..m {
            let j = k + self.rng.below(p - k);
            all.swap(k, j);
        }
        let mut chosen = all[..m].to_vec();
        chosen.sort_unstable();
        chosen
    }

    fn best_split(&self, rows: &[usize], candidates: &[usize]) -> Option<Split> {
        let mut best: Option<Split> = None;
        let min_leaf = self.options.min_leaf.max(1) as f64;
        for &j in candidates {
            let feature = &self.features[j];
            let present: Vec<usize> =
                rows.iter().copied().filter(|&i| !matches!(feature.value(i), Value::Missing)).collect();
            if (present.len() as f64) < 2.0 * min_leaf {
                continue;
            }
            let total = self.stats(&present);
            let parent = self.impurity(&total);
            let candidate = match feature {
                Feature::Numeric(x) => self.numeric_split(x, &present, &total, parent, min_leaf),
                Feature::Categorical { codes, levels } => {
                    self.categorical_split(codes, *levels, &present, &total, parent, min_leaf)
                }
            };
            if let Some((rule, improvement)) = candidate {
                let better = best.as_ref().is_none_or(|b| improvement > b.improvement + 1e-12 * b.improvement.abs());
                if improvement > 1e-12 * (1.0 + parent) && better {
                    best = Some(Split { feature: j, rule, improvement, surrogates: Vec::new(), majority_left: true });
                }
            }
        }
        best
    }

    fn numeric_split(
        &self,
        x: &[f64],
        present: &[usize],
        total: &Stats,
        parent: f64,
        min_leaf: f64,
    ) -> Option<(Rule, f64)> {
        let mut order = present.to_vec();
        order.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
        let mut left = Stats::empty(self.k());
        let mut best: Option<(f64, f64)> = None;
        for k in 0..order.len() - 1 {
            self.push(&mut left, order[k]);
            let (a, b) = (x[order[k]], x[order[k + 1]]);
            if a == b || left.n < min_leaf || total.n - left.n < min_leaf {
                continue;
            }
            let right = total.minus(&left);
            let gain = parent - self.impurity(&left) - self.impurity(&right);
            if best.is_none_or(|(g, _)| gain > g) {
                let mid = a + (b - a) / 2.0;
                let t = if mid > a && mid <= b { mid } else { b };
                best = Some((gain, t));
            }
        }
        best.map(|(g, t)| (Rule::Below(t), g))
    }

    fn categorical_split(
        &self,
        codes: &[Option<usize>],
        levels: usize,
        present: &[usize],
        total: &Stats,
        parent: f64,
        min_leaf: f64,
    ) -> Option<(Rule, f64)> {
        let mut per = vec![Stats::empty(self.k()); levels];
        for &i in present {
            if let Some(l) = codes[i] {
                self.push(&mut per[l], i);
            }
        }
        let seen: Vec<usize> = (0..levels).filter(|&l| per[l].n > 0.0).collect();
        if seen.len() < 2 {
            return None;
        }
        let evaluate = |left_set: &[usize]| -> Option<f64> {
            let mut left = Stats::empty(self.k());
            for &l in left_set {
                left.add(&per[l]);
            }
            if left.n < min_leaf || total.n - left.n < min_leaf {
                return None;
            }
            let right = total.minus(&left);
            Some(parent - self.impurity(&left) - self.impurity(&right))
        };
        let mut best: Option<(f64, Vec<usize>)> = None;
        let mut consider = |set: Vec<usize>| {
            if let Some(g) = evaluate(&set) {
                if best.as_ref().is_none_or(|(b, _)| g > *b) {
                    best = Some((g, set));
                }
            }
        };
        let k = self.k();
        if k >= 3 && seen.len() <= self.options.max_exhaustive_levels.min(EXHAUSTIVE_LEVELS_LIMIT) {
            // Every subset, the last seen level always on the right.
            let m = seen.len() - 1;
            for mask in 1..(1usize << m) {
                consider((0..m).filter(|b| mask >> b & 1 == 1).map(|b| seen[b]).collect());
            }
        } else {
            let focus = if k >= 3 { crate::measures::most_probable(&total.counts) } else { 1.min(k.saturating_sub(1)) };
            let mut ordered = seen.clone();
            ordered.sort_by(|&a, &b| self.order_key(&per[a], focus).total_cmp(&self.order_key(&per[b], focus)));
            for cut in 1..ordered.len() {
                consider(ordered[..cut].to_vec());
            }
        }
        let (gain, set) = best?;
        let mut left = vec![false; levels];
        let n_left: f64 = set.iter().map(|&l| per[l].n).sum();
        // Levels absent from the node go with the larger side.
        if 2.0 * n_left >= total.n {
            left.iter_mut().for_each(|v| *v = true);
            for &l in &seen {
                left[l] = false;
            }
        }
        for &l in &set {
            left[l] = true;
        }
        Some((Rule::Levels(left), gain))
    }

    /// Surrogate splits for a split of `primary` that sent each row of
    /// `present` left (`true`) or right.
    fn surrogates(&self, primary: usize, present: &[(usize, bool)]) -> Vec<Surrogate> {
        if self.options.max_surrogates == 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (j, feature) in self.features.iter().enumerate() {
            if j == primary {
                continue;
            }
            let both: Vec<(usize, bool)> =
                present.iter().copied().filter(|&(i, _)| !matches!(feature.value(i), Value::Missing)).collect();
            if both.len() < 2 {
                continue;
            }
            let n = both.len() as f64;
            let n_left = both.iter().filter(|b| b.1).count() as f64;
            let majority = n_left.max(n - n_left);
            let found = match feature {
                Feature::Numeric(x) => {
                    let mut order = both.clone();
                    order.sort_by(|a, b| x[a.0].total_cmp(&x[b.0]));
                    let (mut lower_left, mut lower) = (0.0, 0.0);
                    let mut best: Option<(f64, Rule)> = None;
                    for k in 0..order.len() - 1 {
                        lower += 1.0;
                        lower_left += f64::from(u8::from(order[k].1));
                        let (a, b) = (x[order[k].0], x[order[k + 1].0]);
                        if a == b {
                            continue;
                        }
                        let mid = a + (b - a) / 2.0;
                        let t = if mid > a && mid <= b { mid } else { b };
                        // Lower part left: agreements are its lefts plus the upper part's rights.
                        let below = lower_left + ((n - lower) - (n_left - lower_left));
                        let above = (lower - lower_left) + (n_left - lower_left);
                        for (agree, rule) in [(below, Rule::Below(t)), (above, Rule::AtLeast(t))] {
                            if best.as_ref().is_none_or(|(g, _)| agree > *g) {
                                best = Some((agree, rule));
                            }
                        }
                    }
                    best
                }
                Feature::Categorical { codes, levels } => {
                    let mut l = vec![0.0_f64; *levels];
                    let mut r = vec![0.0_f64; *levels];
                    for &(i, left) in &both {
                        if let Some(c) = codes[i] {
                            if left {
                                l[c] += 1.0;
                            } else {
                                r[c] += 1.0;
                            }
                        }
                    }
                    let majority_left = 2.0 * n_left >= n;
                    let set: Vec<bool> = (0..*levels)
                        .map(|c| {
                            if l[c] + r[c] == 0.0 {
                                majority_left
                            } else {
                                l[c] > r[c] || (l[c] == r[c] && majority_left)
                            }
                        })
                        .collect();
                    let agree: f64 = (0..*levels).map(|c| l[c].max(r[c])).sum();
                    Some((agree, Rule::Levels(set)))
                }
            };
            if let Some((agree, rule)) = found {
                if agree > majority + 1e-9 {
                    out.push(Surrogate { feature: j, rule, agreement: agree / n });
                }
            }
        }
        out.sort_by(|a, b| b.agreement.total_cmp(&a.agreement));
        out.truncate(self.options.max_surrogates);
        out
    }
}

/// Whether a row with predictor values `value(j)` goes left at `split`.
fn route(split: &Split, value: &dyn Fn(usize) -> Value) -> bool {
    if let Some(l) = split.rule.goes_left(value(split.feature)) {
        return l;
    }
    for s in &split.surrogates {
        if let Some(l) = s.rule.goes_left(value(s.feature)) {
            return l;
        }
    }
    split.majority_left
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Grow a tree on `rows` (positions in `features` and `target`) and compute
/// its pruning sequence.
///
/// # Errors
/// [`StatError::InvalidArgument`] when there are no rows or predictors, the
/// lengths disagree, a class is out of range, or a response is not finite.
pub fn fit_tree(features: &[Feature], target: &Target, rows: &[usize], options: &TreeOptions) -> Result<Tree> {
    if features.is_empty() {
        return Err(invalid("a tree needs at least one predictor"));
    }
    if rows.is_empty() {
        return Err(invalid("a tree needs at least one row"));
    }
    let n = target.len();
    if features.iter().any(|f| f.len() != n) || rows.iter().any(|&i| i >= n) {
        return Err(invalid("the predictors and response have different lengths"));
    }
    let center = match target {
        Target::Regression(y) => {
            if rows.iter().any(|&i| !y[i].is_finite()) {
                return Err(invalid("the response has a missing or infinite value"));
            }
            rows.iter().map(|&i| y[i]).sum::<f64>() / rows.len() as f64
        }
        Target::Classification { classes, n_classes } => {
            if rows.iter().any(|&i| classes[i] >= *n_classes) {
                return Err(invalid("a class is out of range"));
            }
            0.0
        }
    };
    let mut grower = Grower {
        features,
        target,
        options: *options,
        center,
        nodes: Vec::new(),
        rng: stat_core::rng::Rng::new(options.seed),
    };
    grower.grow(rows.to_vec(), 0);
    let mut tree = Tree { nodes: grower.nodes, n_classes: target.n_classes(), steps: Vec::new() };
    tree.prune_sequence();
    Ok(tree)
}

impl Tree {
    /// Weakest-link pruning: set every node's `collapse_alpha` and the steps.
    fn prune_sequence(&mut self) {
        let n = self.nodes.len();
        let mut alphas = Vec::new();
        loop {
            // Leaves and risk of each node's current subtree (post-order:
            // children have larger indices than their parent).
            let mut leaves = vec![1usize; n];
            let mut risk: Vec<f64> = self.nodes.iter().map(|x| x.risk).collect();
            for t in (0..n).rev() {
                if let (Some((l, r)), true) = (self.nodes[t].children, self.nodes[t].collapse_alpha.is_infinite()) {
                    leaves[t] = leaves[l] + leaves[r];
                    risk[t] = risk[l] + risk[r];
                }
            }
            // Internal nodes of the current tree.
            let mut internal = Vec::new();
            let mut stack = vec![0];
            while let Some(t) = stack.pop() {
                if let (Some((l, r)), true) = (self.nodes[t].children, self.nodes[t].collapse_alpha.is_infinite()) {
                    internal.push(t);
                    stack.push(l);
                    stack.push(r);
                }
            }
            if internal.is_empty() {
                break;
            }
            let g: Vec<(usize, f64)> = internal
                .iter()
                .map(|&t| (t, ((self.nodes[t].risk - risk[t]) / (leaves[t] - 1) as f64).max(0.0)))
                .collect();
            let weakest = g.iter().map(|x| x.1).fold(f64::INFINITY, f64::min);
            let tolerance = 1e-10 * (1.0 + weakest.abs());
            for &(t, gt) in &g {
                if gt <= weakest + tolerance {
                    self.nodes[t].collapse_alpha = weakest;
                }
            }
            alphas.push(weakest);
        }
        // A node never collapses later than its ancestors.
        for t in 0..n {
            if let Some((l, r)) = self.nodes[t].children {
                let a = self.nodes[t].collapse_alpha;
                for c in [l, r] {
                    if self.nodes[c].children.is_some() {
                        self.nodes[c].collapse_alpha = self.nodes[c].collapse_alpha.min(a);
                    }
                }
            }
        }
        alphas.dedup_by(|a, b| (*a - *b).abs() <= 1e-10 * (1.0 + b.abs()));
        if alphas.first().is_none_or(|&a| a > 0.0) {
            alphas.insert(0, 0.0);
        }
        self.steps = alphas
            .into_iter()
            .map(|alpha| {
                let leaves = self.leaves(alpha);
                PruneStep { alpha, leaves: leaves.len(), risk: leaves.iter().map(|&t| self.nodes[t].risk).sum() }
            })
            .collect();
    }

    /// `true` if node `t` is a leaf of the subtree for `alpha`.
    #[must_use]
    pub fn is_leaf_at(&self, t: usize, alpha: f64) -> bool {
        self.nodes[t].children.is_none() || self.nodes[t].collapse_alpha <= alpha
    }

    /// The leaves of the subtree for `alpha`, left to right.
    #[must_use]
    pub fn leaves(&self, alpha: f64) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![0];
        while let Some(t) = stack.pop() {
            match self.nodes[t].children {
                Some((l, r)) if !self.is_leaf_at(t, alpha) => {
                    stack.push(r);
                    stack.push(l);
                }
                _ => out.push(t),
            }
        }
        out
    }

    /// The leaf of the subtree for `alpha` that a row with predictor values
    /// `value(j)` reaches.
    #[must_use]
    pub fn leaf_for(&self, value: &dyn Fn(usize) -> Value, alpha: f64) -> usize {
        let mut t = 0;
        while let (Some((l, r)), false) = (self.nodes[t].children, self.is_leaf_at(t, alpha)) {
            let Some(split) = &self.nodes[t].split else { break };
            t = if route(split, value) { l } else { r };
        }
        t
    }

    /// The step of the pruning sequence that `alpha` selects.
    #[must_use]
    pub fn step_for(&self, alpha: f64) -> usize {
        self.steps.iter().rposition(|s| s.alpha <= alpha).unwrap_or(0)
    }

    /// Total impurity reduction credited to each of `n_features` predictors
    /// by the primary splits of the subtree for `alpha`.
    #[must_use]
    pub fn importance(&self, n_features: usize, alpha: f64) -> Vec<f64> {
        let mut out = vec![0.0; n_features];
        let mut stack = vec![0];
        while let Some(t) = stack.pop() {
            if let (Some((l, r)), false) = (self.nodes[t].children, self.is_leaf_at(t, alpha)) {
                if let Some(s) = &self.nodes[t].split {
                    if s.feature < n_features {
                        out[s.feature] += s.improvement;
                    }
                }
                stack.push(l);
                stack.push(r);
            }
        }
        out
    }

    /// The α at which to prune for step `k` in cross-validation: the
    /// geometric mean of its α and the next step's (twice the last α, or 1,
    /// for the last step).
    #[must_use]
    pub fn representative_alpha(&self, k: usize) -> f64 {
        let a = self.steps[k].alpha;
        match self.steps.get(k + 1) {
            Some(next) => (a * next.alpha).sqrt(),
            None => {
                if a > 0.0 {
                    2.0 * a
                } else {
                    1.0
                }
            }
        }
    }
}

/// Cross-validated error of each step of a tree's pruning sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossValidation {
    /// Mean held-out error per row (squared error, or 1 per misclassified
    /// row) and its standard error, one per pruning step.
    pub errors: Vec<(f64, f64)>,
    /// Fold of each of the rows (in the order given).
    pub fold_of: Vec<usize>,
    /// The tree grown without each fold.
    pub trees: Vec<Tree>,
}

/// `folds`-fold cross-validation of `tree`'s pruning sequence (`tree` was
/// grown on `rows` with `options`).
///
/// Node risks and α are totals (squared error, misclassified rows), so a
/// tree grown on a fold's smaller training set has proportionally smaller
/// α values for the same structure. Each fold tree is therefore pruned at
/// the step's representative α times the ratio of the fold tree's root risk
/// to the full tree's, which puts α on the same relative scale (α/R(root),
/// rpart's complexity parameter) for every tree.
///
/// # Errors
/// From [`fold_assignment`] and [`fit_tree`].
pub fn cross_validate_pruning(
    features: &[Feature],
    target: &Target,
    rows: &[usize],
    options: &TreeOptions,
    tree: &Tree,
    folds: usize,
    seed: u64,
) -> Result<CrossValidation> {
    let fold_of = fold_assignment(rows.len(), folds, seed)?;
    cross_validate_pruning_with_folds(features, target, rows, options, tree, folds, fold_of)
}

/// [`cross_validate_pruning`] with a given fold (0-based, below `folds`) for
/// each of `rows`.
fn cross_validate_pruning_with_folds(
    features: &[Feature],
    target: &Target,
    rows: &[usize],
    options: &TreeOptions,
    tree: &Tree,
    folds: usize,
    fold_of: Vec<usize>,
) -> Result<CrossValidation> {
    let steps = tree.steps.len();
    let full_root = tree.nodes[0].risk;
    let mut per_row = vec![vec![0.0; rows.len()]; steps];
    let mut trees = Vec::with_capacity(folds);
    for f in 0..folds {
        let train: Vec<usize> = rows.iter().zip(&fold_of).filter(|(_, k)| **k != f).map(|(&i, _)| i).collect();
        let fold_tree = fit_tree(features, target, &train, options)?;
        let scale = if full_root > 0.0 { fold_tree.nodes[0].risk / full_root } else { 1.0 };
        for (k, errors) in per_row.iter_mut().enumerate() {
            let beta = tree.representative_alpha(k) * scale;
            for (pos, (&i, _)) in rows.iter().zip(&fold_of).enumerate().filter(|(_, (_, g))| **g == f) {
                let leaf = &fold_tree.nodes[fold_tree.leaf_for(&|j| features[j].value(i), beta)];
                errors[pos] = match target {
                    Target::Regression(y) => (y[i] - leaf.mean).powi(2),
                    Target::Classification { classes, .. } => f64::from(u8::from(leaf.majority_class() != classes[i])),
                };
            }
        }
        trees.push(fold_tree);
    }
    let n = rows.len() as f64;
    let errors = per_row
        .iter()
        .map(|e| {
            let mean = e.iter().sum::<f64>() / n;
            let var = if n > 1.0 { e.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0) } else { 0.0 };
            (mean, (var / n).sqrt())
        })
        .collect();
    Ok(CrossValidation { errors, fold_of, trees })
}

/// The step to keep given each step's held-out `(error, standard error)`:
/// the lowest error, or with `one_se`, the smallest tree (latest step)
/// whose error is within one standard error of the lowest (Breiman et al.
/// 1984, §3.4.3).
#[must_use]
pub fn choose_step(errors: &[(f64, f64)], one_se: bool) -> usize {
    let best = (0..errors.len()).min_by(|&a, &b| errors[a].0.total_cmp(&errors[b].0)).unwrap_or(0);
    if !one_se {
        return best;
    }
    let limit = errors[best].0 + errors[best].1;
    (best..errors.len()).rev().find(|&k| errors[k].0 <= limit).unwrap_or(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> TreeOptions {
        TreeOptions { min_split: 2, min_leaf: 1, max_depth: 10, ..TreeOptions::default() }
    }

    #[test]
    fn regression_split_is_the_best_cut() {
        // Brute force: the cut minimizing the two halves' squared error.
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let y = vec![1.0, 1.2, 0.8, 5.0, 5.3, 4.9, 9.0, 9.1];
        let t = fit_tree(
            &[Feature::Numeric(x.clone())],
            &Target::Regression(y.clone()),
            &(0..8).collect::<Vec<_>>(),
            &options(),
        )
        .unwrap();
        let sse = |v: &[f64]| {
            let m = v.iter().sum::<f64>() / v.len() as f64;
            v.iter().map(|a| (a - m).powi(2)).sum::<f64>()
        };
        let best =
            (1..8).min_by(|&a, &b| (sse(&y[..a]) + sse(&y[a..])).total_cmp(&(sse(&y[..b]) + sse(&y[b..])))).unwrap();
        let split = t.nodes[0].split.as_ref().unwrap();
        assert_eq!(split.rule, Rule::Below((x[best - 1] + x[best]) / 2.0));
        assert!((split.improvement - (sse(&y) - sse(&y[..best]) - sse(&y[best..]))).abs() < 1e-9);
        assert!((t.nodes[0].mean - y.iter().sum::<f64>() / 8.0).abs() < 1e-12);
        // Pruning sequence ends with the root alone; risks rise as α grows.
        let last = t.steps.last().unwrap();
        assert_eq!(last.leaves, 1);
        assert!((last.risk - sse(&y)).abs() < 1e-9);
        assert!(
            t.steps
                .windows(2)
                .all(|w| w[0].alpha < w[1].alpha && w[0].leaves > w[1].leaves && w[0].risk <= w[1].risk + 1e-9)
        );
    }

    #[test]
    fn weakest_link_pruning_matches_hand_calculation() {
        // Classes by x: 0 0 0 0 | 1 1 | 0 1 — the first split separates the
        // first four rows cleanly; the right side needs two more splits.
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let classes = vec![0, 0, 0, 0, 1, 1, 0, 1];
        let target = Target::Classification { classes, n_classes: 2 };
        let t = fit_tree(&[Feature::Numeric(x)], &target, &(0..8).collect::<Vec<_>>(), &options()).unwrap();
        // Full tree: 4 leaves, no errors. Root risk 3 (three 1s among eight).
        let full = t.leaves(0.0);
        assert_eq!(full.len(), 4);
        assert_eq!(t.steps[0].risk, 0.0);
        // Weakest links: the node holding [1 1 0 1] has risk 1 and a
        // three-leaf subtree with risk 0, so g = ½; its child [0 1] has
        // g = 1; the root has g = (3 − 0)/(4 − 1) = 1. So α₁ = ½ leaves two
        // leaves with risk 1, and then the root's g = (3 − 1)/(2 − 1) = 2.
        let alphas: Vec<f64> = t.steps.iter().map(|s| s.alpha).collect();
        assert_eq!(alphas, vec![0.0, 0.5, 2.0]);
        assert_eq!(t.steps.iter().map(|s| s.leaves).collect::<Vec<_>>(), vec![4, 2, 1]);
        assert_eq!(t.step_for(0.7), 1);
        assert_eq!(t.leaves(0.7).len(), 2);
    }

    #[test]
    fn categorical_splits_and_surrogates() {
        // Level 2 behaves like level 0; levels 1 and 3 are high.
        let codes: Vec<Option<usize>> = (0..40).map(|i| Some(i % 4)).collect();
        let y: Vec<f64> =
            (0..40).map(|i| if i % 4 == 1 || i % 4 == 3 { 10.0 } else { 0.0 } + (i % 3) as f64 * 0.1).collect();
        // A numeric copy of the split for surrogates, and missing levels.
        let proxy: Vec<f64> =
            (0..40).map(|i| if i % 4 == 1 || i % 4 == 3 { 1.0 } else { -1.0 } + i as f64 * 1e-3).collect();
        let mut with_missing = codes.clone();
        with_missing[5] = None; // level 1: high
        with_missing[6] = None; // level 2: low
        let features = vec![Feature::Categorical { codes: with_missing, levels: 4 }, Feature::Numeric(proxy)];
        let rows: Vec<usize> = (0..40).collect();
        let t = fit_tree(&features, &Target::Regression(y), &rows, &TreeOptions { max_depth: 1, ..options() }).unwrap();
        let split = t.nodes[0].split.as_ref().unwrap();
        // Either the factor or its perfect proxy splits; the other is a
        // surrogate with full agreement.
        let (l, r) = t.nodes[0].children.unwrap();
        let low = if t.nodes[l].mean < t.nodes[r].mean { l } else { r };
        assert!(t.nodes[low].rows.contains(&6) && !t.nodes[low].rows.contains(&5), "missing rows follow the surrogate");
        assert_eq!(t.nodes[low].rows.len(), 20);
        assert_eq!(split.surrogates.len(), 1);
        assert!((split.surrogates[0].agreement - 1.0).abs() < 1e-12);
        // Routing a new row with both predictors missing goes the majority way.
        let leaf = t.leaf_for(&|_| Value::Missing, 0.0);
        assert!(leaf == l || leaf == r);
    }

    #[test]
    fn multiclass_exhaustive_subsets() {
        // Three classes; levels {0, 2} → class 0, {1} → class 1, {3} → class 2.
        let codes: Vec<Option<usize>> = (0..60).map(|i| Some(i % 4)).collect();
        let classes: Vec<usize> = (0..60)
            .map(|i| match i % 4 {
                0 | 2 => 0,
                1 => 1,
                _ => 2,
            })
            .collect();
        let target = Target::Classification { classes: classes.clone(), n_classes: 3 };
        let t =
            fit_tree(&[Feature::Categorical { codes, levels: 4 }], &target, &(0..60).collect::<Vec<_>>(), &options())
                .unwrap();
        // Two splits make every leaf pure.
        assert_eq!(t.leaves(0.0).len(), 3);
        for leaf in t.leaves(0.0) {
            let n = &t.nodes[leaf];
            assert_eq!(n.class_counts.iter().filter(|&&c| c > 0).count(), 1);
        }
        let Rule::Levels(set) = &t.nodes[0].split.as_ref().unwrap().rule else { panic!() };
        assert_eq!(set[0], set[2], "levels 0 and 2 stay together");
    }

    #[test]
    fn many_levels_above_the_exhaustive_limit_use_the_ordered_shortcut() {
        // 70 levels with three classes and an unbounded exhaustive setting:
        // enumerating 2^69 subsets would overflow the mask (and never finish),
        // so the split must fall back to ordering the levels.
        let n = 280;
        let codes: Vec<Option<usize>> = (0..n).map(|i| Some(i % 70)).collect();
        let classes: Vec<usize> = (0..n).map(|i| (i % 70) % 3).collect();
        let target = Target::Classification { classes, n_classes: 3 };
        let opts = TreeOptions { max_exhaustive_levels: usize::MAX, max_depth: 2, ..options() };
        let t = fit_tree(&[Feature::Categorical { codes, levels: 70 }], &target, &(0..n).collect::<Vec<_>>(), &opts)
            .unwrap();
        let Rule::Levels(set) = &t.nodes[0].split.as_ref().unwrap().rule else { panic!() };
        // The most common class's levels (class 0: levels ≡ 0 mod 3) go together.
        let side = set[0];
        assert!((0..70).filter(|l| l % 3 == 0).all(|l| set[l] == side));
        assert!((0..70).filter(|l| l % 3 != 0).all(|l| set[l] != side));
    }

    /// With the same folds, every pruning step's cross-validated error
    /// matches rpart's fold trees pruned by `prune.rpart` at the step's
    /// complexity parameter, which rpart expresses relative to each tree's own
    /// root risk; the full tree's sequence matches rpart's CP table
    /// (`tests/data/cart_pruning_reference.R`, rpart 4.1.23). Pruning every
    /// fold tree at the full tree's absolute α instead misses most steps by
    /// 0.5–1%.
    #[test]
    fn cross_validation_matches_rpart_fold_trees() {
        let text = include_str!("../tests/data/cart_pruning_reference.txt");
        let mut lines = text.lines();
        let nums =
            |line: Option<&str>| -> Vec<f64> { line.unwrap().split_whitespace().map(|v| v.parse().unwrap()).collect() };
        let x = nums(lines.next());
        let y = nums(lines.next());
        let fold_of: Vec<usize> = nums(lines.next()).iter().map(|&f| f as usize - 1).collect();
        let reference: Vec<Vec<f64>> =
            lines.map(|l| l.split_whitespace().map(|v| v.parse().unwrap()).collect()).collect();
        let n = x.len();
        let rows: Vec<usize> = (0..n).collect();
        let features = vec![Feature::Numeric(x)];
        let opts =
            TreeOptions { min_split: 10, min_leaf: 5, max_depth: 4, max_surrogates: 0, ..TreeOptions::default() };
        let target = Target::Regression(y);
        let t = fit_tree(&features, &target, &rows, &opts).unwrap();
        assert_eq!(t.steps.len(), reference.len());
        let cv = cross_validate_pruning_with_folds(&features, &target, &rows, &opts, &t, 10, fold_of).unwrap();
        let root = t.nodes[0].risk;
        for r in &reference {
            let leaves = r[0] as usize + 1;
            let k = t.steps.iter().position(|s| s.leaves == leaves).unwrap_or_else(|| panic!("no step of {leaves}"));
            assert!((t.steps[k].alpha / root - r[1]).abs() < 1e-9, "{leaves} leaves: CP");
            let got = cv.errors[k].0 * n as f64 / root;
            assert!((got - r[2]).abs() < 1e-9, "{leaves} leaves: {got} vs rpart {}", r[2]);
        }
    }

    #[test]
    fn cross_validation_prefers_the_true_size() {
        // A step function with noise: two leaves are right; the full tree overfits.
        let n = 200;
        let x: Vec<f64> = (0..n).map(|i| i as f64 / n as f64).collect();
        let mut rng = stat_core::rng::Rng::new(4);
        let y: Vec<f64> = x.iter().map(|&v| if v < 0.5 { 0.0 } else { 3.0 } + rng.normal()).collect();
        let rows: Vec<usize> = (0..n).collect();
        let features = vec![Feature::Numeric(x)];
        let opts = TreeOptions { min_split: 4, min_leaf: 2, ..TreeOptions::default() };
        let target = Target::Regression(y);
        let t = fit_tree(&features, &target, &rows, &opts).unwrap();
        assert!(t.steps[0].leaves > 10);
        let cv = cross_validate_pruning(&features, &target, &rows, &opts, &t, 10, 1).unwrap();
        let k = choose_step(&cv.errors, true);
        assert!(t.steps[k].leaves <= 4, "one-SE choice keeps few leaves: {}", t.steps[k].leaves);
        let k_min = choose_step(&cv.errors, false);
        assert!(k_min <= k);
        assert!(cv.errors[k].0 < cv.errors[0].0 + cv.errors[0].1, "the pruned tree predicts at least as well");
        let imp = t.importance(1, t.steps[k].alpha);
        assert!(imp[0] > 0.0);
    }
}
