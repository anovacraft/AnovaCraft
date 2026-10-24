//! Row colours and markers set from a column's values.
//!
//! *Colour and marker by column* gives every row a display colour (and, for
//! a categorical column, a marker shape) from its value in one column: the
//! categorical palette with a marker per level, so levels never differ by
//! colour alone, or the sequential ramp for a continuous column. The rows
//! keep those styles in their row state, and every plot with no grouping of
//! its own draws them; [`row_style_key`] rebuilds the plot's key from the
//! column the styles came from.

use stat_data::{DataTable, MeasurementLevel, RowColor, RowMarker, RowStateManager, natural_order};

use crate::frame::ColorKey;
use crate::layer::{MarkShape, Rgba};
use crate::palette;

/// The colours (and markers) a column's values give the rows.
#[derive(Debug, Clone, PartialEq)]
pub enum RowStyleScheme {
    /// One colour and marker per level of a categorical column.
    Levels {
        /// Column name.
        title: String,
        /// `(level, colour, marker)` in natural order of the level names.
        levels: Vec<(String, RowColor, RowMarker)>,
        /// Level of each row (`None` when missing).
        codes: Vec<Option<usize>>,
    },
    /// The sequential ramp over a continuous column's range.
    Ramp {
        /// Column name.
        title: String,
        /// Smallest value.
        min: f64,
        /// Largest value.
        max: f64,
        /// Each row's value (NaN when missing).
        values: Vec<f64>,
    },
}

/// The `#rrggbb` colour of an opaque palette colour.
#[must_use]
pub fn row_color(c: Rgba) -> RowColor {
    RowColor { r: c.0[0], g: c.0[1], b: c.0[2] }
}

/// The palette colour of a row colour.
#[must_use]
pub fn rgba(c: RowColor) -> Rgba {
    Rgba::rgb(c.r, c.g, c.b)
}

/// The plot marker of a row marker.
#[must_use]
pub fn mark_shape(m: RowMarker) -> MarkShape {
    match m {
        RowMarker::Circle => MarkShape::Circle,
        RowMarker::Square => MarkShape::Square,
        RowMarker::Diamond => MarkShape::Diamond,
        RowMarker::Up => MarkShape::Up,
        RowMarker::Down => MarkShape::Down,
        RowMarker::Plus => MarkShape::Plus,
        RowMarker::Cross => MarkShape::Cross,
        RowMarker::Asterisk => MarkShape::Asterisk,
    }
}

/// Colour and marker of level `k`: the categorical palette and the marker
/// sequence side by side. Seven colours against eight markers give 56
/// distinct colour–marker pairs before one repeats.
#[must_use]
pub fn level_row_style(k: usize) -> (RowColor, RowMarker) {
    let n = palette::CATEGORICAL.len();
    let markers = RowMarker::ALL;
    (row_color(palette::CATEGORICAL[k % n]), markers[k % markers.len()])
}

impl RowStyleScheme {
    /// The scheme of column `column` of `table` over all its rows: the ramp
    /// for a continuous numeric column, levels otherwise (named as analyses
    /// name them, by value label when there is one). `None` for an unknown
    /// column or one with no values.
    #[must_use]
    pub fn of(table: &DataTable, column: &str) -> Option<Self> {
        let col = table.column(table.find(column)?)?;
        let title = col.name.clone();
        if let (Some(v), MeasurementLevel::Continuous) = (col.as_numeric(), col.level) {
            let (min, max) = v
                .iter()
                .filter(|x| x.is_finite())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
            return min.is_finite().then(|| Self::Ramp { title, min, max, values: v.to_vec() });
        }
        let names: Vec<String> = (0..table.n_rows()).map(|r| col.level_text(r)).collect();
        let mut levels: Vec<String> = names.iter().filter(|t| !t.trim().is_empty()).cloned().collect();
        levels.sort_by(|a, b| natural_order(a, b));
        levels.dedup();
        if levels.is_empty() {
            return None;
        }
        let codes = names
            .iter()
            .map(|t| if t.trim().is_empty() { None } else { levels.binary_search_by(|l| natural_order(l, t)).ok() })
            .collect();
        let levels = levels
            .into_iter()
            .enumerate()
            .map(|(k, l)| {
                let (c, m) = level_row_style(k);
                (l, c, m)
            })
            .collect();
        Some(Self::Levels { title, levels, codes })
    }

    /// The colour and marker the scheme gives `row` (`None` for a missing
    /// value; the ramp gives no marker).
    #[must_use]
    pub fn style(&self, row: usize) -> Option<(RowColor, Option<RowMarker>)> {
        match self {
            Self::Levels { levels, codes, .. } => {
                codes.get(row).copied().flatten().map(|k| (levels[k].1, Some(levels[k].2)))
            }
            Self::Ramp { min, max, values, .. } => {
                let v = values.get(row).copied().filter(|v| v.is_finite())?;
                let t = if max > min { (v - min) / (max - min) } else { 0.5 };
                Some((row_color(palette::ramp(t)), None))
            }
        }
    }

