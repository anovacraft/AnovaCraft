//! Random forests (Breiman, *Machine Learning* 45, 2001): many unpruned
//! trees, each grown on a bootstrap sample of the rows and choosing each
//! split from a random subset of the predictors, whose predictions are
//! averaged (class probabilities for classification, means for regression).
//!
//! * **Out-of-bag (OOB) estimates.** About 37% of the rows are left out of
//!   each bootstrap sample. A row's OOB prediction averages only the trees
//!   that did not see it, so OOB error is an honest estimate of error on new
//!   data without holding rows back; [`Forest::oob_error_curve`] tracks it as
//!   trees are added.
//! * **Permutation importance.** For each tree, the increase in OOB error
//!   when one predictor's values are shuffled among that tree's OOB rows,
//!   averaged over trees (Breiman 2001, §10), with its standard error.
//! * **Impurity importance.** The impurity reduction of every split on the
//!   predictor, summed over each tree and averaged over trees. It favours
//!   predictors with many distinct values (Strobl et al., *BMC
//!   Bioinformatics* 8, 2007), which permutation importance does not.
//!
//! Trees are grown in parallel. Every tree has its own seed derived from the
//! forest's, so results do not depend on the number of threads.

use rayon::prelude::*;
use stat_core::rng::Rng;
use stat_core::{Result, StatError};

use crate::measures::most_probable;
use crate::tree::{Criterion, Feature, Target, Tree, TreeOptions, Value, fit_tree};

/// α at which a tree is evaluated in full (every split kept).
const FULL_TREE: f64 = -1.0;

/// Settings of a random forest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForestOptions {
    /// Number of trees.
    pub trees: usize,
    /// Predictors tried at each split; 0 chooses the usual default:
    /// `⌊√p⌋` for classification, `⌊p/3⌋` for regression (at least 1).
    pub features_per_split: usize,
    /// Size of each bootstrap sample as a share of the rows (drawn with
    /// replacement).
    pub sample_fraction: f64,
    /// Smallest node that may be split.
    pub min_split: usize,
    /// Fewest rows on each side of a split.
    pub min_leaf: usize,
    /// Greatest depth.
    pub max_depth: usize,
    /// Impurity of classification trees.
    pub criterion: Criterion,
    /// Surrogate splits kept per node (for rows with missing values).
    pub max_surrogates: usize,
    /// Seed of the bootstrap samples and predictor draws.
    pub seed: u64,
}

impl Default for ForestOptions {
    fn default() -> Self {
        Self {
            trees: 100,
            features_per_split: 0,
            sample_fraction: 1.0,
            min_split: 5,
            min_leaf: 2,
            max_depth: 30,
            criterion: Criterion::Gini,
            max_surrogates: 3,
            seed: 1,
        }
    }
}

/// A fitted random forest.
#[derive(Debug, Clone)]
pub struct Forest {
    /// The trees (their nodes keep counts and means but not row lists).
    pub trees: Vec<Tree>,
    /// Rows (data positions) left out of each tree's bootstrap sample.
    pub out_of_bag: Vec<Vec<usize>>,
    /// Classes of a classification forest (0 for regression).
    pub n_classes: usize,
    /// Predictors tried at each split.
    pub features_per_split: usize,
    /// OOB prediction of every data position (mean, or class probabilities);
    /// `None` for rows never out of bag or not used.
    pub oob_predictions: Vec<Option<Vec<f64>>>,
    /// OOB error (mean squared error, or misclassification rate) after the
    /// first `k + 1` trees, over the rows out of bag at least once by then
    /// (NaN while there are none).
    pub oob_error_curve: Vec<f64>,
}

