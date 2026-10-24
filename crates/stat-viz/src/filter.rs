//! A graph's own row filter: conditions on columns (chosen levels of a
//! categorical column, an interval of a continuous one) that restrict what
//! one graph shows or analyses without changing the shared row states.
//!
//! What happens to rows outside the filter is chosen separately:
//! * **show only matching rows** — the others are not drawn;
//! * **analyse only matching rows** — fits, smoothers and summaries use the
//!   matching rows only (rows still drawn are marked as left out);
//! * **select matching rows** — the matching rows become the shared selection,
//!   so every linked view highlights them (applied by the application when
//!   the filter changes).

use stat_data::DataTable;

use crate::encoding::is_continuous;

/// One condition of a filter.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterCondition {
    /// The row's level of `column` is one of `levels` (cell text).
    Levels {
        /// Column name.
        column: String,
        /// Accepted levels.
        levels: Vec<String>,
    },
    /// The row's value of `column` lies in `[min, max]`.
    Range {
        /// Column name.
        column: String,
        /// Lower limit (inclusive).
        min: f64,
        /// Upper limit (inclusive).
        max: f64,
    },
}

impl FilterCondition {
    /// A condition on `column` that every row with a value passes: all its
    /// levels, or its whole range. `None` if the column is missing.
    #[must_use]
    pub fn for_column(table: &DataTable, column: &str) -> Option<Self> {
        let col = table.find(column).and_then(|i| table.column(i))?;
        if is_continuous(table, column) {
            let values = col.as_numeric().unwrap_or(&[]);
            let (min, max) = values
                .iter()
                .filter(|v| v.is_finite())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
            let (min, max) = if min.is_finite() { (min, max) } else { (0.0, 0.0) };
            Some(Self::Range { column: column.to_owned(), min, max })
        } else {
            Some(Self::Levels { column: column.to_owned(), levels: column_levels(table, column) })
        }
    }

    /// Column the condition tests.
    #[must_use]
    pub fn column(&self) -> &str {
        match self {
            Self::Levels { column, .. } | Self::Range { column, .. } => column,
        }
    }

    fn column_mut(&mut self) -> &mut String {
        match self {
            Self::Levels { column, .. } | Self::Range { column, .. } => column,
        }
    }

    /// Whether each row of `table` passes (rows of a missing column fail).
    #[must_use]
    pub fn matches(&self, table: &DataTable) -> Vec<bool> {
        let n = table.n_rows();
        let Some(col) = table.find(self.column()).and_then(|i| table.column(i)) else {
            return vec![false; n];
        };
        match self {
            Self::Levels { levels, .. } => (0..n).map(|r| levels.contains(&col.level_text(r))).collect(),
            Self::Range { min, max, .. } => {
                let values = col.as_numeric().unwrap_or(&[]);
                (0..n).map(|r| values.get(r).is_some_and(|v| v.is_finite() && *v >= *min && *v <= *max)).collect()
            }
        }
    }

    /// The condition moved `k` steps through its column, for stepping
    /// through levels one at a time ("animating" a filter).
    ///
    /// A level condition holding exactly one of `levels` moves to the level
    /// `k` places on, wrapping round; any other choice of levels starts at
    /// the first level (`k > 0`) or the last (`k < 0`). A range condition
    /// is a moving window over `range` (the column's smallest and largest
    /// values): it keeps its width and moves by that width, wrapping round
    /// at either end; a window that is empty or spans the whole range
    /// starts as the first (or last) tenth. A zero step, an empty level
    /// list or a range of no width leave the condition as it is. Windows
    /// share their end points, so a value exactly on a boundary is in two
    /// adjacent windows.
    #[must_use]
    pub fn stepped(&self, levels: &[String], range: (f64, f64), k: isize) -> Self {
        let mut out = self.clone();
        if k == 0 {
            return out;
        }
        match &mut out {
            Self::Levels { levels: chosen, .. } => {
                let n = levels.len();
                if n == 0 {
                    return out;
                }
                let at = match chosen.as_slice() {
                    [one] => levels.iter().position(|l| l == one),
                    _ => None,
                };
                let i = match at {
                    Some(i) => (i as isize + k).rem_euclid(n as isize) as usize,
                    None if k > 0 => 0,
                    None => n - 1,
                };
                *chosen = vec![levels[i].clone()];
            }
            Self::Range { min, max, .. } => {
                let (lo, hi) = range;
                let span = hi - lo;
                if !(span.is_finite() && span > 0.0) {
                    return out;
                }
                let width = *max - *min;
                if !(width > 0.0 && width < span) {
                    let w = span / 10.0;
                    (*min, *max) = if k > 0 { (lo, lo + w) } else { (hi - w, hi) };
                    return out;
                }
                let start = *min + k as f64 * width;
                (*min, *max) = if start > hi - 1e-12 * span {
                    (lo, lo + width)
                } else if start + width < lo + 1e-12 * span {
                    (hi - width, hi)
                } else {
                    (start, start + width)
                };
            }
        }
        out
    }

