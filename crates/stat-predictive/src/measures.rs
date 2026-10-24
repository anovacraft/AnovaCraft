//! Standard measures of how well predictions match observed responses.
//!
//! **Continuous responses** ([`ContinuousFit`]): root mean square error
//! `√(Σ(yᵢ − ŷᵢ)²/n)`, mean absolute error `Σ|yᵢ − ŷᵢ|/n`, and
//! `R² = 1 − Σ(yᵢ − ŷᵢ)² / Σ(yᵢ − ȳ)²` with `ȳ` the mean of the rows
//! measured (so R² can be negative on held-out rows that the model predicts
//! worse than their own mean).
//!
//! **Categorical responses** ([`ClassFit`]): the misclassification rate of
//! the most probable class, the mean log loss `−Σ ln p̂ᵢ(yᵢ) / n` (with
//! probabilities clipped at 10⁻¹⁵ so that a confidently wrong prediction
//! costs a large but finite amount), the confusion matrix, and the area under
//! the ROC curve ([`auc`]): for two classes, of the second class's
//! probability; for more, the average of each class against the rest
//! (Hand & Till, *Machine Learning* 45, 2001, discuss alternatives).
//!
//! The AUC is computed as the Mann–Whitney statistic: the probability that a
//! randomly chosen positive scores above a randomly chosen negative, ties
//! counting one half (Hanley & McNeil, *Radiology* 143, 1982).

/// Smallest probability used in the log loss.
const PROBABILITY_FLOOR: f64 = 1e-15;

/// Fit measures for a continuous response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContinuousFit {
    /// Rows measured (pairs with both values present).
    pub n: usize,
    /// Root mean square error.
    pub rmse: f64,
    /// Mean absolute error.
    pub mae: f64,
    /// `1 − SSE / SST` over the rows measured (NaN when all `y` are equal).
    pub r_squared: f64,
}

impl ContinuousFit {
    /// Measures of predictions `predicted` against `observed`, skipping pairs
    /// with a NaN. `None` when no pair is complete.
    #[must_use]
    pub fn new(observed: &[f64], predicted: &[f64]) -> Option<Self> {
        let pairs: Vec<(f64, f64)> = observed
            .iter()
            .zip(predicted)
            .filter(|(y, p)| y.is_finite() && p.is_finite())
            .map(|(&y, &p)| (y, p))
            .collect();
        if pairs.is_empty() {
            return None;
        }
        let n = pairs.len() as f64;
        let mean = pairs.iter().map(|p| p.0).sum::<f64>() / n;
        let sse: f64 = pairs.iter().map(|(y, p)| (y - p).powi(2)).sum();
        let sst: f64 = pairs.iter().map(|(y, _)| (y - mean).powi(2)).sum();
        Some(Self {
            n: pairs.len(),
            rmse: (sse / n).sqrt(),
            mae: pairs.iter().map(|(y, p)| (y - p).abs()).sum::<f64>() / n,
            r_squared: if sst > 0.0 { 1.0 - sse / sst } else { f64::NAN },
        })
    }
}

/// Fit measures for a categorical response.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassFit {
    /// Rows measured.
    pub n: usize,
    /// Share of rows whose most probable class is not the observed one.
    pub misclassification: f64,
    /// Mean negative log probability of the observed class.
    pub log_loss: f64,
    /// Area under the ROC curve (see the module documentation); NaN when a
    /// class needed for it is absent.
    pub auc: f64,
    /// `confusion[actual][predicted]` counts.
    pub confusion: Vec<Vec<usize>>,
}

impl ClassFit {
    /// Measures of class probabilities `probabilities[i]` (one per class)
    /// against observed classes `observed[i]`, skipping rows whose
    /// probabilities are not all finite. `None` when no row is complete.
    #[must_use]
    pub fn new(observed: &[usize], probabilities: &[Vec<f64>], n_classes: usize) -> Option<Self> {
        let rows: Vec<(usize, &Vec<f64>)> = observed
            .iter()
            .zip(probabilities)
            .filter(|(c, p)| **c < n_classes && p.len() == n_classes && p.iter().all(|v| v.is_finite()))
            .map(|(&c, p)| (c, p))
            .collect();
        if rows.is_empty() {
            return None;
        }
        let n = rows.len() as f64;
        let mut confusion = vec![vec![0usize; n_classes]; n_classes];
        let mut wrong = 0usize;
        let mut loss = 0.0;
        for &(c, p) in &rows {
            let best = most_probable(p);
            confusion[c][best] += 1;
            wrong += usize::from(best != c);
            loss -= p[c].max(PROBABILITY_FLOOR).ln();
        }
        let auc = if n_classes == 2 {
            let scores: Vec<f64> = rows.iter().map(|(_, p)| p[1]).collect();
            let positive: Vec<bool> = rows.iter().map(|&(c, _)| c == 1).collect();
            auc(&scores, &positive)
        } else {
            let each: Vec<f64> = (0..n_classes)
                .map(|k| {
                    let scores: Vec<f64> = rows.iter().map(|(_, p)| p[k]).collect();
                    let positive: Vec<bool> = rows.iter().map(|&(c, _)| c == k).collect();
                    auc(&scores, &positive)
                })
                .collect();
            each.iter().sum::<f64>() / each.len().max(1) as f64
        };
        Some(Self { n: rows.len(), misclassification: wrong as f64 / n, log_loss: loss / n, auc, confusion })
    }
}

