//! Pareto analysis of defect categories (Juran's "vital few and trivial
//! many"; Juran & Godfrey, *Juran's Quality Handbook*, 5th ed., 1999,
//! section 5; Montgomery, *Introduction to Statistical Quality Control*,
//! section 5.4).
//!
//! Each observation names a category and carries a weight (1 for a count of
//! rows, or a frequency). Categories are ranked by their totals, largest
//! first, and each gets its share of the grand total and the cumulative
//! share up to and including it:
//!
//! ```text
//! share_k      = T_k / Σ_j T_j
//! cumulative_k = Σ_{j ≤ k} share_j
//! ```
//!
//! Categories whose share falls below a threshold can be pooled into one
//! "Other" bar, which is placed last whatever its size (Juran's convention),
//! so the ranking of the named categories stays readable.

use stat_core::{Result, StatError};

/// Options of [`pareto_table`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParetoOptions {
    /// Pool categories whose share of the total is below this fraction
    /// (0 to 1) into one "Other" category. `0` keeps every category. Pooling
    /// needs at least two categories below the threshold; a single small
    /// category keeps its own name.
    pub other_below: f64,
}

impl Default for ParetoOptions {
    fn default() -> Self {
        Self { other_below: 0.0 }
    }
}

/// One ranked category of a [`ParetoTable`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParetoCategory {
    /// Category name (the pooled bar is named `"Other"`).
    pub label: String,
    /// Total weight of the category.
    pub total: f64,
    /// Indices (into the input) of the observations in the category.
    pub members: Vec<usize>,
    /// Share of the grand total, 0 to 1.
    pub share: f64,
    /// Cumulative share up to and including this category, 0 to 1.
    pub cumulative: f64,
    /// Names of the categories pooled into this one (empty unless it is the
    /// "Other" category).
    pub pooled: Vec<String>,
}

impl ParetoCategory {
    /// `true` for the pooled "Other" category.
    #[must_use]
    pub fn is_other(&self) -> bool {
        !self.pooled.is_empty()
    }
}

/// Categories ranked by total, largest first ("Other" last).
#[derive(Debug, Clone, PartialEq)]
pub struct ParetoTable {
    /// The ranked categories.
    pub categories: Vec<ParetoCategory>,
    /// Grand total of the weights.
    pub total: f64,
}

impl ParetoTable {
    /// Number of leading categories that together reach `fraction` of the
    /// total (the "vital few" at, say, 0.8), counting the pooled category if
    /// it is reached.
    #[must_use]
    pub fn vital_few(&self, fraction: f64) -> usize {
        self.categories.iter().position(|c| c.cumulative >= fraction - 1e-12).map_or(self.categories.len(), |i| i + 1)
    }
}