    /// Where a stepped condition stands: its 1-based position and the
    /// number of positions (one per level, or windows of its width across
    /// `range`). `None` when it is not one step (several levels, or a
    /// window spanning the whole range).
    #[must_use]
    pub fn step_position(&self, levels: &[String], range: (f64, f64)) -> Option<(usize, usize)> {
        match self {
            Self::Levels { levels: chosen, .. } => match chosen.as_slice() {
                [one] => levels.iter().position(|l| l == one).map(|i| (i + 1, levels.len())),
                _ => None,
            },
            Self::Range { min, max, .. } => {
                let (lo, hi) = range;
                let (span, width) = (hi - lo, max - min);
                if !(span > 0.0 && width > 0.0 && width < span) {
                    return None;
                }
                let n = (span / width - 1e-9).ceil().max(1.0) as usize;
                let i = ((min - lo) / width + 1e-9).floor().clamp(0.0, (n - 1) as f64) as usize;
                Some((i + 1, n))
            }
        }
    }

    /// A short description, e.g. `Region ∈ {East, West}` or `2 ≤ Age ≤ 9`.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Levels { column, levels } => format!("{column} ∈ {{{}}}", levels.join(", ")),
            Self::Range { column, min, max } => {
                format!("{} ≤ {column} ≤ {}", crate::view::format_value(*min), crate::view::format_value(*max))
            }
        }
    }
}

/// The distinct non-empty cell texts of `column`, in natural order.
#[must_use]
pub fn column_levels(table: &DataTable, column: &str) -> Vec<String> {
    let Some(col) = table.find(column).and_then(|i| table.column(i)) else {
        return Vec::new();
    };
    let mut levels: Vec<String> =
        (0..table.n_rows()).map(|r| col.level_text(r)).filter(|l| !l.trim().is_empty()).collect();
    levels.sort_by(|a, b| stat_linear::natural_order(a, b));
    levels.dedup();
    levels
}

/// A graph's row filter: all conditions must hold.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalFilter {
    /// Conditions, all of which a row must pass.
    pub conditions: Vec<FilterCondition>,
    /// Draw only the matching rows.
    pub show_only: bool,
    /// Compute fits, smoothers and summaries from the matching rows only.
    pub include_only: bool,
    /// Make the matching rows the shared selection whenever the filter changes.
    pub select: bool,
}

impl Default for LocalFilter {
    fn default() -> Self {
        Self { conditions: Vec::new(), show_only: true, include_only: true, select: false }
    }
}

