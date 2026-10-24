//! Multiple-response (check-all-that-apply) questions: each case may pick
//! any number of `I` items, recorded as a cases × items matrix of picks.
//!
//! **Frequencies.** With `n` respondents (cases that picked at least one
//! item) and `cᵢ` the number of cases picking item `i`, the report gives
//! `cᵢ`, the percentage of cases `100 cᵢ / n` (these add to more than 100%
//! when cases pick several items) and the percentage of responses
//! `100 cᵢ / Σⱼ cⱼ` (these add to 100%). Cases with no pick are counted
//! separately and left out of the percentages, as is usual for these tables.
//!
//! **Cross-tabulation by a group.** For each group `g` with `n_g`
//! respondents, `c_{gi}` cases picking item `i` and the percentage of the
//! group's cases `100 c_{gi} / n_g`.
//!
//! **Tests of marginal independence.** A case can count under several items,
//! so the items × groups table of counts is not a contingency table and the
//! ordinary χ² test does not apply to it (Agresti & Liu 1999). Each item on
//! its own is a 2 × G table (picked or not, by group), tested with Pearson's
//! χ² on `G − 1` degrees of freedom; the overall hypothesis of simultaneous
//! marginal independence of every item from the group is tested with the
//! Bonferroni-adjusted smallest p-value, `min(1, I · minᵢ pᵢ)` (Bilder &
//! Loughin 2004, *Biometrics* 60).
//!
//! References: Agresti, A. & Liu, I.-M. (1999), "Modeling a categorical
//! variable allowing arbitrarily many category choices", *Biometrics* 55;
//! Bilder, C. R. & Loughin, T. M. (2004), "Testing for marginal independence
//! between two categorical variables with multiple responses", *Biometrics* 60.

use stat_core::{Result, StatError};

use crate::{ContingencyTable, test_independence};

/// Frequency of one item.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemFrequency {
    /// The item.
    pub item: String,
    /// Cases that picked it.
    pub count: usize,
    /// Percentage of respondents that picked it.
    pub percent_of_cases: f64,
    /// Its share of all picks, in percent.
    pub percent_of_responses: f64,
}

/// One group's counts in a cross-tabulation.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupCounts {
    /// The group's label.
    pub group: String,
    /// Respondents in the group.
    pub respondents: usize,
    /// Cases picking each item, in item order.
    pub counts: Vec<usize>,
    /// Percentage of the group's respondents picking each item.
    pub percents: Vec<f64>,
}

/// Test of one item's independence from the group.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemTest {
    /// The item.
    pub item: String,
    /// Pearson's χ² of the 2 × G table.
    pub chi_square: f64,
    /// Degrees of freedom (`G − 1`, fewer if a group has no respondent).
    pub df: usize,
    /// Its p-value.
    pub p_value: f64,
    /// Bonferroni-adjusted p-value, `min(1, I · p)`.
    pub adjusted_p_value: f64,
    /// The χ² approximation is doubtful (Cochran's guideline).
    pub small_expected: bool,
}

/// Frequencies of a multiple-response question.
#[derive(Debug, Clone, PartialEq)]
pub struct MultipleResponseTable {
    /// Cases given (rows of the matrix).
    pub cases: usize,
    /// Cases with at least one pick.
    pub respondents: usize,
    /// Picks in all.
    pub responses: usize,
    /// One entry per item, in the order given.
    pub items: Vec<ItemFrequency>,
}

impl MultipleResponseTable {
    /// Mean number of picks per respondent.
    #[must_use]
    pub fn mean_per_respondent(&self) -> f64 {
        if self.respondents == 0 { f64::NAN } else { self.responses as f64 / self.respondents as f64 }
    }
}

/// Cross-tabulation of a multiple-response question by a group.
#[derive(Debug, Clone, PartialEq)]
pub struct MultipleResponseCrossTab {
    /// The items, in order.
    pub items: Vec<String>,
    /// One entry per group, in the order given.
    pub groups: Vec<GroupCounts>,
    /// Each item's test (`None` when there are fewer than two groups with
    /// respondents or an item was picked by every respondent or none).
    pub tests: Vec<Option<ItemTest>>,
    /// The overall Bonferroni test of simultaneous marginal independence:
    /// `min(1, I · min p)` over the items that could be tested.
    pub overall_p_value: Option<f64>,
}