/// A well-mixed seed for tree `b` (SplitMix64 finalizer).
fn tree_seed(seed: u64, b: usize) -> u64 {
    let mut z = seed ^ (b as u64).wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The output of `tree` for a row: `[mean]`, or class probabilities.
fn tree_output(tree: &Tree, value: &dyn Fn(usize) -> Value) -> Vec<f64> {
    let node = &tree.nodes[tree.leaf_for(value, FULL_TREE)];
    if tree.n_classes == 0 { vec![node.mean] } else { node.probabilities() }
}

/// Error of one prediction: squared error, or 1 for a wrong most-probable
/// class.
fn loss(target: &Target, i: usize, prediction: &[f64]) -> f64 {
    match target {
        Target::Regression(y) => (y[i] - prediction[0]).powi(2),
        Target::Classification { classes, .. } => f64::from(u8::from(most_probable(prediction) != classes[i])),
    }
}

/// Grow a random forest on `rows` (positions in `features` and `target`).
///
/// # Errors
/// [`StatError::InvalidArgument`] for no trees, a sample fraction outside
/// (0, 1], or anything [`fit_tree`] rejects.
pub fn fit_forest(features: &[Feature], target: &Target, rows: &[usize], options: &ForestOptions) -> Result<Forest> {
    if options.trees == 0 {
        return Err(StatError::InvalidArgument { message: "a forest needs at least one tree".into() });
    }
    if !(options.sample_fraction > 0.0 && options.sample_fraction <= 1.0) {
        return Err(StatError::InvalidArgument {
            message: "the bootstrap sample must be more than 0 and at most 100% of the rows".into(),
        });
    }
    if rows.is_empty() {
        return Err(StatError::InvalidArgument { message: "a forest needs at least one row".into() });
    }
    let p = features.len();
    let n_classes = target.n_classes();
    let m = match options.features_per_split {
        0 if n_classes > 0 => ((p as f64).sqrt().floor() as usize).max(1),
        0 => (p / 3).max(1),
        m => m.min(p.max(1)),
    };
    let draws = ((options.sample_fraction * rows.len() as f64).round() as usize).max(1);
    // Trees grow on rayon's threads: they check the caller's interrupt.
    let interrupt = stat_core::interrupt::current();
    if let Some(i) = &interrupt {
        i.begin(options.trees);
    }
    let grown: Vec<Result<(Tree, Vec<usize>)>> = (0..options.trees)
        .into_par_iter()
        .map(|b| {
            if let Some(i) = &interrupt {
                i.check()?;
                i.step();
            }
            let seed = tree_seed(options.seed, b);
            let mut rng = Rng::new(seed);
            let mut drawn = vec![false; rows.len()];
            let sample: Vec<usize> = (0..draws)
                .map(|_| {
                    let k = rng.below(rows.len());
                    drawn[k] = true;
                    rows[k]
                })
                .collect();
            let oob: Vec<usize> = rows.iter().zip(&drawn).filter(|(_, d)| !**d).map(|(&i, _)| i).collect();
            let tree_options = TreeOptions {
                criterion: options.criterion,
                min_split: options.min_split,
                min_leaf: options.min_leaf,
                max_depth: options.max_depth,
                max_surrogates: options.max_surrogates,
                features_per_split: m,
                seed: tree_seed(seed, usize::MAX),
                ..TreeOptions::default()
            };
            let mut tree = fit_tree(features, target, &sample, &tree_options)?;
            for node in &mut tree.nodes {
                node.rows = Vec::new();
            }
            Ok((tree, oob))
        })
        .collect();
    let mut trees = Vec::with_capacity(options.trees);
    let mut out_of_bag = Vec::with_capacity(options.trees);
    for g in grown {
        let (t, o) = g?;
        trees.push(t);
        out_of_bag.push(o);
    }
    // OOB predictions, accumulated tree by tree for the error curve.
    let n = target.len();
    let width = n_classes.max(1);
    let outputs: Vec<Vec<(usize, Vec<f64>)>> = trees
        .par_iter()
        .zip(&out_of_bag)
        .map(|(tree, oob)| oob.iter().map(|&i| (i, tree_output(tree, &|j| features[j].value(i)))).collect())
        .collect();
    let mut sums = vec![vec![0.0; width]; n];
    let mut votes = vec![0usize; n];
    let mut curve = Vec::with_capacity(trees.len());
    // Running total of current losses over rows with at least one vote.
    let mut current = vec![0.0; n];
    let (mut total, mut counted) = (0.0, 0usize);
    for tree_outputs in &outputs {
        for (i, out) in tree_outputs {
            let i = *i;
            for (s, o) in sums[i].iter_mut().zip(out) {
                *s += o;
            }
            votes[i] += 1;
            let mean: Vec<f64> = sums[i].iter().map(|s| s / votes[i] as f64).collect();
            let l = loss(target, i, &mean);
            if votes[i] == 1 {
                counted += 1;
            } else {
                total -= current[i];
            }
            current[i] = l;
            total += l;
        }
        curve.push(if counted > 0 { total / counted as f64 } else { f64::NAN });
    }
    let oob_predictions =
        (0..n).map(|i| (votes[i] > 0).then(|| sums[i].iter().map(|s| s / votes[i] as f64).collect())).collect();
    Ok(Forest { trees, out_of_bag, n_classes, features_per_split: m, oob_predictions, oob_error_curve: curve })
}

impl Forest {
    /// The forest's prediction for a row with predictor values `value(j)`:
    /// `[mean]` (regression) or the averaged class probabilities.
    #[must_use]
    pub fn predict(&self, value: &dyn Fn(usize) -> Value) -> Vec<f64> {
        let mut sum = vec![0.0; self.n_classes.max(1)];
        for tree in &self.trees {
            for (s, o) in sum.iter_mut().zip(tree_output(tree, value)) {
                *s += o;
            }
        }
        let b = self.trees.len().max(1) as f64;
        sum.iter().map(|s| s / b).collect()
    }

    /// Mean impurity reduction credited to each of `n_features` predictors
    /// per tree.
    #[must_use]
    pub fn impurity_importance(&self, n_features: usize) -> Vec<f64> {
        let mut out = vec![0.0; n_features];
        for tree in &self.trees {
            for (o, v) in out.iter_mut().zip(tree.importance(n_features, FULL_TREE)) {
                *o += v;
            }
        }
        let b = self.trees.len().max(1) as f64;
        out.iter().map(|v| v / b).collect()
    }

    /// Permutation importance of each predictor: the mean over trees of the
    /// increase in OOB error (mean squared error, or misclassification rate)
    /// when the predictor's values are shuffled among the tree's OOB rows,
    /// with its standard error. Trees with fewer than two OOB rows are
    /// skipped. Seeded by `seed`.
    #[must_use]
    pub fn permutation_importance(&self, features: &[Feature], target: &Target, seed: u64) -> Vec<(f64, f64)> {
        let p = features.len();
        let per_tree: Vec<Option<Vec<f64>>> = self
            .trees
            .par_iter()
            .zip(&self.out_of_bag)
            .enumerate()
            .map(|(b, (tree, oob))| {
                if oob.len() < 2 {
                    return None;
                }
                let m = oob.len() as f64;
                let base: f64 =
                    oob.iter().map(|&i| loss(target, i, &tree_output(tree, &|j| features[j].value(i)))).sum::<f64>()
                        / m;
                let mut rng = Rng::new(tree_seed(seed, b));
                Some(
                    (0..p)
                        .map(|f| {
                            let mut shuffled = oob.clone();
                            rng.shuffle(&mut shuffled);
                            let permuted: f64 = oob
                                .iter()
                                .zip(&shuffled)
                                .map(|(&i, &s)| {
                                    let value =
                                        |j: usize| if j == f { features[j].value(s) } else { features[j].value(i) };
                                    loss(target, i, &tree_output(tree, &value))
                                })
                                .sum::<f64>()
                                / m;
                            permuted - base
                        })
                        .collect(),
                )
            })
            .collect();
        let used: Vec<&Vec<f64>> = per_tree.iter().flatten().collect();
        let b = used.len() as f64;
        (0..p)
            .map(|f| {
                if used.is_empty() {
                    return (f64::NAN, f64::NAN);
                }
                let mean = used.iter().map(|v| v[f]).sum::<f64>() / b;
                let var =
                    if b > 1.0 { used.iter().map(|v| (v[f] - mean).powi(2)).sum::<f64>() / (b - 1.0) } else { 0.0 };
                (mean, (var / b).sqrt())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// y depends on x0 (strongly) and x1 (weakly); x2 is noise.
    fn data(n: usize) -> (Vec<Feature>, Vec<f64>) {
        let mut rng = Rng::new(3);
        let x: Vec<Vec<f64>> = (0..3).map(|_| (0..n).map(|_| rng.next_f64()).collect()).collect();
        let y: Vec<f64> =
            (0..n).map(|i| if x[0][i] > 0.5 { 10.0 } else { 0.0 } + 2.0 * x[1][i] + 0.3 * rng.normal()).collect();
        (x.into_iter().map(Feature::Numeric).collect(), y)
    }

    #[test]
    fn regression_forest_oob_and_importance() {
        let (features, y) = data(300);
        let target = Target::Regression(y.clone());
        let rows: Vec<usize> = (0..300).collect();
        let opts = ForestOptions { trees: 60, seed: 5, ..ForestOptions::default() };
        let f = fit_forest(&features, &target, &rows, &opts).unwrap();
        assert_eq!(f.trees.len(), 60);
        assert_eq!(f.features_per_split, 1);
        // Each tree leaves out about e⁻¹ of the rows.
        let share = f.out_of_bag.iter().map(Vec::len).sum::<usize>() as f64 / (60.0 * 300.0);
        assert!((share - (-1.0_f64).exp()).abs() < 0.03, "{share}");
        // OOB error is well below the response variance and settles.
        let var = {
            let m = y.iter().sum::<f64>() / 300.0;
            y.iter().map(|v| (v - m).powi(2)).sum::<f64>() / 300.0
        };
        let last = *f.oob_error_curve.last().unwrap();
        assert!(last < 0.1 * var, "{last} vs {var}");
        assert!(f.oob_predictions.iter().all(Option::is_some));
        // The same seed gives the same forest.
        let g = fit_forest(&features, &target, &rows, &opts).unwrap();
        assert_eq!(f.oob_error_curve, g.oob_error_curve);
        // x0 matters most, then x1; noise near zero.
        let perm = f.permutation_importance(&features, &target, 1);
        assert!(perm[0].0 > perm[1].0 && perm[1].0 > perm[2].0, "{perm:?}");
        assert!(perm[2].0.abs() < 3.0 * perm[2].1 + 0.05, "{perm:?}");
        let imp = f.impurity_importance(3);
        assert!(imp[0] > imp[1] && imp[1] > imp[2]);
        // Predictions follow the step in x0.
        let lo = f.predict(&|j| Value::Number([0.2, 0.5, 0.5][j]))[0];
        let hi = f.predict(&|j| Value::Number([0.8, 0.5, 0.5][j]))[0];
        assert!(hi - lo > 8.0, "{lo} {hi}");
    }

    /// Out-of-bag predictions average exactly the trees that left the row
    /// out, and the forest does not depend on the number of threads.
    /// A cancelled interrupt stops the forest; one left alone counts every
    /// tree and changes nothing.
    #[test]
    fn interrupt_counts_trees_and_cancels() {
        use stat_core::interrupt::{Interrupt, with};
        let (features, y) = data(120);
        let target = Target::Regression(y);
        let rows: Vec<usize> = (0..120).collect();
        let opts = ForestOptions { trees: 25, seed: 3, ..ForestOptions::default() };
        let plain = fit_forest(&features, &target, &rows, &opts).unwrap();
        let i = Interrupt::new();
        let watched = with(std::sync::Arc::clone(&i), || fit_forest(&features, &target, &rows, &opts)).unwrap();
        assert_eq!(i.progress(), (25, 25));
        assert_eq!(watched.oob_predictions, plain.oob_predictions);
        i.cancel();
        let stopped = with(i, || fit_forest(&features, &target, &rows, &opts));
        assert!(matches!(stopped, Err(StatError::Cancelled)));
    }

    #[test]
    fn out_of_bag_rows_and_thread_independence() {
        let (features, y) = data(120);
        let target = Target::Regression(y);
        let rows: Vec<usize> = (0..120).collect();
        let opts = ForestOptions { trees: 25, sample_fraction: 0.7, seed: 11, ..ForestOptions::default() };
        let f = fit_forest(&features, &target, &rows, &opts).unwrap();
        for i in [0, 17, 64, 119] {
            let (mut sum, mut k) = (0.0, 0);
            for (tree, oob) in f.trees.iter().zip(&f.out_of_bag) {
                if oob.contains(&i) {
                    sum += tree_output(tree, &|j| features[j].value(i))[0];
                    k += 1;
                }
            }
            match &f.oob_predictions[i] {
                Some(p) => assert!((p[0] - sum / f64::from(k)).abs() < 1e-12 && k > 0),
                None => assert_eq!(k, 0),
            }
        }
        let single = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let g = single.install(|| fit_forest(&features, &target, &rows, &opts)).unwrap();
        assert_eq!(f.oob_error_curve, g.oob_error_curve);
        assert_eq!(f.out_of_bag, g.out_of_bag);
        let at = |j: usize| Value::Number([0.3, 0.6, 0.1][j]);
        assert_eq!(f.predict(&at), g.predict(&at));
    }

    #[test]
    fn classification_forest_probabilities() {
        let (features, y) = data(200);
        let classes: Vec<usize> = y.iter().map(|&v| usize::from(v > 5.0)).collect();
        let target = Target::Classification { classes, n_classes: 2 };
        let rows: Vec<usize> = (0..200).collect();
        let f =
            fit_forest(&features, &target, &rows, &ForestOptions { trees: 40, ..ForestOptions::default() }).unwrap();
        let p = f.predict(&|j| Value::Number([0.9, 0.5, 0.5][j]));
        assert_eq!(p.len(), 2);
        assert!((p[0] + p[1] - 1.0).abs() < 1e-12);
        assert!(p[1] > 0.8);
        assert!(*f.oob_error_curve.last().unwrap() < 0.1);
        assert!(
            fit_forest(&features, &target, &rows, &ForestOptions { trees: 0, ..ForestOptions::default() }).is_err()
        );
    }
}
