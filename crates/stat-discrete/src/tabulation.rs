//! Multi-way tables: counts, percentages and summary statistics of
//! continuous measures, cross-classified by several nested row
//! classifications and several nested column classifications.
//!
//! # Layout
//! The row classifications are nested in the order given: the rows of the
//! table are the combinations of their levels that occur in the data, in
//! lexicographic order of the levels (outermost first). The column
//! classifications are nested the same way. Optionally,
//!
//! * **subtotals** add, after each group of rows sharing an outer level, a
//!   row for that outer level over all inner levels (likewise for columns);
//! * **margins** add a grand-total row and column ("all levels").
//!
//! A row or column group is written as a [`GroupKey`]: one entry per
//! classification, `None` where the group spans every level.
//!
//! # Cell statistics
//! Each observation carries a frequency weight `w ≥ 0` (1 when none is
//! given). For a cell, the **count** is `Σ w`; the **row percentage** is
//! `100 · count / (count of the cell's row group over all columns)`, the
//! **column percentage** the same over all rows, and the **total
//! percentage** `100 · count / grand total` — the denominators do not depend
//! on whether margins are shown. For each measure, missing values (NaN) are
//! left out, and with `W = Σ w` over the cell's non-missing values:
//! `N = W`, sum `Σ w x`, mean `Σ w x / W`, standard deviation
//! `√(Σ w (x − x̄)² / (W − 1))` (frequency weights: as if each value were
//! repeated `w` times), median (the first value whose cumulative weight
//! passes `W/2`, or the average of it and the next value when the cumulative
//! weight equals `W/2` exactly; for whole-number weights, the middle of the
//! values repeated by their weights), minimum
//! and maximum. Statistics that need more data than the cell has are NaN.

use std::collections::BTreeSet;

use stat_core::{Result, StatError};

/// A classification of the observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    /// Name shown in the table's headings.
    pub name: String,
    /// Level labels in display order.
    pub levels: Vec<String>,
    /// Level index of each observation (`None`: missing; the observation is
    /// left out of the table).
    pub codes: Vec<Option<usize>>,
}

/// A continuous measure summarized in the cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    /// Name shown in the table's headings.
    pub name: String,
    /// Value of each observation (NaN: missing).
    pub values: Vec<f64>,
}

/// What a multi-way table classifies and summarizes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TabulationInput {
    /// Row classifications, outermost first.
    pub rows: Vec<Classification>,
    /// Column classifications, outermost first.
    pub columns: Vec<Classification>,
    /// Measures summarized in every cell.
    pub measures: Vec<Measure>,
    /// Frequency weight of each observation (`None`: 1 each).
    pub weights: Option<Vec<f64>>,
}

/// Which totals a multi-way table adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TabulationOptions {
    /// A grand-total row and column.
    pub margins: bool,
    /// Subtotals of each outer level over its inner levels.
    pub subtotals: bool,
}

/// A row or column group: one entry per classification, `None` where the
/// group spans every level of that classification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupKey(pub Vec<Option<usize>>);

impl GroupKey {
    /// `true` for a subtotal or grand total.
    #[must_use]
    pub fn is_total(&self) -> bool {
        self.0.iter().any(Option::is_none)
    }

    /// Whether an observation with full key `codes` belongs to this group.
    fn contains(&self, codes: &[usize]) -> bool {
        self.0.iter().zip(codes).all(|(k, c)| k.is_none_or(|k| k == *c))
    }
}

/// Summary statistics of one measure in one cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasureSummary {
    /// Number (total weight) of non-missing values.
    pub n: f64,
    /// Weighted sum.
    pub sum: f64,
    /// Weighted mean (NaN when `n = 0`).
    pub mean: f64,
    /// Standard deviation (NaN when `n ≤ 1`).
    pub std_dev: f64,
    /// Median (NaN when `n = 0`).
    pub median: f64,
    /// Smallest value (NaN when `n = 0`).
    pub min: f64,
    /// Largest value (NaN when `n = 0`).
    pub max: f64,
}

