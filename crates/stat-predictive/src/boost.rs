//! Gradient-boosted trees.
//!
//! **Model.** The prediction is a sum of small regression trees fitted one
//! after another, each to the gradient of the loss at the current fit
//! (Friedman, "Greedy function approximation: a gradient boosting machine",
//! *Annals of Statistics* 29, 2001), shrunk by a learning rate ν:
//! `F_m(x) = F_{m−1}(x) + ν f_m(x)`. A categorical response with two classes
//! fits one score per row (logistic loss, `p = 1/(1 + e^{−F})`); with K > 2
//! classes, one score per class (multinomial loss, softmax probabilities)
//! and one tree per class per round.
//!
//! **Trees.** Each tree is grown level by level to a maximum depth with
//! second-order (Newton) split gains and leaf values, regularized by an L2
//! penalty λ on the leaf values (Chen & Guestrin, "XGBoost", *KDD* 2016):
//! with G and H the sums of the loss's gradients and Hessians over a node,
//! a leaf's value is `−G/(H + λ)` and a split's gain
//! `G_L²/(H_L + λ) + G_R²/(H_R + λ) − G²/(H + λ)`. Continuous predictors
//! are binned at their training-row quantiles (at most `bins` thresholds),
//! so each split scans a histogram instead of sorted values; categorical
//! predictors split into two groups of levels after ordering the levels by
//! `G/(H + λ)`. Missing values go the way that gains more (or, when a node
//! has none, the larger side).
//!
//! **Losses.** Squared error (gradient `F − y`, Hessian 1), absolute error
//! (gradient `sign(F − y)`; leaf values are then the median residual of
//! the leaf's rows, as in Friedman's LAD-TreeBoost), logistic and
//! multinomial (gradient `p − y`, Hessian `p(1 − p)`). The starting score
//! is the mean, the median, the log-odds or the log class shares.
//!
//! **Subsampling and early stopping.** Each round may use a random share of
//! the training rows (stochastic gradient boosting; Friedman,
//! *Computational Statistics & Data Analysis* 38, 2002). When validation
//! rows are given, boosting stops once their loss has not improved for a
//! set number of rounds, and the model keeps the best round.

use rayon::prelude::*;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::tree::{Feature, Target, Value};

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// Loss minimized for a continuous response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegressionLoss {
    /// Squared error `(y − F)²/2`.
    Squared,
    /// Absolute error `|y − F|`, robust to outlying responses.
    Absolute,
}

/// Boosting settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoostOptions {
    /// Greatest number of rounds.
    pub rounds: usize,
    /// Shrinkage ν of each tree, in (0, 1].
    pub learning_rate: f64,
    /// Greatest depth of each tree.
    pub max_depth: usize,
    /// Fewest training rows in a leaf.
    pub min_leaf: usize,
    /// Smallest Hessian sum in a leaf (guards logistic leaves where p is
    /// near 0 or 1).
    pub min_hessian: f64,
    /// L2 penalty λ on leaf values.
    pub lambda: f64,
    /// Smallest gain for a split.
    pub min_gain: f64,
    /// Share of the training rows each round uses, in (0, 1].
    pub subsample: f64,
    /// Greatest number of thresholds per continuous predictor.
    pub bins: usize,
    /// Rounds without improvement on the validation rows before stopping
    /// (0: never stop early).
    pub early_stopping: usize,
    /// Loss for a continuous response.
    pub regression_loss: RegressionLoss,
    /// Seed of the subsamples.
    pub seed: u64,
}

impl Default for BoostOptions {
    fn default() -> Self {
        Self {
            rounds: 300,
            learning_rate: 0.1,
            max_depth: 3,
            min_leaf: 5,
            min_hessian: 1e-3,
            lambda: 1.0,
            min_gain: 0.0,
            subsample: 0.8,
            bins: 1024,
            early_stopping: 20,
            regression_loss: RegressionLoss::Squared,
            seed: 1,
        }
    }
}

/// How a split sends rows.
#[derive(Debug, Clone, PartialEq)]
pub enum BoostRule {
    /// Values `≤ threshold` go left.
    Threshold(f64),
    /// `side[level]`: 0 right, 1 left, 2 unseen at the node (goes the
    /// missing-value way).
    Levels(Vec<u8>),
}

/// A split of a boosted tree's node.
#[derive(Debug, Clone, PartialEq)]
pub struct BoostSplit {
    /// Predictor index.
    pub feature: usize,
    /// Which rows go left.
    pub rule: BoostRule,
    /// Whether missing values go left.
    pub missing_left: bool,
    /// Loss reduction of the split.
    pub gain: f64,
}

/// A node of a boosted tree.
#[derive(Debug, Clone, PartialEq)]
pub struct BoostNode {
    /// The split (`None` for a leaf).
    pub split: Option<BoostSplit>,
    /// Children (left, right) of a split node.
    pub children: Option<(usize, usize)>,
    /// Leaf value (already shrunk by the learning rate).
    pub value: f64,
    /// Training rows of the round that reached the node.
    pub count: usize,
}