fn check(picks: &[Vec<bool>], items: &[String]) -> Result<()> {
    if items.is_empty() {
        return Err(StatError::InvalidArgument { message: "a multiple-response question needs items".into() });
    }
    if picks.iter().any(|row| row.len() != items.len()) {
        return Err(StatError::InvalidArgument { message: "every case needs one pick flag per item".into() });
    }
    Ok(())
}

/// Frequencies of items picked: `picks[case][item]`.
///
/// # Errors
/// [`StatError::InvalidArgument`] if there are no items, a row has the wrong
/// length, or no case picked anything.
pub fn multiple_response_frequencies(picks: &[Vec<bool>], items: &[String]) -> Result<MultipleResponseTable> {
    check(picks, items)?;
    let respondents = picks.iter().filter(|r| r.iter().any(|&p| p)).count();
    if respondents == 0 {
        return Err(StatError::InvalidArgument { message: "no case picked any item".into() });
    }
    let counts: Vec<usize> = (0..items.len()).map(|i| picks.iter().filter(|r| r[i]).count()).collect();
    let responses: usize = counts.iter().sum();
    let items = items
        .iter()
        .zip(&counts)
        .map(|(item, &count)| ItemFrequency {
            item: item.clone(),
            count,
            percent_of_cases: 100.0 * count as f64 / respondents as f64,
            percent_of_responses: 100.0 * count as f64 / responses as f64,
        })
        .collect();
    Ok(MultipleResponseTable { cases: picks.len(), respondents, responses, items })
}