/// Rank categories by total weight.
///
/// `labels[i]` names observation `i`'s category and `weights[i]` is its
/// weight (1 for counts of rows, or a frequency). Ties keep the order in
/// which the categories first appear in the input. Observations with weight 0
/// still belong to their category (so they can be selected through it).
///
/// # Errors
/// [`StatError::InvalidArgument`] if the lengths differ, a weight is negative
/// or not finite, the threshold is outside `[0, 1)`, there are no
/// observations, or the total is zero.
pub fn pareto_table<S: AsRef<str>>(labels: &[S], weights: &[f64], options: ParetoOptions) -> Result<ParetoTable> {
    let invalid = |message: &str| StatError::InvalidArgument { message: message.to_owned() };
    if labels.len() != weights.len() {
        return Err(invalid("labels and weights must have the same length"));
    }
    if labels.is_empty() {
        return Err(invalid("a Pareto chart needs at least one observation"));
    }
    if weights.iter().any(|w| !w.is_finite() || *w < 0.0) {
        return Err(invalid("frequencies must be finite and not negative"));
    }
    if !(0.0..1.0).contains(&options.other_below) {
        return Err(invalid("the threshold for pooling into \"Other\" must be at least 0 and below 1"));
    }
    // Categories in order of first appearance, with compensated totals.
    let mut names: Vec<String> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    let mut index = std::collections::HashMap::new();
    for (i, label) in labels.iter().enumerate() {
        let label = label.as_ref();
        let k = *index.entry(label.to_owned()).or_insert_with(|| {
            names.push(label.to_owned());
            members.push(Vec::new());
            names.len() - 1
        });
        members[k].push(i);
    }
    let totals: Vec<f64> =
        members.iter().map(|m| stat_core::summation::sum_iter(m.iter().map(|&i| weights[i]))).collect();
    let total = stat_core::summation::sum_iter(totals.iter().copied());
    if total <= 0.0 {
        return Err(invalid("the frequencies add up to zero"));
    }
    let mut order: Vec<usize> = (0..names.len()).collect();
    // Stable: ties keep their first-appearance order.
    order.sort_by(|&a, &b| totals[b].total_cmp(&totals[a]));
    let small: Vec<usize> = order.iter().copied().filter(|&k| totals[k] / total < options.other_below).collect();
    let pool = small.len() >= 2;
    let mut categories: Vec<ParetoCategory> = order
        .iter()
        .copied()
        .filter(|k| !(pool && small.contains(k)))
        .map(|k| ParetoCategory {
            label: names[k].clone(),
            total: totals[k],
            members: members[k].clone(),
            share: 0.0,
            cumulative: 0.0,
            pooled: Vec::new(),
        })
        .collect();
    if pool {
        let mut pooled_members: Vec<usize> = small.iter().flat_map(|&k| members[k].iter().copied()).collect();
        pooled_members.sort_unstable();
        categories.push(ParetoCategory {
            label: "Other".to_owned(),
            total: stat_core::summation::sum_iter(small.iter().map(|&k| totals[k])),
            members: pooled_members,
            share: 0.0,
            cumulative: 0.0,
            pooled: small.iter().map(|&k| names[k].clone()).collect(),
        });
    }
    let mut running = 0.0;
    for c in &mut categories {
        c.share = c.total / total;
        running += c.total;
        c.cumulative = running / total;
    }
    if let Some(last) = categories.last_mut() {
        // Exactly 100% at the end, whatever the rounding.
        last.cumulative = 1.0;
    }
    Ok(ParetoTable { categories, total })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-computed example: 50 defects in five categories.
    fn defects() -> (Vec<&'static str>, Vec<f64>) {
        (
            vec!["Scratch", "Dent", "Stain", "Scratch", "Crack", "Burr", "Dent"],
            vec![12.0, 9.0, 4.0, 10.0, 2.0, 1.0, 12.0],
        )
    }

    #[test]
    fn ranks_categories_with_shares_and_cumulative_shares() {
        let (labels, weights) = defects();
        let t = pareto_table(&labels, &weights, ParetoOptions::default()).unwrap();
        assert!((t.total - 50.0).abs() < 1e-12);
        let ranked: Vec<(&str, f64)> = t.categories.iter().map(|c| (c.label.as_str(), c.total)).collect();
        // Scratch 22 and Dent 21 lead; Stain 4, Crack 2, Burr 1.
        assert_eq!(ranked, [("Scratch", 22.0), ("Dent", 21.0), ("Stain", 4.0), ("Crack", 2.0), ("Burr", 1.0)]);
        let cumulative: Vec<f64> = t.categories.iter().map(|c| c.cumulative).collect();
        for (got, want) in cumulative.iter().zip([0.44, 0.86, 0.94, 0.98, 1.0]) {
            assert!((got - want).abs() < 1e-12, "{got} vs {want}");
        }
        assert_eq!(t.categories[0].members, [0, 3]);
        assert!((t.categories[1].share - 0.42).abs() < 1e-12);
        assert_eq!(t.vital_few(0.8), 2);
        assert_eq!(t.vital_few(0.95), 4);
    }

    #[test]
    fn small_categories_pool_into_other_placed_last() {
        let (labels, weights) = defects();
        // Below 5%: Crack (4%) and Burr (2%) pool; Stain (8%) stays.
        let t = pareto_table(&labels, &weights, ParetoOptions { other_below: 0.05 }).unwrap();
        let ranked: Vec<(&str, f64)> = t.categories.iter().map(|c| (c.label.as_str(), c.total)).collect();
        assert_eq!(ranked, [("Scratch", 22.0), ("Dent", 21.0), ("Stain", 4.0), ("Other", 3.0)]);
        let other = &t.categories[3];
        assert!(other.is_other());
        assert_eq!(other.pooled, ["Crack", "Burr"]);
        assert_eq!(other.members, [4, 5]);
        assert!((other.share - 0.06).abs() < 1e-12 && other.cumulative == 1.0);
        // "Other" stays last even when it outgrows a named category.
        let t = pareto_table(&labels, &weights, ParetoOptions { other_below: 0.09 }).unwrap();
        let ranked: Vec<(&str, f64)> = t.categories.iter().map(|c| (c.label.as_str(), c.total)).collect();
        assert_eq!(ranked, [("Scratch", 22.0), ("Dent", 21.0), ("Other", 7.0)]);
        // A single small category is not renamed.
        let t = pareto_table(&["a", "a", "b"], &[10.0, 9.0, 1.0], ParetoOptions { other_below: 0.1 }).unwrap();
        assert_eq!(t.categories[1].label, "b");
    }

    #[test]
    fn ties_keep_first_appearance_and_bad_input_is_refused() {
        let t = pareto_table(&["b", "a", "c"], &[1.0, 1.0, 2.0], ParetoOptions::default()).unwrap();
        let order: Vec<&str> = t.categories.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(order, ["c", "b", "a"]);
        assert!(pareto_table(&["a"], &[-1.0], ParetoOptions::default()).is_err());
        assert!(pareto_table(&["a"], &[0.0], ParetoOptions::default()).is_err());
        assert!(pareto_table::<&str>(&[], &[], ParetoOptions::default()).is_err());
        assert!(pareto_table(&["a"], &[1.0, 2.0], ParetoOptions::default()).is_err());
        assert!(pareto_table(&["a"], &[1.0], ParetoOptions { other_below: 1.0 }).is_err());
    }
}