/// One regression tree of a boosted model.
#[derive(Debug, Clone, PartialEq)]
pub struct BoostTree {
    /// Nodes; the root is node 0.
    pub nodes: Vec<BoostNode>,
}

impl BoostTree {
    /// The tree's output for a row with predictor values `value(j)`.
    #[must_use]
    pub fn output(&self, value: &dyn Fn(usize) -> Value) -> f64 {
        let mut k = 0;
        loop {
            let node = &self.nodes[k];
            let (Some(split), Some((left, right))) = (&node.split, node.children) else { return node.value };
            k = if goes_left(split, value(split.feature)) { left } else { right };
        }
    }

    /// Number of leaves.
    #[must_use]
    pub fn leaves(&self) -> usize {
        self.nodes.iter().filter(|n| n.split.is_none()).count()
    }
}

fn goes_left(split: &BoostSplit, v: Value) -> bool {
    match (&split.rule, v) {
        (BoostRule::Threshold(t), Value::Number(x)) => x <= *t,
        (BoostRule::Levels(side), Value::Level(l)) => match side.get(l) {
            Some(0) => false,
            Some(1) => true,
            _ => split.missing_left,
        },
        _ => split.missing_left,
    }
}

/// The loss a model minimizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoostLoss {
    /// Squared error.
    Squared,
    /// Absolute error.
    Absolute,
    /// Logistic loss (two classes; the score is the log-odds of class 1).
    Logistic,
    /// Multinomial loss with this many classes.
    Multinomial(usize),
}

impl BoostLoss {
    /// Scores per row.
    fn width(self) -> usize {
        match self {
            Self::Multinomial(k) => k,
            _ => 1,
        }
    }
}

/// A fitted boosted model.
#[derive(Debug, Clone, PartialEq)]
pub struct Boosted {
    /// The loss.
    pub loss: BoostLoss,
    /// Starting score(s).
    pub base: Vec<f64>,
    /// `trees[m][k]`: round `m`'s tree for score `k`.
    pub trees: Vec<Vec<BoostTree>>,
    /// Training loss after each round (on all training rows), including
    /// rounds after the best one when boosting stopped early.
    pub training_curve: Vec<f64>,
    /// Validation loss after each round (empty without validation rows).
    pub validation_curve: Vec<f64>,
    /// Rounds kept.
    pub best_rounds: usize,
    /// Whether boosting stopped before the greatest number of rounds.
    pub stopped_early: bool,
}

impl Boosted {
    /// Scores (before the link) for a row, using the first `rounds` rounds.
    fn scores(&self, value: &dyn Fn(usize) -> Value, rounds: usize) -> Vec<f64> {
        let mut s = self.base.clone();
        for round in self.trees.iter().take(rounds) {
            for (sk, tree) in s.iter_mut().zip(round) {
                *sk += tree.output(value);
            }
        }
        s
    }

    /// The prediction for a row with predictor values `value(j)`: `[value]`
    /// for a continuous response, otherwise class probabilities.
    #[must_use]
    pub fn predict(&self, value: &dyn Fn(usize) -> Value) -> Vec<f64> {
        let s = self.scores(value, self.trees.len());
        link(self.loss, &s)
    }

    /// Total split gain credited to each of `n_features` predictors, summed
    /// over the kept trees.
    #[must_use]
    pub fn gain_importance(&self, n_features: usize) -> Vec<f64> {
        let mut out = vec![0.0; n_features];
        for tree in self.trees.iter().flatten() {
            for split in tree.nodes.iter().filter_map(|n| n.split.as_ref()) {
                if split.feature < n_features {
                    out[split.feature] += split.gain;
                }
            }
        }
        out
    }

    /// Number of splits on each of `n_features` predictors in the kept trees.
    #[must_use]
    pub fn split_counts(&self, n_features: usize) -> Vec<usize> {
        let mut out = vec![0; n_features];
        for tree in self.trees.iter().flatten() {
            for split in tree.nodes.iter().filter_map(|n| n.split.as_ref()) {
                if split.feature < n_features {
                    out[split.feature] += 1;
                }
            }
        }
        out
    }
}

fn link(loss: BoostLoss, s: &[f64]) -> Vec<f64> {
    match loss {
        BoostLoss::Squared | BoostLoss::Absolute => vec![s[0]],
        BoostLoss::Logistic => {
            let p = 1.0 / (1.0 + (-s[0]).exp());
            vec![1.0 - p, p]
        }
        BoostLoss::Multinomial(_) => {
            let m = s.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let e: Vec<f64> = s.iter().map(|v| (v - m).exp()).collect();
            let total: f64 = e.iter().sum();
            e.iter().map(|v| v / total).collect()
        }
    }
}

