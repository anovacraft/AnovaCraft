//! Small charts of a column's values for the data grid's headings: a
//! histogram of a continuous column, the level counts of a categorical one,
//! or the values in row order for a date-time column, over the rows
//! analyses use (excluded rows and rows outside the active subset are left
//! out), with the number of missing values. Each bar knows its rows, so a
//! click on it can select them (linked brushing).

use stat_data::{Column, ColumnData, MeasurementLevel, RowStateManager, natural_order};

/// Most bars a histogram heading shows.
const BINS: usize = 12;
/// Most levels shown; the rest are pooled into one "other levels" bar.
const LEVELS: usize = 8;
/// Most points of a row-order line (longer columns are thinned evenly).
const LINE_POINTS: usize = 200;

/// What a heading chart draws.
#[derive(Debug, Clone, PartialEq)]
pub enum HeadingChart {
    /// Equal-width bins from `min` to `max`: rows per bin.
    Histogram {
        /// Lower edge of the first bin.
        min: f64,
        /// Upper edge of the last bin.
        max: f64,
        /// Rows per bin.
        counts: Vec<usize>,
    },
    /// Rows per level in natural order: `(level, rows)`. With more than
    /// [`LEVELS`] levels, the most frequent are kept and the last entry
    /// pools the rest ("12 other levels").
    Levels {
        /// `(level, rows)`.
        levels: Vec<(String, usize)>,
        /// Whether the last entry pools several levels.
        pooled: bool,
    },
    /// The values in row order (thinned to at most [`LINE_POINTS`]) and
    /// their range.
    Line {
        /// `(row, value)` pairs.
        points: Vec<(usize, f64)>,
        /// Smallest value.
        min: f64,
        /// Largest value.
        max: f64,
    },
    /// No values to chart.
    Empty,
}

/// A heading chart with its missing count.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadingSummary {
    /// The chart.
    pub chart: HeadingChart,
    /// Missing values among the rows charted.
    pub missing: usize,
}

/// Bin of `x` in a histogram from `min` to `max` with `n` bins.
fn bin_of(x: f64, min: f64, max: f64, n: usize) -> usize {
    if max <= min {
        return 0;
    }
    (((x - min) / (max - min) * n as f64) as usize).min(n - 1)
}

/// The heading chart of `col` over the rows analyses use.
#[must_use]
pub fn summarize(col: &Column, rows: &RowStateManager) -> HeadingSummary {
    let n = col.data.len();
    let used: Vec<usize> = (0..n).filter(|&r| r >= rows.len() || !rows.is_excluded(r)).collect();
    match (&col.data, col.level) {
        (ColumnData::DateTime { values, .. }, _) => {
            let finite: Vec<(usize, f64)> =
                used.iter().map(|&r| (r, values[r])).filter(|(_, v)| v.is_finite()).collect();
            let missing = used.len() - finite.len();
            if finite.is_empty() {
                return HeadingSummary { chart: HeadingChart::Empty, missing };
            }
            let (min, max) =
                finite.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.1), b.max(p.1)));
            let step = finite.len().div_ceil(LINE_POINTS).max(1);
            let points = finite.into_iter().step_by(step).collect();
            HeadingSummary { chart: HeadingChart::Line { points, min, max }, missing }
        }
        (ColumnData::Numeric(values), MeasurementLevel::Continuous) => {
            let finite: Vec<f64> = used.iter().map(|&r| values[r]).filter(|v| v.is_finite()).collect();
            let missing = used.len() - finite.len();
            if finite.is_empty() {
                return HeadingSummary { chart: HeadingChart::Empty, missing };
            }
            let (min, max) = finite.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
            let bins = if max > min { BINS } else { 1 };
            let mut counts = vec![0; bins];
            for v in finite {
                counts[bin_of(v, min, max, bins)] += 1;
            }
            HeadingSummary { chart: HeadingChart::Histogram { min, max, counts }, missing }
        }
        _ => {
            let mut levels: Vec<(String, usize)> = Vec::new();
            let mut missing = 0;
            for &r in &used {
                let t = col.level_text(r);
                if t.trim().is_empty() {
                    missing += 1;
                    continue;
                }
                match levels.iter_mut().find(|(l, _)| *l == t) {
                    Some((_, c)) => *c += 1,
                    None => levels.push((t, 1)),
                }
            }
            if levels.is_empty() {
                return HeadingSummary { chart: HeadingChart::Empty, missing };
            }
            levels.sort_by(|a, b| natural_order(&a.0, &b.0));
            let pooled = levels.len() > LEVELS;
            if pooled {
                // Keep the most frequent levels (in their natural order) and pool the rest.
                let mut by_count: Vec<usize> = (0..levels.len()).collect();
                by_count.sort_by(|&a, &b| levels[b].1.cmp(&levels[a].1).then(a.cmp(&b)));
                let keep: Vec<usize> = by_count[..LEVELS - 1].to_vec();
                let other: usize = (0..levels.len()).filter(|i| !keep.contains(i)).map(|i| levels[i].1).sum();
                let n_other = levels.len() - keep.len();
                let mut kept: Vec<(String, usize)> =
                    (0..levels.len()).filter(|i| keep.contains(i)).map(|i| levels[i].clone()).collect();
                kept.push((format!("{n_other} other levels"), other));
                levels = kept;
            }
            HeadingSummary { chart: HeadingChart::Levels { levels, pooled }, missing }
        }
    }
}