impl MeasureSummary {
    /// Summarize `(value, weight)` pairs (weights positive).
    fn of(mut pairs: Vec<(f64, f64)>) -> Self {
        let n: f64 = pairs.iter().map(|p| p.1).sum();
        if pairs.is_empty() || n <= 0.0 {
            return Self {
                n: 0.0,
                sum: 0.0,
                mean: f64::NAN,
                std_dev: f64::NAN,
                median: f64::NAN,
                min: f64::NAN,
                max: f64::NAN,
            };
        }
        let sum: f64 = pairs.iter().map(|(x, w)| x * w).sum();
        let mean = sum / n;
        let ss: f64 = pairs.iter().map(|(x, w)| w * (x - mean) * (x - mean)).sum();
        let std_dev = if n > 1.0 { (ss / (n - 1.0)).sqrt() } else { f64::NAN };
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Weighted median: the first value whose cumulative weight passes half
        // the total, or the mean of two neighbouring values when the
        // cumulative weight lands exactly on the half. For whole-number
        // weights this is the ordinary median of the values repeated by
        // weight; it also holds for fractional weights.
        let half = 0.5 * n;
        let tol = 1e-9 * n;
        let mut cum = 0.0;
        let mut median = pairs[pairs.len() - 1].0;
        for (i, &(x, w)) in pairs.iter().enumerate() {
            cum += w;
            if cum > half + tol {
                median = x;
                break;
            }
            if cum >= half - tol {
                median = pairs.get(i + 1).map_or(x, |next| 0.5 * (x + next.0));
                break;
            }
        }
        Self { n, sum, mean, std_dev, median, min: pairs[0].0, max: pairs[pairs.len() - 1].0 }
    }
}

/// A computed multi-way table.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiwayTable {
    /// Row groups in display order (subtotals after their members, the grand
    /// total last).
    pub row_groups: Vec<GroupKey>,
    /// Column groups in display order.
    pub column_groups: Vec<GroupKey>,
    /// `counts[i][j]`: total weight of row group `i` and column group `j`.
    pub counts: Vec<Vec<f64>>,
    /// `summaries[i][j][m]`: measure `m` in that cell.
    pub summaries: Vec<Vec<Vec<MeasureSummary>>>,
    /// Count of each row group over all columns.
    pub row_totals: Vec<f64>,
    /// Count of each column group over all rows.
    pub column_totals: Vec<f64>,
    /// Total weight of the observations used.
    pub grand_total: f64,
    /// Observations used.
    pub observations: usize,
    /// Observations left out for a missing classification or weight.
    pub skipped: usize,
}

impl MultiwayTable {
    /// Row percentage of a cell (NaN when its row group is empty).
    #[must_use]
    pub fn row_percent(&self, i: usize, j: usize) -> f64 {
        100.0 * self.counts[i][j] / self.row_totals[i]
    }

    /// Column percentage of a cell.
    #[must_use]
    pub fn column_percent(&self, i: usize, j: usize) -> f64 {
        100.0 * self.counts[i][j] / self.column_totals[j]
    }

    /// Percentage of the grand total.
    #[must_use]
    pub fn total_percent(&self, i: usize, j: usize) -> f64 {
        100.0 * self.counts[i][j] / self.grand_total
    }
}

/// `(value, weight)` pairs of one measure in one cell.
type WeightedValues = Vec<(f64, f64)>;

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

/// The groups of one side: the observed full keys, with subtotals and the
/// grand total when asked, in display order.
fn groups(observed: &BTreeSet<Vec<usize>>, depth: usize, options: TabulationOptions) -> Vec<GroupKey> {
    let mut set: BTreeSet<Vec<usize>> = BTreeSet::new();
    // Sort with "all levels" after every level: map None to usize::MAX.
    for key in observed {
        set.insert(key.clone());
        if depth == 0 {
            continue;
        }
        if options.subtotals {
            for k in 1..depth {
                let mut sub = key[..k].to_vec();
                sub.resize(depth, usize::MAX);
                set.insert(sub);
            }
        }
        if options.margins {
            set.insert(vec![usize::MAX; depth]);
        }
    }
    set.into_iter().map(|k| GroupKey(k.into_iter().map(|c| (c != usize::MAX).then_some(c)).collect())).collect()
}