/// Loss of row `i` at scores `s`: half squared error, absolute error, or
/// the negative log-likelihood of the observed class.
fn row_loss(loss: BoostLoss, target: &Target, i: usize, s: &[f64]) -> f64 {
    match (loss, target) {
        (BoostLoss::Squared, Target::Regression(y)) => 0.5 * (y[i] - s[0]).powi(2),
        (BoostLoss::Absolute, Target::Regression(y)) => (y[i] - s[0]).abs(),
        (_, Target::Classification { classes, .. }) => {
            let p = link(loss, s);
            -p[classes[i]].max(1e-15).ln()
        }
        _ => f64::NAN,
    }
}

/// Bin edges and per-row bins of each predictor (training rows set the
/// edges; every row gets a bin, `missing` for missing values).
struct Binned {
    /// `bins[j][i]`: bin of row `i` for predictor `j` (`u16::MAX`: missing).
    bins: Vec<Vec<u16>>,
    /// Split threshold after each bin of a continuous predictor: halfway
    /// between the bin's largest training value and the next bin's
    /// smallest (`None` for a categorical one, whose bins are its levels).
    edges: Vec<Option<Vec<f64>>>,
    /// Number of bins of each predictor.
    sizes: Vec<usize>,
}

const MISSING: u16 = u16::MAX;

fn bin_features(features: &[Feature], train: &[usize], max_bins: usize) -> Binned {
    let max_bins = max_bins.clamp(2, usize::from(u16::MAX) - 1);
    let mut bins = Vec::with_capacity(features.len());
    let mut edges = Vec::with_capacity(features.len());
    let mut sizes = Vec::with_capacity(features.len());
    for f in features {
        match f {
            Feature::Numeric(x) => {
                let mut v: Vec<f64> = train.iter().map(|&i| x[i]).filter(|v| v.is_finite()).collect();
                v.sort_by(f64::total_cmp);
                v.dedup();
                // Thresholds: every distinct value, or quantiles of them.
                let e: Vec<f64> = if v.len() <= max_bins {
                    v.clone()
                } else {
                    let mut e: Vec<f64> =
                        (1..=max_bins).map(|k| v[(k * v.len() / max_bins).min(v.len()) - 1]).collect();
                    e.dedup();
                    e
                };
                let b: Vec<u16> = x
                    .iter()
                    .map(|&value| {
                        if value.is_nan() {
                            MISSING
                        } else {
                            // First edge ≥ value (values beyond the last edge
                            // share the last bin).
                            e.partition_point(|&edge| edge < value).min(e.len().saturating_sub(1)) as u16
                        }
                    })
                    .collect();
                // Midpoints to the next training value (training rows route
                // the same way by value as by bin).
                let thresholds = e
                    .iter()
                    .map(|&edge| {
                        let next = v.partition_point(|&x| x <= edge);
                        if next < v.len() { 0.5 * (edge + v[next]) } else { edge }
                    })
                    .collect();
                sizes.push(e.len().max(1));
                edges.push(Some(thresholds));
                bins.push(b);
            }
            Feature::Categorical { codes, levels } => {
                let levels = (*levels).min(max_bins);
                bins.push(codes.iter().map(|c| c.filter(|&l| l < levels).map_or(MISSING, |l| l as u16)).collect());
                edges.push(None);
                sizes.push(levels.max(1));
            }
        }
    }
    Binned { bins, edges, sizes }
}

/// One cell of a split histogram.
#[derive(Debug, Clone, Copy, Default)]
struct Cell {
    g: f64,
    h: f64,
    n: usize,
}

impl Cell {
    fn add(&mut self, other: Cell) {
        self.g += other.g;
        self.h += other.h;
        self.n += other.n;
    }
    fn minus(self, other: Cell) -> Cell {
        Cell { g: self.g - other.g, h: self.h - other.h, n: self.n - other.n }
    }
}

struct Candidate {
    split: BoostSplit,
    /// Bins (or levels) going left, for routing the training rows.
    left_bins: Vec<bool>,
}

struct TreeGrower<'a> {
    binned: &'a Binned,
    g: &'a [f64],
    h: &'a [f64],
    options: &'a BoostOptions,
}