/// The rows behind bar `bar` of `summary` (ascending), among the rows
/// analyses use: a histogram bin's rows, a level's rows (or the pooled
/// levels' rows for the last bar), or every charted row of a line.
#[must_use]
pub fn bar_rows(col: &Column, rows: &RowStateManager, summary: &HeadingSummary, bar: usize) -> Vec<usize> {
    let n = col.data.len();
    let used = (0..n).filter(|&r| r >= rows.len() || !rows.is_excluded(r));
    match &summary.chart {
        HeadingChart::Histogram { min, max, counts } => {
            let Some(v) = col.as_numeric() else { return Vec::new() };
            used.filter(|&r| v[r].is_finite() && bin_of(v[r], *min, *max, counts.len()) == bar).collect()
        }
        HeadingChart::Levels { levels, pooled } => {
            let Some((name, _)) = levels.get(bar) else { return Vec::new() };
            let pooled = *pooled && bar + 1 == levels.len();
            let named: Vec<&str> = levels[..levels.len() - usize::from(pooled)].iter().map(|l| l.0.as_str()).collect();
            used.filter(|&r| {
                let t = col.level_text(r);
                if pooled { !t.trim().is_empty() && !named.contains(&t.as_str()) } else { t == *name }
            })
            .collect()
        }
        HeadingChart::Line { .. } => {
            let Some(v) = col.as_numeric() else { return Vec::new() };
            used.filter(|&r| v[r].is_finite()).collect()
        }
        HeadingChart::Empty => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::TemporalDisplay;

    /// A continuous column bins its included values; a click on a bin finds
    /// its rows; excluded rows and missing values are left out.
    #[test]
    fn histograms_count_included_values() {
        let col = Column::numeric("x", vec![0.0, 1.0, 2.0, 12.0, f64::NAN, 12.0]);
        let mut rows = RowStateManager::new(6);
        rows.set_excluded([5], true);
        let s = summarize(&col, &rows);
        assert_eq!(s.missing, 1);
        let HeadingChart::Histogram { min, max, counts } = &s.chart else { panic!("histogram expected") };
        assert_eq!((*min, *max), (0.0, 12.0));
        assert_eq!(counts.len(), BINS);
        assert_eq!(counts.iter().sum::<usize>(), 4);
        assert_eq!(counts[BINS - 1], 1, "the maximum falls in the last bin");
        assert_eq!(bar_rows(&col, &rows, &s, BINS - 1), vec![3]);
        assert_eq!(bar_rows(&col, &rows, &s, 0), vec![0]);
    }

    /// Categories count by their value labels, pooling levels past the limit.
    #[test]
    fn levels_count_by_label_and_pool_the_rest() {
        let mut col = Column::numeric("arm", vec![0.0, 1.0, 1.0, f64::NAN]);
        col.level = MeasurementLevel::Nominal;
        col.value_labels = [("1".to_owned(), "Treatment".to_owned())].into();
        let rows = RowStateManager::new(4);
        let s = summarize(&col, &rows);
        let levels = vec![("0".into(), 1), ("Treatment".into(), 2)];
        assert_eq!(s.chart, HeadingChart::Levels { levels, pooled: false });
        assert_eq!(s.missing, 1);
        assert_eq!(bar_rows(&col, &rows, &s, 1), vec![1, 2]);

        let names: Vec<String> = (0..20).map(|i| format!("L{i:02}")).chain(["L00".into(), "L00".into()]).collect();
        let col = Column::text("many", names);
        let rows = RowStateManager::new(22);
        let s = summarize(&col, &rows);
        let HeadingChart::Levels { levels, pooled: true } = &s.chart else { panic!("pooled levels expected") };
        assert_eq!(levels.len(), LEVELS);
        assert_eq!(levels[0], ("L00".into(), 3), "the most frequent level is kept");
        assert_eq!(levels[LEVELS - 1].1, 22 - 3 - (LEVELS - 2));
        assert_eq!(bar_rows(&col, &rows, &s, LEVELS - 1).len(), levels[LEVELS - 1].1);
    }

    /// Dates chart as a line in row order.
    #[test]
    fn dates_draw_a_row_order_line() {
        let display = TemporalDisplay::new(stat_data::TemporalFormat::Date);
        let col = Column::date_time("d", vec![86_400.0, f64::NAN, 0.0], display);
        let s = summarize(&col, &RowStateManager::new(3));
        assert_eq!(s.chart, HeadingChart::Line { points: vec![(0, 86_400.0), (2, 0.0)], min: 0.0, max: 86_400.0 });
        assert_eq!(s.missing, 1);
    }
}