/// Cross-tabulate picks by `group` (one label per case; `None` leaves the
/// case out), with each item's test of independence from the group.
/// Groups are listed in order of first appearance; cases with no pick are
/// not respondents and are left out.
///
/// # Errors
/// As [`multiple_response_frequencies`], or `group` of the wrong length.
pub fn multiple_response_crosstab(
    picks: &[Vec<bool>],
    items: &[String],
    group: &[Option<String>],
) -> Result<MultipleResponseCrossTab> {
    check(picks, items)?;
    if group.len() != picks.len() {
        return Err(StatError::InvalidArgument { message: "one group label is needed per case".into() });
    }
    let mut labels: Vec<String> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (case, g) in group.iter().enumerate() {
        let Some(g) = g else { continue };
        if !picks[case].iter().any(|&p| p) {
            continue;
        }
        match labels.iter().position(|l| l == g) {
            Some(k) => members[k].push(case),
            None => {
                labels.push(g.clone());
                members.push(vec![case]);
            }
        }
    }
    if members.is_empty() {
        return Err(StatError::InvalidArgument { message: "no case with a group picked any item".into() });
    }
    let groups: Vec<GroupCounts> = labels
        .iter()
        .zip(&members)
        .map(|(label, cases)| {
            let counts: Vec<usize> = (0..items.len()).map(|i| cases.iter().filter(|&&c| picks[c][i]).count()).collect();
            let percents = counts.iter().map(|&c| 100.0 * c as f64 / cases.len() as f64).collect();
            GroupCounts { group: label.clone(), respondents: cases.len(), counts, percents }
        })
        .collect();
    let mut tests: Vec<Option<ItemTest>> = (0..items.len())
        .map(|i| {
            let table = ContingencyTable {
                row_labels: vec!["picked".into(), "not picked".into()],
                column_labels: labels.clone(),
                counts: vec![
                    groups.iter().map(|g| g.counts[i] as u64).collect(),
                    groups.iter().map(|g| (g.respondents - g.counts[i]) as u64).collect(),
                ],
            };
            test_independence(&table).ok().map(|t| ItemTest {
                item: items[i].clone(),
                chi_square: t.pearson_chi_square,
                df: t.df,
                p_value: t.pearson_p_value,
                adjusted_p_value: 0.0,
                small_expected: t.small_expected_counts(),
            })
        })
        .collect();
    let m = items.len() as f64;
    for t in tests.iter_mut().flatten() {
        t.adjusted_p_value = (m * t.p_value).min(1.0);
    }
    let overall_p_value = tests.iter().flatten().map(|t| t.p_value).reduce(f64::min).map(|p| (m * p).min(1.0));
    Ok(MultipleResponseCrossTab { items: items.to_vec(), groups, tests, overall_p_value })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Six cases picking from three items, worked by hand.
    fn picks() -> Vec<Vec<bool>> {
        vec![
            vec![true, true, false],
            vec![true, false, false],
            vec![false, false, false],
            vec![true, false, true],
            vec![false, true, true],
            vec![true, true, true],
        ]
    }

    #[test]
    fn frequencies_match_hand_counts() {
        let t = multiple_response_frequencies(&picks(), &names(&["A", "B", "C"])).unwrap();
        assert_eq!((t.cases, t.respondents, t.responses), (6, 5, 10));
        let counts: Vec<usize> = t.items.iter().map(|i| i.count).collect();
        assert_eq!(counts, [4, 3, 3]);
        assert!((t.items[0].percent_of_cases - 80.0).abs() < 1e-12);
        assert!((t.items[1].percent_of_responses - 30.0).abs() < 1e-12);
        let total: f64 = t.items.iter().map(|i| i.percent_of_responses).sum();
        assert!((total - 100.0).abs() < 1e-12);
        assert!((t.mean_per_respondent() - 2.0).abs() < 1e-12);
    }

    #[test]
    fn crosstab_counts_and_tests_match_hand_calculation() {
        let group: Vec<Option<String>> = ["x", "x", "x", "y", "y", "y"].iter().map(|s| Some((*s).to_owned())).collect();
        let c = multiple_response_crosstab(&picks(), &names(&["A", "B", "C"]), &group).unwrap();
        // Case 3 picked nothing: x has 2 respondents, y has 3.
        assert_eq!(c.groups[0].respondents, 2);
        assert_eq!(c.groups[0].counts, [2, 1, 0]);
        assert_eq!(c.groups[1].counts, [2, 2, 3]);
        assert!((c.groups[1].percents[1] - 200.0 / 3.0).abs() < 1e-12);
        // Item C: picked 0 of 2 in x, 3 of 3 in y — a 2 × 2 table [[0, 3], [2, 0]]
        // with margins 3, 2 / 2, 3 and n = 5: X² = n (ad − bc)² / (r1 r2 c1 c2) = 5 · 36 / 36 = 5.
        let t = c.tests[2].as_ref().unwrap();
        assert!((t.chi_square - 5.0).abs() < 1e-12);
        assert_eq!(t.df, 1);
        let p = 0.025_347_318_677_468_25; // P(χ²₁ > 5), SciPy chi2.sf(5, 1)
        assert!((t.p_value - p).abs() < 1e-12);
        assert!((t.adjusted_p_value - 3.0 * p).abs() < 1e-12);
        // Item A: 2 of 2 and 2 of 3: [[2, 2], [0, 1]], X² = 5 (2·1 − 2·0)² / (4 · 1 · 2 · 3) = 20 / 24.
        let a = c.tests[0].as_ref().unwrap();
        assert!((a.chi_square - 20.0 / 24.0).abs() < 1e-12);
        assert!((c.overall_p_value.unwrap() - 3.0 * p).abs() < 1e-12);
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(multiple_response_frequencies(&[vec![false, false]], &names(&["A", "B"])).is_err());
        assert!(multiple_response_frequencies(&[vec![true]], &names(&["A", "B"])).is_err());
        assert!(multiple_response_frequencies(&[], &[]).is_err());
        assert!(multiple_response_crosstab(&picks(), &names(&["A", "B", "C"]), &[None]).is_err());
        // One group only: counts, but no tests.
        let one = vec![Some("x".to_owned()); 6];
        let c = multiple_response_crosstab(&picks(), &names(&["A", "B", "C"]), &one).unwrap();
        assert!(c.tests.iter().all(Option::is_none) && c.overall_p_value.is_none());
    }
}