impl TreeGrower<'_> {
    fn score(&self, c: Cell) -> f64 {
        c.g * c.g / (c.h + self.options.lambda)
    }

    fn allowed(&self, c: Cell) -> bool {
        c.n >= self.options.min_leaf && c.h >= self.options.min_hessian
    }

    /// Best split of `rows` on predictor `j`.
    fn best_on(&self, j: usize, rows: &[usize], total: Cell) -> Option<Candidate> {
        let size = self.binned.sizes[j];
        let bins = &self.binned.bins[j];
        let mut hist = vec![Cell::default(); size];
        let mut missing = Cell::default();
        for &i in rows {
            let c = Cell { g: self.g[i], h: self.h[i], n: 1 };
            match bins[i] {
                MISSING => missing.add(c),
                b => hist[usize::from(b)].add(c),
            }
        }
        let parent = self.score(total);
        // Order of the bins scanned: as they are for a continuous predictor,
        // by G/(H + λ) for a categorical one (levels absent here left out).
        let categorical = self.binned.edges[j].is_none();
        let mut order: Vec<usize> = (0..size).filter(|&b| !categorical || hist[b].n > 0).collect();
        if categorical {
            order.sort_by(|&a, &b| {
                let ra = hist[a].g / (hist[a].h + self.options.lambda);
                let rb = hist[b].g / (hist[b].h + self.options.lambda);
                ra.total_cmp(&rb)
            });
        }
        let mut best: Option<(f64, usize, bool)> = None;
        let mut left = Cell::default();
        for (k, &b) in order.iter().enumerate().take(order.len().saturating_sub(1)) {
            left.add(hist[b]);
            if hist[b].n == 0 {
                continue;
            }
            let present = total.minus(missing);
            for missing_left in [true, false] {
                if missing.n == 0 && !missing_left {
                    continue;
                }
                let mut l = left;
                if missing_left {
                    l.add(missing);
                }
                let r = total.minus(l);
                if !self.allowed(l) || !self.allowed(r) || present.minus(left).n == 0 {
                    continue;
                }
                let gain = self.score(l) + self.score(r) - parent;
                if gain > self.options.min_gain && best.is_none_or(|(g, _, _)| gain > g) {
                    best = Some((gain, k, missing_left));
                }
            }
        }
        let (gain, k, mut missing_left) = best?;
        let mut left_bins = vec![false; size];
        for &b in &order[..=k] {
            left_bins[b] = true;
        }
        if missing.n == 0 {
            // No missing values here: send them to the larger side.
            let n_left: usize = order[..=k].iter().map(|&b| hist[b].n).sum();
            missing_left = 2 * n_left >= total.n;
        }
        let rule = match &self.binned.edges[j] {
            Some(e) => BoostRule::Threshold(e[order[k]]),
            None => {
                BoostRule::Levels((0..size).map(|b| if hist[b].n == 0 { 2 } else { u8::from(left_bins[b]) }).collect())
            }
        };
        Some(Candidate { split: BoostSplit { feature: j, rule, missing_left, gain }, left_bins })
    }

    /// Grow a tree on `rows`; returns it and each leaf's rows.
    fn grow(&self, rows: Vec<usize>) -> (BoostTree, Vec<(usize, Vec<usize>)>) {
        let mut nodes = Vec::new();
        let mut leaves = Vec::new();
        let mut frontier = vec![(0usize, rows, 0usize)];
        nodes.push(BoostNode { split: None, children: None, value: 0.0, count: 0 });
        while let Some((k, rows, depth)) = frontier.pop() {
            let total = rows.iter().fold(Cell::default(), |mut c, &i| {
                c.add(Cell { g: self.g[i], h: self.h[i], n: 1 });
                c
            });
            nodes[k].count = rows.len();
            nodes[k].value = -total.g / (total.h + self.options.lambda) * self.options.learning_rate;
            let candidate = if depth < self.options.max_depth && rows.len() >= 2 * self.options.min_leaf {
                (0..self.binned.bins.len()).into_par_iter().filter_map(|j| self.best_on(j, &rows, total)).reduce_with(
                    |a, b| {
                        if b.split.gain > a.split.gain
                            || (b.split.gain == a.split.gain && b.split.feature < a.split.feature)
                        {
                            b
                        } else {
                            a
                        }
                    },
                )
            } else {
                None
            };
            let Some(c) = candidate else {
                leaves.push((k, rows));
                continue;
            };
            let bins = &self.binned.bins[c.split.feature];
            let (l, r): (Vec<usize>, Vec<usize>) = rows.iter().partition(|&&i| match bins[i] {
                MISSING => c.split.missing_left,
                b => c.left_bins[usize::from(b)],
            });
            let (li, ri) = (nodes.len(), nodes.len() + 1);
            nodes.push(BoostNode { split: None, children: None, value: 0.0, count: 0 });
            nodes.push(BoostNode { split: None, children: None, value: 0.0, count: 0 });
            nodes[k].split = Some(c.split);
            nodes[k].children = Some((li, ri));
            frontier.push((ri, r, depth + 1));
            frontier.push((li, l, depth + 1));
        }
        (BoostTree { nodes }, leaves)
    }
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 1 { v[m] } else { 0.5 * (v[m - 1] + v[m]) }
}