/// Index of the largest probability (the first on ties).
#[must_use]
pub fn most_probable(probabilities: &[f64]) -> usize {
    let mut best = 0;
    for (k, &p) in probabilities.iter().enumerate() {
        if p > probabilities[best] {
            best = k;
        }
    }
    best
}

/// Area under the ROC curve of `scores` for the rows marked `positive`: the
/// Mann–Whitney probability that a positive outscores a negative (ties ½).
/// NaN when there are no positives or no negatives.
#[must_use]
pub fn auc(scores: &[f64], positive: &[bool]) -> f64 {
    let n_pos = positive.iter().filter(|&&p| p).count();
    let n_neg = positive.len() - n_pos;
    if n_pos == 0 || n_neg == 0 {
        return f64::NAN;
    }
    // Mid-ranks of the scores.
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]));
    let mut ranks = vec![0.0; scores.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && scores[order[j + 1]] == scores[order[i]] {
            j += 1;
        }
        let mid = (i + j) as f64 / 2.0 + 1.0;
        for &k in &order[i..=j] {
            ranks[k] = mid;
        }
        i = j + 1;
    }
    let rank_sum: f64 = ranks.iter().zip(positive).filter(|(_, p)| **p).map(|(r, _)| r).sum();
    let (p, q) = (n_pos as f64, n_neg as f64);
    (rank_sum - p * (p + 1.0) / 2.0) / (p * q)
}

/// The ROC curve of `scores` for the rows marked `positive`: points
/// (false positive rate, true positive rate) from (0, 0) to (1, 1), one per
/// distinct score, thresholds descending. Empty when either class is absent.
#[must_use]
pub fn roc_curve(scores: &[f64], positive: &[bool]) -> Vec<[f64; 2]> {
    let n_pos = positive.iter().filter(|&&p| p).count() as f64;
    let n_neg = positive.len() as f64 - n_pos;
    if n_pos == 0.0 || n_neg == 0.0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    let mut out = vec![[0.0, 0.0]];
    let (mut tp, mut fp) = (0.0, 0.0);
    let mut i = 0;
    while i < order.len() {
        let s = scores[order[i]];
        while i < order.len() && scores[order[i]] == s {
            if positive[order[i]] {
                tp += 1.0;
            } else {
                fp += 1.0;
            }
            i += 1;
        }
        out.push([fp / n_neg, tp / n_pos]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_measures() {
        let f = ContinuousFit::new(&[1.0, 2.0, 3.0, f64::NAN], &[1.5, 2.0, 2.0, 9.0]).unwrap();
        assert_eq!(f.n, 3);
        assert!((f.rmse - (1.25_f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!((f.mae - 0.5).abs() < 1e-12);
        assert!((f.r_squared - (1.0 - 1.25 / 2.0)).abs() < 1e-12);
        assert!(ContinuousFit::new(&[f64::NAN], &[1.0]).is_none());
    }

    #[test]
    fn auc_matches_counting_pairs() {
        let scores = [0.9, 0.8, 0.8, 0.4, 0.3, 0.8, 0.1, 0.5];
        let positive = [true, true, false, true, false, true, false, false];
        // Count positive-negative pairs directly.
        let mut wins = 0.0;
        let mut pairs = 0.0;
        for i in 0..8 {
            for j in 0..8 {
                if positive[i] && !positive[j] {
                    pairs += 1.0;
                    wins += if scores[i] > scores[j] {
                        1.0
                    } else if scores[i] == scores[j] {
                        0.5
                    } else {
                        0.0
                    };
                }
            }
        }
        assert!((auc(&scores, &positive) - wins / pairs).abs() < 1e-12);
        assert!(auc(&[0.1, 0.2], &[true, true]).is_nan());
        let roc = roc_curve(&scores, &positive);
        assert_eq!(roc.first(), Some(&[0.0, 0.0]));
        assert_eq!(roc.last(), Some(&[1.0, 1.0]));
        // The trapezoid area under the curve is the AUC.
        let area: f64 = roc.windows(2).map(|w| (w[1][0] - w[0][0]) * (w[1][1] + w[0][1]) / 2.0).sum();
        assert!((area - wins / pairs).abs() < 1e-12);
    }

    #[test]
    fn class_measures() {
        let observed = [0, 1, 1, 2];
        let p = vec![vec![0.7, 0.2, 0.1], vec![0.1, 0.6, 0.3], vec![0.5, 0.4, 0.1], vec![0.2, 0.2, 0.6]];
        let f = ClassFit::new(&observed, &p, 3).unwrap();
        assert_eq!(f.n, 4);
        assert!((f.misclassification - 0.25).abs() < 1e-12);
        let loss = -(0.7_f64.ln() + 0.6_f64.ln() + 0.4_f64.ln() + 0.6_f64.ln()) / 4.0;
        assert!((f.log_loss - loss).abs() < 1e-12);
        assert_eq!(f.confusion[1], vec![1, 1, 0]);
        // A certain wrong answer costs ln(10¹⁵), not infinity.
        let f = ClassFit::new(&[0], &[vec![0.0, 1.0]], 2).unwrap();
        assert!((f.log_loss - 1e15_f64.ln()).abs() < 1e-9);
    }
}