    /// Each row's colour and marker, in row order.
    #[must_use]
    pub fn styles(&self, n_rows: usize) -> (Vec<Option<RowColor>>, Vec<Option<RowMarker>>) {
        (0..n_rows).map(|r| self.style(r).map_or((None, None), |(c, m)| (Some(c), m))).unzip()
    }
}

/// The key of the row colours for a plot drawing `rows`: the levels (or
/// the ramp) of the column the colours were set from, keeping only levels
/// some of those rows still show (a row recoloured by hand drops out).
/// `None` when the colours were not set from a column, or the column is
/// gone.
#[must_use]
pub fn row_style_key(table: &DataTable, state: &RowStateManager, rows: &[usize]) -> Option<ColorKey> {
    let scheme = RowStyleScheme::of(table, state.style_column()?)?;
    match &scheme {
        RowStyleScheme::Levels { title, levels, codes } => {
            let mut shown = vec![false; levels.len()];
            for &r in rows.iter().filter(|&&r| r < state.len()) {
                if let Some(k) = codes.get(r).copied().flatten()
                    && state.color(r) == Some(levels[k].1)
                    && state.marker(r) == Some(levels[k].2)
                {
                    shown[k] = true;
                }
            }
            let entries: Vec<(String, Rgba, MarkShape)> = levels
                .iter()
                .zip(shown)
                .filter(|(_, s)| *s)
                .map(|((l, c, m), _)| (l.clone(), rgba(*c), mark_shape(*m)))
                .collect();
            (!entries.is_empty()).then(|| ColorKey::Levels { title: title.clone(), entries })
        }
        RowStyleScheme::Ramp { title, min, max, .. } => rows
            .iter()
            .any(|&r| r < state.len() && state.color(r).is_some())
            .then(|| ColorKey::Gradient { title: title.clone(), min: *min, max: *max }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::Column;

    fn table() -> DataTable {
        let mut arm = Column::numeric("Arm", vec![0.0, 1.0, 1.0, f64::NAN, 0.0]);
        arm.level = MeasurementLevel::Nominal;
        arm.value_labels = [("0".to_owned(), "Control".to_owned()), ("1".to_owned(), "Treatment".to_owned())].into();
        DataTable::new("t", vec![arm, Column::numeric("Dose", vec![1.0, 2.0, 3.0, 4.0, 5.0])]).unwrap()
    }

    /// Levels take the palette and a marker each, named by their labels;
    /// a continuous column takes the ramp ends; missing values get nothing.
    #[test]
    fn schemes_follow_levels_and_ramp() {
        let t = table();
        let s = RowStyleScheme::of(&t, "Arm").unwrap();
        let RowStyleScheme::Levels { levels, .. } = &s else { panic!("levels expected") };
        assert_eq!(levels.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Control", "Treatment"]);
        assert_eq!(s.style(0), Some((level_row_style(0).0, Some(RowMarker::Circle))));
        assert_eq!(s.style(1), Some((level_row_style(1).0, Some(RowMarker::Square))));
        assert_eq!(s.style(3), None);
        let r = RowStyleScheme::of(&t, "Dose").unwrap();
        assert_eq!(r.style(0), Some((row_color(palette::ramp(0.0)), None)));
        assert_eq!(r.style(4), Some((row_color(palette::ramp(1.0)), None)));
        assert!(RowStyleScheme::of(&t, "nope").is_none());
    }

    /// Colour–marker pairs stay distinct for many levels.
    #[test]
    fn level_styles_are_distinct() {
        let pairs: Vec<_> = (0..56).map(level_row_style).collect();
        for (i, a) in pairs.iter().enumerate() {
            assert!(pairs[..i].iter().all(|b| b != a), "level {i} repeats an earlier style");
        }
    }

    /// The key lists only levels the plotted rows still show.
    #[test]
    fn key_lists_levels_still_shown() {
        let t = table();
        let mut state = RowStateManager::new(5);
        let (colors, markers) = RowStyleScheme::of(&t, "Arm").unwrap().styles(5);
        for r in 0..5 {
            state.set_color([r], colors[r]);
            state.set_marker([r], markers[r]);
        }
        assert!(row_style_key(&t, &state, &[0, 1]).is_none(), "no key without a column");
        state.set_style_column(Some("Arm".into()));
        let Some(ColorKey::Levels { entries, .. }) = row_style_key(&t, &state, &[0, 1, 2]) else { panic!() };
        assert_eq!(entries.len(), 2);
        // Recolour the treatment rows by hand: their level leaves the key.
        state.set_color([1, 2], Some(RowColor { r: 1, g: 2, b: 3 }));
        let Some(ColorKey::Levels { entries, .. }) = row_style_key(&t, &state, &[0, 1, 2]) else { panic!() };
        assert_eq!(entries.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["Control"]);
    }
}