/// Fit a boosted model on the `train` rows; `valid` rows (possibly none)
/// drive early stopping.
///
/// # Errors
/// No training rows, a learning rate or subsample share outside (0, 1], no
/// rounds, a zero maximum depth or leaf size, a negative penalty, or a
/// categorical target with fewer than two classes.
pub fn fit_boosted(
    features: &[Feature],
    target: &Target,
    train: &[usize],
    valid: &[usize],
    options: &BoostOptions,
) -> Result<Boosted> {
    if train.is_empty() {
        return Err(invalid("no training rows"));
    }
    if options.rounds == 0 || options.max_depth == 0 || options.min_leaf == 0 {
        return Err(invalid("boosting needs at least one round, one level and one row per leaf"));
    }
    if !(options.learning_rate > 0.0 && options.learning_rate <= 1.0) {
        return Err(invalid("the learning rate must be in (0, 1]"));
    }
    if !(options.subsample > 0.0 && options.subsample <= 1.0) {
        return Err(invalid("the subsample share must be in (0, 1]"));
    }
    if options.lambda.is_nan() || options.lambda < 0.0 || options.min_hessian.is_nan() || options.min_hessian < 0.0 {
        return Err(invalid("the penalty and smallest leaf Hessian must not be negative"));
    }
    let loss = match target {
        Target::Regression(_) => match options.regression_loss {
            RegressionLoss::Squared => BoostLoss::Squared,
            RegressionLoss::Absolute => BoostLoss::Absolute,
        },
        Target::Classification { n_classes, .. } if *n_classes == 2 => BoostLoss::Logistic,
        Target::Classification { n_classes, .. } if *n_classes > 2 => BoostLoss::Multinomial(*n_classes),
        Target::Classification { .. } => return Err(invalid("a categorical response needs two or more classes")),
    };
    let width = loss.width();
    let n = target.len();
    // Starting scores.
    let base = match target {
        Target::Regression(y) => {
            let mut v: Vec<f64> = train.iter().map(|&i| y[i]).collect();
            if loss == BoostLoss::Squared { vec![v.iter().sum::<f64>() / v.len() as f64] } else { vec![median(&mut v)] }
        }
        Target::Classification { classes, n_classes } => {
            let mut counts = vec![0.5; *n_classes];
            for &i in train {
                counts[classes[i]] += 1.0;
            }
            let total: f64 = counts.iter().sum();
            if loss == BoostLoss::Logistic {
                vec![(counts[1] / counts[0]).ln()]
            } else {
                counts.iter().map(|c| (c / total).ln()).collect()
            }
        }
    };
    let binned = bin_features(features, train, options.bins);
    // Current scores of every row (training and validation).
    let mut scores: Vec<Vec<f64>> = vec![base.clone(); n];
    let mut rng = Rng::new(options.seed);
    let mut trees: Vec<Vec<BoostTree>> = Vec::new();
    let mut training_curve = Vec::new();
    let mut validation_curve = Vec::new();
    let (mut best_loss, mut best_rounds, mut stale) = (f64::INFINITY, 0usize, 0usize);
    let mean_loss = |rows: &[usize], scores: &[Vec<f64>]| {
        rows.iter().map(|&i| row_loss(loss, target, i, &scores[i])).sum::<f64>() / rows.len() as f64
    };
    let mut g = vec![0.0; n];
    let mut h = vec![0.0; n];
    let mut stopped_early = false;
    stat_core::interrupt::begin(options.rounds);
    for _ in 0..options.rounds {
        stat_core::interrupt::check()?;
        stat_core::interrupt::step();
        let sample: Vec<usize> = if options.subsample < 1.0 {
            let take = ((options.subsample * train.len() as f64).round() as usize).clamp(1, train.len());
            let mut s = train.to_vec();
            // Partial Fisher–Yates draw without replacement.
            let len = s.len();
            for k in 0..take {
                let pick = k + (rng.next_f64() * (len - k) as f64) as usize;
                s.swap(k, pick.min(len - 1));
            }
            s.truncate(take);
            s.sort_unstable();
            s
        } else {
            train.to_vec()
        };
        let probabilities: Vec<Vec<f64>> = match loss {
            BoostLoss::Logistic | BoostLoss::Multinomial(_) => sample.iter().map(|&i| link(loss, &scores[i])).collect(),
            _ => Vec::new(),
        };
        let mut round = Vec::with_capacity(width);
        for k in 0..width {
            for (s, &i) in sample.iter().enumerate() {
                let (gi, hi) = match (loss, target) {
                    (BoostLoss::Squared, Target::Regression(y)) => (scores[i][0] - y[i], 1.0),
                    (BoostLoss::Absolute, Target::Regression(y)) => {
                        // sign(F − y), 0 at a zero residual (f64::signum gives ±1 there).
                        let r = scores[i][0] - y[i];
                        (
                            if r > 0.0 {
                                1.0
                            } else if r < 0.0 {
                                -1.0
                            } else {
                                0.0
                            },
                            1.0,
                        )
                    }
                    (BoostLoss::Logistic, Target::Classification { classes, .. }) => {
                        let p = probabilities[s][1];
                        (p - f64::from(u8::from(classes[i] == 1)), (p * (1.0 - p)).max(1e-12))
                    }
                    (BoostLoss::Multinomial(_), Target::Classification { classes, .. }) => {
                        let p = probabilities[s][k];
                        (p - f64::from(u8::from(classes[i] == k)), (p * (1.0 - p)).max(1e-12))
                    }
                    _ => (0.0, 1.0),
                };
                g[i] = gi;
                h[i] = hi;
            }
            let grower = TreeGrower { binned: &binned, g: &g, h: &h, options };
            let (mut tree, leaves) = grower.grow(sample.clone());
            if let (BoostLoss::Absolute, Target::Regression(y)) = (loss, target) {
                for (leaf, rows) in &leaves {
                    let mut r: Vec<f64> = rows.iter().map(|&i| y[i] - scores[i][0]).collect();
                    tree.nodes[*leaf].value = options.learning_rate * median(&mut r);
                }
            }
            round.push(tree);
        }
        // Update every row's scores (the trees route by the original values).
        scores.par_iter_mut().enumerate().for_each(|(i, s)| {
            for (sk, tree) in s.iter_mut().zip(&round) {
                *sk += tree.output(&|j| features[j].value(i));
            }
        });
        trees.push(round);
        training_curve.push(mean_loss(train, &scores));
        if valid.is_empty() {
            best_rounds = trees.len();
            continue;
        }
        let v = mean_loss(valid, &scores);
        validation_curve.push(v);
        if v < best_loss - 1e-12 {
            best_loss = v;
            best_rounds = trees.len();
            stale = 0;
        } else {
            stale += 1;
            if options.early_stopping > 0 && stale >= options.early_stopping {
                stopped_early = true;
                break;
            }
        }
    }
    trees.truncate(best_rounds.max(1));
    Ok(Boosted { loss, base, trees, training_curve, validation_curve, best_rounds: best_rounds.max(1), stopped_early })
}