/// Build a multi-way table (see the module notes).
///
/// # Errors
/// No classification at all, a classification, measure or weight vector
/// whose length differs from the others, a code beyond its levels, or no
/// observation with every classification present and a non-negative weight.
pub fn tabulate(input: &TabulationInput, options: TabulationOptions) -> Result<MultiwayTable> {
    let all: Vec<&Classification> = input.rows.iter().chain(&input.columns).collect();
    let Some(first) = all.first() else {
        return Err(invalid("a table needs at least one row or column classification"));
    };
    let n = first.codes.len();
    if all.iter().any(|c| c.codes.len() != n)
        || input.measures.iter().any(|m| m.values.len() != n)
        || input.weights.as_ref().is_some_and(|w| w.len() != n)
    {
        return Err(invalid("every classification, measure and weight needs one entry per observation"));
    }
    if all.iter().any(|c| c.codes.iter().flatten().any(|&k| k >= c.levels.len())) {
        return Err(invalid("a classification code is beyond its list of levels"));
    }
    let (dr, dc) = (input.rows.len(), input.columns.len());
    // Full keys of the usable observations.
    let mut obs: Vec<(usize, Vec<usize>, Vec<usize>, f64)> = Vec::new();
    let mut skipped = 0;
    for r in 0..n {
        let w = input.weights.as_ref().map_or(1.0, |w| w[r]);
        let rk: Option<Vec<usize>> = input.rows.iter().map(|c| c.codes[r]).collect();
        let ck: Option<Vec<usize>> = input.columns.iter().map(|c| c.codes[r]).collect();
        match (rk, ck) {
            (Some(rk), Some(ck)) if w.is_finite() && w >= 0.0 => obs.push((r, rk, ck, w)),
            _ => skipped += 1,
        }
    }
    if obs.is_empty() {
        return Err(invalid("no observation has every classification (and a valid frequency)"));
    }
    let row_keys: BTreeSet<Vec<usize>> = obs.iter().map(|o| o.1.clone()).collect();
    let col_keys: BTreeSet<Vec<usize>> = obs.iter().map(|o| o.2.clone()).collect();
    let row_groups = groups(&row_keys, dr, options);
    let column_groups = groups(&col_keys, dc, options);
    let (nr, nc, nm) = (row_groups.len(), column_groups.len(), input.measures.len());
    let mut counts = vec![vec![0.0; nc]; nr];
    let mut values: Vec<Vec<Vec<WeightedValues>>> = vec![vec![vec![Vec::new(); nm]; nc]; nr];
    let mut row_totals = vec![0.0; nr];
    let mut column_totals = vec![0.0; nc];
    let mut grand_total = 0.0;
    for (r, rk, ck, w) in &obs {
        grand_total += w;
        let ri: Vec<usize> = (0..nr).filter(|&i| row_groups[i].contains(rk)).collect();
        let ci: Vec<usize> = (0..nc).filter(|&j| column_groups[j].contains(ck)).collect();
        for &i in &ri {
            row_totals[i] += w;
        }
        for &j in &ci {
            column_totals[j] += w;
        }
        for &i in &ri {
            for &j in &ci {
                counts[i][j] += w;
                if *w > 0.0 {
                    for (m, measure) in input.measures.iter().enumerate() {
                        let x = measure.values[*r];
                        if x.is_finite() {
                            values[i][j][m].push((x, *w));
                        }
                    }
                }
            }
        }
    }
    let summaries = values
        .into_iter()
        .map(|row| row.into_iter().map(|cell| cell.into_iter().map(MeasureSummary::of).collect()).collect())
        .collect();
    Ok(MultiwayTable {
        row_groups,
        column_groups,
        counts,
        summaries,
        row_totals,
        column_totals,
        grand_total,
        observations: obs.len(),
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str, levels: &[&str], codes: &[usize]) -> Classification {
        Classification {
            name: name.into(),
            levels: levels.iter().map(|s| (*s).to_owned()).collect(),
            codes: codes.iter().map(|&c| Some(c)).collect(),
        }
    }

    /// Eight observations, two nested row classifications and one column
    /// classification; every number checked by hand.
    #[test]
    fn nested_rows_with_subtotals_and_margins() {
        let input = TabulationInput {
            rows: vec![
                class("A", &["a1", "a2"], &[0, 0, 0, 0, 1, 1, 1, 1]),
                class("B", &["b1", "b2"], &[0, 0, 1, 1, 0, 0, 0, 1]),
            ],
            columns: vec![class("Y", &["no", "yes"], &[0, 1, 1, 1, 0, 0, 1, 1])],
            measures: vec![Measure { name: "x".into(), values: vec![1.0, 2.0, 3.0, 5.0, 4.0, 6.0, f64::NAN, 10.0] }],
            weights: None,
        };
        let t = tabulate(&input, TabulationOptions { margins: true, subtotals: true }).unwrap();
        let keys: Vec<Vec<Option<usize>>> = t.row_groups.iter().map(|g| g.0.clone()).collect();
        assert_eq!(
            keys,
            vec![
                vec![Some(0), Some(0)],
                vec![Some(0), Some(1)],
                vec![Some(0), None],
                vec![Some(1), Some(0)],
                vec![Some(1), Some(1)],
                vec![Some(1), None],
                vec![None, None],
            ]
        );
        assert_eq!(t.column_groups.len(), 3);
        // a1·b1: one "no", one "yes".
        assert_eq!(t.counts[0], vec![1.0, 1.0, 2.0]);
        // a2 subtotal: no = 2, yes = 2.
        assert_eq!(t.counts[5], vec![2.0, 2.0, 4.0]);
        assert_eq!(t.counts[6], vec![3.0, 5.0, 8.0]);
        assert_eq!(t.row_percent(0, 1), 50.0);
        assert_eq!(t.column_percent(6, 1), 100.0);
        assert!((t.column_percent(0, 1) - 20.0).abs() < 1e-12);
        assert_eq!(t.total_percent(6, 2), 100.0);
        // a2·b1 "yes" holds only the missing value.
        let s = t.summaries[3][1][0];
        assert_eq!((s.n, s.mean.is_nan()), (0.0, true));
        assert_eq!(t.counts[3][1], 1.0);
        // Grand total of x: 1, 2, 3, 5, 4, 6, 10.
        let g = t.summaries[6][2][0];
        assert_eq!((g.n, g.sum, g.median, g.min, g.max), (7.0, 31.0, 4.0, 1.0, 10.0));
        let mean = 31.0 / 7.0;
        let ss: f64 = [1.0, 2.0, 3.0, 5.0, 4.0, 6.0, 10.0].iter().map(|x: &f64| (x - mean).powi(2)).sum();
        assert!((g.std_dev - (ss / 6.0).sqrt()).abs() < 1e-12);
        assert_eq!((t.observations, t.skipped), (8, 0));
    }

    #[test]
    fn frequency_weights_act_as_repeated_rows() {
        let weighted = TabulationInput {
            rows: vec![class("G", &["g"], &[0, 0, 0])],
            columns: vec![],
            measures: vec![Measure { name: "x".into(), values: vec![1.0, 2.0, 7.0] }],
            weights: Some(vec![2.0, 1.0, 1.0]),
        };
        let repeated = TabulationInput {
            rows: vec![class("G", &["g"], &[0, 0, 0, 0])],
            columns: vec![],
            measures: vec![Measure { name: "x".into(), values: vec![1.0, 1.0, 2.0, 7.0] }],
            weights: None,
        };
        let a = tabulate(&weighted, TabulationOptions::default()).unwrap();
        let b = tabulate(&repeated, TabulationOptions::default()).unwrap();
        assert_eq!(a.counts, b.counts);
        let (x, y) = (a.summaries[0][0][0], b.summaries[0][0][0]);
        assert_eq!((x.n, x.sum, x.median, x.min, x.max), (y.n, y.sum, y.median, y.min, y.max));
        assert_eq!(x.median, 1.5);
        assert!((x.std_dev - y.std_dev).abs() < 1e-12 && (x.mean - y.mean).abs() < 1e-12);
        // No column classification: a single column spanning everything.
        assert_eq!(a.column_groups, vec![GroupKey(vec![])]);
    }

    #[test]
    fn missing_codes_and_bad_input() {
        let mut c = class("A", &["x", "y"], &[0, 1, 1]);
        c.codes[2] = None;
        let t = tabulate(
            &TabulationInput { rows: vec![c.clone()], ..TabulationInput::default() },
            TabulationOptions { margins: true, subtotals: false },
        )
        .unwrap();
        assert_eq!((t.observations, t.skipped, t.grand_total), (2, 1, 2.0));
        assert_eq!(t.row_groups.len(), 3);
        assert!(tabulate(&TabulationInput::default(), TabulationOptions::default()).is_err());
        let short = Measure { name: "m".into(), values: vec![1.0] };
        assert!(
            tabulate(
                &TabulationInput { rows: vec![c.clone()], measures: vec![short], ..TabulationInput::default() },
                TabulationOptions::default()
            )
            .is_err()
        );
        let mut bad = c;
        bad.codes[0] = Some(5);
        assert!(
            tabulate(&TabulationInput { rows: vec![bad], ..TabulationInput::default() }, TabulationOptions::default())
                .is_err()
        );
    }

    /// Fractional weights: the median is the value where the cumulative
    /// weight passes half the total, and scaling every weight leaves it
    /// unchanged (it used to index by whole-number rank, giving the maximum).
    #[test]
    fn weighted_median_with_fractional_weights() {
        let median = |values: &[f64], weights: &[f64]| {
            MeasureSummary::of(values.iter().copied().zip(weights.iter().copied()).collect()).median
        };
        assert_eq!(median(&[10.0, 20.0, 30.0, 40.0, 50.0], &[0.1; 5]), 30.0);
        assert_eq!(median(&[10.0, 20.0], &[0.5, 0.5]), 15.0);
        assert_eq!(median(&[10.0, 20.0], &[1.5, 1.5]), 15.0);
        assert_eq!(median(&[10.0, 20.0, 30.0], &[0.2, 0.5, 0.4]), 20.0);
        assert_eq!(median(&[10.0, 20.0, 30.0], &[2.0, 1.0, 1.0]), 15.0);
        assert_eq!(median(&[10.0, 20.0, 30.0], &[0.2, 0.1, 0.1]), 15.0);
        assert_eq!(median(&[1.0, 2.0, 7.0], &[1.0, 1.0, 1.0]), 2.0);
    }
}