impl LocalFilter {
    /// `true` when there is at least one condition.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.conditions.is_empty()
    }

    /// Whether each row passes every condition (all rows without conditions).
    #[must_use]
    pub fn matches(&self, table: &DataTable) -> Vec<bool> {
        let mut out = vec![true; table.n_rows()];
        for c in &self.conditions {
            for (o, m) in out.iter_mut().zip(c.matches(table)) {
                *o &= m;
            }
        }
        out
    }

    /// Rows passing the filter, ascending.
    #[must_use]
    pub fn matching_rows(&self, table: &DataTable) -> Vec<usize> {
        self.matches(table).iter().enumerate().filter(|(_, m)| **m).map(|(r, _)| r).collect()
    }

    /// Columns the conditions use.
    #[must_use]
    pub fn columns(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for c in &self.conditions {
            if !out.contains(&c.column()) {
                out.push(c.column());
            }
        }
        out
    }

    /// Follow a column renamed from `old` to `new`.
    pub fn rename(&mut self, old: &str, new: &str) {
        for c in &mut self.conditions {
            if c.column() == old {
                *c.column_mut() = new.to_owned();
            }
        }
    }

    /// The conditions joined with "and" (empty without conditions).
    #[must_use]
    pub fn describe(&self) -> String {
        self.conditions.iter().map(FilterCondition::describe).collect::<Vec<_>>().join(" and ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.0, 2.0, f64::NAN, 4.0, 5.0]),
                Column::text("g", vec!["b".into(), "a".into(), "b".into(), "".into(), "c".into()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn conditions_and_filters() {
        let t = table();
        let Some(FilterCondition::Range { min, max, .. }) = FilterCondition::for_column(&t, "x") else {
            panic!("x is continuous")
        };
        assert_eq!((min, max), (1.0, 5.0));
        let Some(FilterCondition::Levels { levels, .. }) = FilterCondition::for_column(&t, "g") else {
            panic!("g is categorical")
        };
        assert_eq!(levels, ["a", "b", "c"]);
        assert!(FilterCondition::for_column(&t, "nope").is_none());

        let range = FilterCondition::Range { column: "x".into(), min: 2.0, max: 4.5 };
        assert_eq!(range.matches(&t), [false, true, false, true, false]);
        let levels = FilterCondition::Levels { column: "g".into(), levels: vec!["b".into()] };
        assert_eq!(levels.matches(&t), [true, false, true, false, false]);

        let mut f = LocalFilter::default();
        assert!(!f.is_active() && f.matches(&t).iter().all(|m| *m));
        f.conditions = vec![range, FilterCondition::Levels { column: "g".into(), levels: vec!["a".into(), "".into()] }];
        assert_eq!(f.matching_rows(&t), [1, 3]);
        assert_eq!(f.columns(), ["x", "g"]);
        assert_eq!(f.describe(), "2 ≤ x ≤ 4.5 and g ∈ {a, }");
        f.rename("g", "h");
        assert_eq!(f.columns(), ["x", "h"]);
        assert_eq!(f.matching_rows(&t), Vec::<usize>::new(), "a missing column matches nothing");
    }

    #[test]
    fn stepping_through_levels_and_windows() {
        let levels: Vec<String> = ["a", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
        let all = FilterCondition::Levels { column: "g".into(), levels: levels.clone() };
        let one = |l: &str| FilterCondition::Levels { column: "g".into(), levels: vec![l.into()] };
        // Several levels start at the first (forward) or last (back).
        assert_eq!(all.stepped(&levels, (0.0, 1.0), 1), one("a"));
        assert_eq!(all.stepped(&levels, (0.0, 1.0), -1), one("c"));
        assert_eq!(all.step_position(&levels, (0.0, 1.0)), None);
        // One level moves on, wrapping round.
        assert_eq!(one("a").stepped(&levels, (0.0, 1.0), 1), one("b"));
        assert_eq!(one("c").stepped(&levels, (0.0, 1.0), 1), one("a"));
        assert_eq!(one("a").stepped(&levels, (0.0, 1.0), -1), one("c"));
        assert_eq!(one("b").step_position(&levels, (0.0, 1.0)), Some((2, 3)));
        assert_eq!(one("b").stepped(&levels, (0.0, 1.0), 0), one("b"));
        assert_eq!(all.stepped(&[], (0.0, 1.0), 1), all, "no levels: unchanged");

        let range = |min: f64, max: f64| FilterCondition::Range { column: "x".into(), min, max };
        let r = |c: FilterCondition| match c {
            FilterCondition::Range { min, max, .. } => (min, max),
            FilterCondition::Levels { .. } => panic!("a range"),
        };
        let close = |(a, b): (f64, f64), (c, d): (f64, f64)| (a - c).abs() < 1e-12 && (b - d).abs() < 1e-12;
        // The whole range starts as the first or last tenth.
        assert!(close(r(range(0.0, 50.0).stepped(&[], (0.0, 50.0), 1)), (0.0, 5.0)));
        assert!(close(r(range(0.0, 50.0).stepped(&[], (0.0, 50.0), -1)), (45.0, 50.0)));
        assert_eq!(range(0.0, 50.0).step_position(&[], (0.0, 50.0)), None);
        // A window keeps its width and wraps at either end.
        assert!(close(r(range(10.0, 20.0).stepped(&[], (0.0, 50.0), 1)), (20.0, 30.0)));
        assert!(close(r(range(40.0, 50.0).stepped(&[], (0.0, 50.0), 1)), (0.0, 10.0)));
        assert!(close(r(range(0.0, 10.0).stepped(&[], (0.0, 50.0), -1)), (40.0, 50.0)));
        assert!(close(r(range(35.0, 45.0).stepped(&[], (0.0, 50.0), 1)), (45.0, 55.0)), "the last window may overhang");
        assert_eq!(range(20.0, 30.0).step_position(&[], (0.0, 50.0)), Some((3, 5)));
        assert_eq!(range(45.0, 55.0).step_position(&[], (0.0, 50.0)), Some((5, 5)));
        assert_eq!(range(1.0, 2.0).stepped(&[], (3.0, 3.0), 1), range(1.0, 2.0), "a range of no width: unchanged");
    }
}