/// Mean held-out loss after each round, averaged over k folds of `train`
/// (each fold's model fitted without early stopping on the other folds), and
/// the round with the smallest loss. `fold_of[i]` is the fold of `train[i]`.
///
/// # Errors
/// As [`fit_boosted`].
pub fn cross_validate_rounds(
    features: &[Feature],
    target: &Target,
    train: &[usize],
    fold_of: &[usize],
    options: &BoostOptions,
) -> Result<(Vec<f64>, usize)> {
    let folds = fold_of.iter().copied().max().map_or(0, |m| m + 1);
    let opts = BoostOptions { early_stopping: 0, ..*options };
    let mut sum = vec![0.0; options.rounds];
    let mut weight = vec![0.0; options.rounds];
    for f in 0..folds {
        let fit: Vec<usize> = train.iter().zip(fold_of).filter(|(_, k)| **k != f).map(|(&r, _)| r).collect();
        let held: Vec<usize> = train.iter().zip(fold_of).filter(|(_, k)| **k == f).map(|(&r, _)| r).collect();
        if fit.is_empty() || held.is_empty() {
            continue;
        }
        let m = fit_boosted(features, target, &fit, &held, &opts)?;
        for (k, v) in m.validation_curve.iter().enumerate() {
            sum[k] += v * held.len() as f64;
            weight[k] += held.len() as f64;
        }
    }
    let curve: Vec<f64> = sum.iter().zip(&weight).map(|(s, w)| if *w > 0.0 { s / w } else { f64::NAN }).collect();
    let best = (0..curve.len()).filter(|&k| curve[k].is_finite()).min_by(|&a, &b| curve[a].total_cmp(&curve[b]));
    Ok((curve, best.map_or(options.rounds, |k| k + 1)))
}

