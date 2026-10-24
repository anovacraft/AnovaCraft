//! Ranks with a choice of rule for tied values.
//!
//! The values are put in ascending order (missing values, NaN, get no rank)
//! and numbered 1, 2, …, n. A run of equal values spanning positions
//! `i + 1, …, j + 1` is ranked by the [`TieRule`]:
//!
//! | rule | rank of every value in the run |
//! |---|---|
//! | average | `(i + j)/2 + 1`, the mean of the positions |
//! | minimum | `i + 1` (competition ranking, "1 2 2 4") |
//! | maximum | `j + 1` ("1 3 3 4") |
//! | dense | one more than the number of distinct smaller values ("1 2 2 3") |
//! | first | the positions themselves, in row order ("1 2 3 4") |
//!
//! These are the standard definitions (Kendall & Gibbons, *Rank Correlation
//! Methods*, 5th ed., §3.1, for mid-ranks; the others as in common
//! statistical software).

/// How tied values are ranked (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TieRule {
    /// The mean of the positions the run spans (mid-ranks).
    #[default]
    Average,
    /// The lowest position of the run.
    Minimum,
    /// The highest position of the run.
    Maximum,
    /// Consecutive integers for distinct values, with no gaps.
    Dense,
    /// Distinct ranks in row order.
    First,
}

impl TieRule {
    /// Every rule, in menu order.
    pub const ALL: [Self; 5] = [Self::Average, Self::Minimum, Self::Maximum, Self::Dense, Self::First];

    /// The key used in formulas and scripts.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Average => "average",
            Self::Minimum => "min",
            Self::Maximum => "max",
            Self::Dense => "dense",
            Self::First => "first",
        }
    }

    /// The rule named by `key` (also `"mean"`, `"minimum"`, `"maximum"`,
    /// `"ordinal"`), ignoring case.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "average" | "mean" => Some(Self::Average),
            "min" | "minimum" => Some(Self::Minimum),
            "max" | "maximum" => Some(Self::Maximum),
            "dense" => Some(Self::Dense),
            "first" | "ordinal" => Some(Self::First),
            _ => None,
        }
    }

    /// Short description for menus.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Average => "Average (ties share the mean rank)",
            Self::Minimum => "Minimum (1, 2, 2, 4)",
            Self::Maximum => "Maximum (1, 3, 3, 4)",
            Self::Dense => "Dense (1, 2, 2, 3)",
            Self::First => "First (ties in row order)",
        }
    }
}

/// Ranks of `x` from 1 under `rule`; NaN values get NaN and are not counted.
#[must_use]
pub fn ranks_with_ties(x: &[f64], rule: TieRule) -> Vec<f64> {
    let mut order: Vec<usize> = (0..x.len()).filter(|&i| !x[i].is_nan()).collect();
    // A stable sort keeps tied values in row order (for `First`).
    order.sort_by(|&a, &b| x[a].total_cmp(&x[b]));
    let mut ranks = vec![f64::NAN; x.len()];
    let (mut i, mut distinct) = (0, 0usize);
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && x[order[j + 1]] == x[order[i]] {
            j += 1;
        }
        distinct += 1;
        for (offset, &k) in order[i..=j].iter().enumerate() {
            ranks[k] = match rule {
                TieRule::Average => (i + j) as f64 / 2.0 + 1.0,
                TieRule::Minimum => (i + 1) as f64,
                TieRule::Maximum => (j + 1) as f64,
                TieRule::Dense => distinct as f64,
                TieRule::First => (i + offset + 1) as f64,
            };
        }
        i = j + 1;
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected ranks from pandas 3.0 `Series.rank(method=…)` on
    /// `[3, 1, 4, 1, 5, 9, 2, 6, 5, 3, NaN, 5]` (NaN kept as NaN).
    #[test]
    fn every_rule_matches_pandas() {
        let x = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0, 5.0, 3.0, f64::NAN, 5.0];
        let expect: [(TieRule, [f64; 11]); 5] = [
            (TieRule::Average, [4.5, 1.5, 6.0, 1.5, 8.0, 11.0, 3.0, 10.0, 8.0, 4.5, 8.0]),
            (TieRule::Minimum, [4.0, 1.0, 6.0, 1.0, 7.0, 11.0, 3.0, 10.0, 7.0, 4.0, 7.0]),
            (TieRule::Maximum, [5.0, 2.0, 6.0, 2.0, 9.0, 11.0, 3.0, 10.0, 9.0, 5.0, 9.0]),
            (TieRule::Dense, [3.0, 1.0, 4.0, 1.0, 5.0, 7.0, 2.0, 6.0, 5.0, 3.0, 5.0]),
            (TieRule::First, [4.0, 1.0, 6.0, 2.0, 7.0, 11.0, 3.0, 10.0, 8.0, 5.0, 9.0]),
        ];
        for (rule, want) in expect {
            let got = ranks_with_ties(&x, rule);
            assert!(got[10].is_nan(), "{rule:?}");
            let got: Vec<f64> = got.iter().copied().filter(|v| !v.is_nan()).collect();
            assert_eq!(got, want, "{rule:?}");
        }
    }

    #[test]
    fn keys_round_trip() {
        for rule in TieRule::ALL {
            assert_eq!(TieRule::from_key(rule.key()), Some(rule));
        }
        assert_eq!(TieRule::from_key(" Dense "), Some(TieRule::Dense));
        assert_eq!(TieRule::from_key("ordinal"), Some(TieRule::First));
        assert_eq!(TieRule::from_key("random"), None);
        assert!(ranks_with_ties(&[], TieRule::Dense).is_empty());
    }
}