/// Per-round loss of `model` on `rows`, for a curve of a fitted model.
#[must_use]
pub fn loss_curve(model: &Boosted, features: &[Feature], target: &Target, rows: &[usize]) -> Vec<f64> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut scores: Vec<Vec<f64>> = vec![model.base.clone(); rows.len()];
    model
        .trees
        .iter()
        .map(|round| {
            let mut total = 0.0;
            for (s, &i) in scores.iter_mut().zip(rows) {
                for (sk, tree) in s.iter_mut().zip(round) {
                    *sk += tree.output(&|j| features[j].value(i));
                }
                total += row_loss(model.loss, target, i, s);
            }
            total / rows.len() as f64
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// y = 10·[x0 > 0.5] + 4·x1 + noise; x2 is noise.
    fn data(n: usize, seed: u64) -> (Vec<Feature>, Vec<f64>) {
        let mut rng = Rng::new(seed);
        let x: Vec<Vec<f64>> = (0..3).map(|_| (0..n).map(|_| rng.next_f64()).collect()).collect();
        let y = (0..n).map(|i| if x[0][i] > 0.5 { 10.0 } else { 0.0 } + 4.0 * x[1][i] + 0.3 * rng.normal()).collect();
        (x.into_iter().map(Feature::Numeric).collect(), y)
    }

    #[test]
    fn one_round_stump_matches_hand_computation() {
        // Two groups: Newton leaf values −G/(H + λ) times ν, from the mean.
        let x = Feature::Numeric(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let y = vec![1.0, 1.0, 1.0, 7.0, 7.0, 7.0];
        let target = Target::Regression(y);
        let opts = BoostOptions {
            rounds: 1,
            learning_rate: 0.5,
            max_depth: 1,
            min_leaf: 1,
            lambda: 1.0,
            subsample: 1.0,
            ..BoostOptions::default()
        };
        let m = fit_boosted(&[x], &target, &[0, 1, 2, 3, 4, 5], &[], &opts).unwrap();
        assert_eq!(m.base, vec![4.0]);
        let tree = &m.trees[0][0];
        let split = tree.nodes[0].split.as_ref().unwrap();
        assert_eq!(split.rule, BoostRule::Threshold(3.5));
        // Left: g = 1 − 4 = −3 each, G = −9, H = 3 → value 0.5·9/4.
        let low = m.predict(&|_| Value::Number(2.0))[0];
        let high = m.predict(&|_| Value::Number(5.0))[0];
        assert!((low - (4.0 - 0.5 * 9.0 / 4.0)).abs() < 1e-12, "{low}");
        assert!((high - (4.0 + 0.5 * 9.0 / 4.0)).abs() < 1e-12, "{high}");
        // Gain: 81/4 + 81/4 − 0/7.
        assert!((split.gain - 40.5).abs() < 1e-12);
    }

    #[test]
    fn regression_fits_and_stops_early() {
        let (features, y) = data(600, 2);
        let target = Target::Regression(y);
        let train: Vec<usize> = (0..400).collect();
        let valid: Vec<usize> = (400..600).collect();
        let opts = BoostOptions { rounds: 2000, learning_rate: 0.3, ..BoostOptions::default() };
        let m = fit_boosted(&features, &target, &train, &valid, &opts).unwrap();
        assert!(m.stopped_early, "{} rounds", m.trees.len());
        assert_eq!(m.trees.len(), m.best_rounds);
        // Validation loss far below that of predicting the mean (the few
        // validation rows near the step at x0 = 0.5 dominate what remains).
        let best = m.validation_curve[m.best_rounds - 1];
        let Target::Regression(y) = &target else { unreachable!() };
        let mean = valid.iter().map(|&i| y[i]).sum::<f64>() / 200.0;
        let null = valid.iter().map(|&i| 0.5 * (y[i] - mean).powi(2)).sum::<f64>() / 200.0;
        assert!(best < 0.05 * null, "{best} vs {null}");
        // The training loss keeps falling.
        assert!(m.training_curve.windows(2).filter(|w| w[1] > w[0] + 1e-9).count() < 5);
        let imp = m.gain_importance(3);
        assert!(imp[0] > imp[1] && imp[1] > imp[2], "{imp:?}");
        // Same seed, same model; loss_curve reproduces the validation curve.
        let again = fit_boosted(&features, &target, &train, &valid, &opts).unwrap();
        assert_eq!(m, again);
        let curve = loss_curve(&m, &features, &target, &valid);
        for (a, b) in curve.iter().zip(&m.validation_curve) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn absolute_error_resists_outliers() {
        let (features, mut y) = data(400, 5);
        for i in (0..400).step_by(20) {
            y[i] += 500.0;
        }
        let target = Target::Regression(y);
        let train: Vec<usize> = (0..400).collect();
        let opts = BoostOptions {
            rounds: 200,
            regression_loss: RegressionLoss::Absolute,
            subsample: 1.0,
            ..BoostOptions::default()
        };
        let m = fit_boosted(&features, &target, &train, &[], &opts).unwrap();
        assert_eq!(m.loss, BoostLoss::Absolute);
        let lo = m.predict(&|j| Value::Number([0.2, 0.5, 0.5][j]))[0];
        let hi = m.predict(&|j| Value::Number([0.8, 0.5, 0.5][j]))[0];
        assert!((lo - 2.0).abs() < 1.0 && (hi - 12.0).abs() < 1.0, "{lo} {hi}");
    }

    #[test]
    fn classification_with_missing_and_categorical() {
        let mut rng = Rng::new(9);
        let n = 600;
        let x: Vec<f64> = (0..n).map(|_| rng.next_f64()).collect();
        let level: Vec<Option<usize>> =
            (0..n).map(|_| if rng.next_f64() < 0.1 { None } else { Some((rng.next_f64() * 4.0) as usize) }).collect();
        // Class from x for levels 0/1, flipped for levels 2/3; three classes
        // where x is in its middle third.
        let classes: Vec<usize> = (0..n)
            .map(|i| {
                let flip = level[i].is_some_and(|l| l >= 2);
                let a = if x[i] < 1.0 / 3.0 {
                    0
                } else if x[i] < 2.0 / 3.0 {
                    1
                } else {
                    2
                };
                if flip { 2 - a } else { a }
            })
            .collect();
        let mut xm = x.clone();
        for i in (0..n).step_by(15) {
            xm[i] = f64::NAN;
        }
        let features = vec![Feature::Numeric(xm), Feature::Categorical { codes: level, levels: 4 }];
        let target = Target::Classification { classes: classes.clone(), n_classes: 3 };
        let train: Vec<usize> = (0..n).collect();
        let m = fit_boosted(&features, &target, &train, &[], &BoostOptions::default()).unwrap();
        assert_eq!(m.loss, BoostLoss::Multinomial(3));
        assert_eq!(m.trees[0].len(), 3);
        let p = m.predict(&|j| [Value::Number(0.9), Value::Level(0)][j]);
        assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert!(p[2] > 0.8, "{p:?}");
        let q = m.predict(&|j| [Value::Number(0.9), Value::Level(3)][j]);
        assert!(q[0] > 0.8, "{q:?}");
        // Missing values and unseen levels still give probabilities.
        let r = m.predict(&|j| [Value::Missing, Value::Level(9)][j]);
        assert!((r.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        let wrong = (0..n)
            .filter(|&i| {
                let p = m.predict(&|j| features[j].value(i));
                (0..3).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap() != classes[i]
            })
            .count();
        assert!(wrong < n / 10, "{wrong}");
        // Two classes use one logistic score.
        let binary: Vec<usize> = classes.iter().map(|&c| usize::from(c == 2)).collect();
        let t2 = Target::Classification { classes: binary, n_classes: 2 };
        let m2 =
            fit_boosted(&features, &t2, &train, &[], &BoostOptions { rounds: 50, ..BoostOptions::default() }).unwrap();
        assert_eq!((m2.loss, m2.trees[0].len()), (BoostLoss::Logistic, 1));
    }

    #[test]
    fn cross_validated_rounds_and_errors() {
        let (features, y) = data(300, 4);
        let target = Target::Regression(y);
        let train: Vec<usize> = (0..300).collect();
        let fold_of: Vec<usize> = (0..300).map(|i| i % 5).collect();
        let opts = BoostOptions { rounds: 150, learning_rate: 0.3, ..BoostOptions::default() };
        let (curve, best) = cross_validate_rounds(&features, &target, &train, &fold_of, &opts).unwrap();
        assert_eq!(curve.len(), 150);
        assert!(best > 5 && best <= 150, "{best}");
        assert!(curve[best - 1] <= curve[0]);
        for bad in [
            BoostOptions { learning_rate: 0.0, ..opts },
            BoostOptions { subsample: 1.5, ..opts },
            BoostOptions { rounds: 0, ..opts },
        ] {
            assert!(fit_boosted(&features, &target, &train, &[], &bad).is_err());
        }
        assert!(fit_boosted(&features, &target, &[], &[], &opts).is_err());
    }

    /// The absolute-error gradient is `sign(F − y)`, zero for a row the
    /// current fit predicts exactly (Friedman 2001, LAD-TreeBoost). With
    /// y = (0, 0, 0, 0, 0, 10) the starting median is 0, so the gradients are
    /// (0, 0, 0, 0, 0, −1); with λ = 0 the best stump puts the last row alone
    /// and gains 1²/1 − 1²/6 = 5/6 (not 10/3, as gradients of +1 at the five
    /// exact rows would give).
    #[test]
    fn absolute_loss_gradient_is_zero_at_zero_residual() {
        let x = Feature::Numeric(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let target = Target::Regression(vec![0.0, 0.0, 0.0, 0.0, 0.0, 10.0]);
        let opts = BoostOptions {
            rounds: 1,
            learning_rate: 1.0,
            max_depth: 1,
            min_leaf: 1,
            lambda: 0.0,
            subsample: 1.0,
            regression_loss: RegressionLoss::Absolute,
            ..BoostOptions::default()
        };
        let m = fit_boosted(&[x], &target, &[0, 1, 2, 3, 4, 5], &[], &opts).unwrap();
        assert_eq!(m.base, vec![0.0]);
        let split = m.trees[0][0].nodes[0].split.as_ref().unwrap();
        assert_eq!(split.rule, BoostRule::Threshold(5.5));
        assert!((split.gain - 5.0 / 6.0).abs() < 1e-12, "{}", split.gain);
        // Leaf values are the leaves' median residuals: 0 and 10.
        assert!((m.predict(&|_| Value::Number(6.0))[0] - 10.0).abs() < 1e-12);
        assert!(m.predict(&|_| Value::Number(2.0))[0].abs() < 1e-12);
    }
}
