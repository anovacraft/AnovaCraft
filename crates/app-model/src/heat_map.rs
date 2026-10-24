//! A matrix of signed values drawn as coloured cells with each value printed
//! in its cell (a correlation heat map). The colour follows a diverging ramp
//! (hue for the sign, contrast with the neutral for the size), and the
//! printed number carries the same information, so colour is never the only
//! cue.

use stat_viz::export::{ChartPanel, Label};
use stat_viz::palette::{DIVERGING_LIGHT, LABEL, diverging};
use stat_viz::{KeyEntry, KeyGlyph, Mark, Rgba};

use crate::report::TextTable;

/// A heat map of a matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatMapSpec {
    /// Row labels, top to bottom.
    pub rows: Vec<String>,
    /// Column labels, left to right.
    pub columns: Vec<String>,
    /// `values[i][j]` (NaN where there is no value).
    pub values: Vec<Vec<f64>>,
    /// Text printed in each cell (parallel to `values`).
    pub text: Vec<Vec<String>>,
    /// What the values are, e.g. `Pearson r` (the key's title).
    pub measure: String,
    /// The magnitude drawn at full colour (values are divided by it and
    /// clamped to [−1, 1]); 1 for correlations.
    pub limit: f64,
}

impl HeatMapSpec {
    /// Position of `value` on the diverging ramp, in [−1, 1] (0 for NaN).
    #[must_use]
    pub fn ramp_position(&self, value: f64) -> f64 {
        if value.is_finite() && self.limit > 0.0 { (value / self.limit).clamp(-1.0, 1.0) } else { 0.0 }
    }

    /// The cell text as a table (row label, then one column per column label).
    #[must_use]
    pub fn table(&self) -> TextTable {
        let mut t = TextTable::new(std::iter::once(String::new()).chain(self.columns.iter().cloned()));
        for (label, cells) in self.rows.iter().zip(&self.text) {
            t.push(std::iter::once(label.clone()).chain(cells.iter().cloned()));
        }
        t
    }

    /// The heat map as a figure panel (light palette): unit cells with row 0
    /// at the top, each value printed in dark or light ink to contrast with
    /// its fill, labelled rows and columns, and a key of the ramp's ends.
    #[must_use]
    pub fn chart_panel(&self) -> ChartPanel {
        let n_rows = self.rows.len();
        let mut c = ChartPanel { x_title: String::new(), y_title: String::new(), ..ChartPanel::default() };
        for (i, row) in self.values.iter().enumerate() {
            let y = (n_rows - i) as f64;
            for (j, &v) in row.iter().enumerate() {
                let x = j as f64 + 1.0;
                let fill = diverging(self.ramp_position(v), DIVERGING_LIGHT);
                c.marks.push(Mark::Polygon {
                    points: vec![[x - 0.5, y - 0.5], [x + 0.5, y - 0.5], [x + 0.5, y + 0.5], [x - 0.5, y + 0.5]],
                    fill,
                    stroke: Rgba::rgb(255, 255, 255),
                    name: self.measure.clone(),
                });
                let text = self.text.get(i).and_then(|r| r.get(j)).cloned().unwrap_or_default();
                if !text.is_empty() {
                    c.labels.push(Label {
                        at: [x, y],
                        text,
                        size: 8.0,
                        color: ink_on(fill),
                        align: 0.5,
                        valign: 0.5,
                        bold: false,
                    });
                }
            }
        }
        c.x_ticks = Some(self.columns.iter().enumerate().map(|(j, l)| (j as f64 + 1.0, l.clone())).collect());
        c.y_ticks = Some(self.rows.iter().enumerate().map(|(i, l)| ((n_rows - i) as f64, l.clone())).collect());
        c.x_range = (0.5, self.columns.len() as f64 + 0.5);
        c.y_range = (0.5, n_rows as f64 + 0.5);
        let end = |v: f64| crate::format::format_number(v, 3);
        c.key = vec![
            KeyEntry {
                label: format!("{} = {}", self.measure, end(-self.limit)),
                color: DIVERGING_LIGHT[0],
                glyph: KeyGlyph::Area(DIVERGING_LIGHT[0]),
            },
            KeyEntry { label: "0".to_owned(), color: DIVERGING_LIGHT[1], glyph: KeyGlyph::Area(DIVERGING_LIGHT[1]) },
            KeyEntry {
                label: format!("{} = {}", self.measure, end(self.limit)),
                color: DIVERGING_LIGHT[2],
                glyph: KeyGlyph::Area(DIVERGING_LIGHT[2]),
            },
        ];
        c
    }
}

/// Dark ink on light fills and white ink on dark ones (relative luminance
/// threshold 0.5), so printed values stay legible on every cell.
#[must_use]
pub fn ink_on(fill: Rgba) -> Rgba {
    let [r, g, b, _] = fill.0;
    let lum = (0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)) / 255.0;
    if lum > 0.5 { LABEL } else { Rgba::rgb(255, 255, 255) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_cells_text_and_ink() {
        let h = HeatMapSpec {
            rows: vec!["a".into(), "b".into()],
            columns: vec!["a".into(), "b".into()],
            values: vec![vec![1.0, -0.5], vec![-0.5, f64::NAN]],
            text: vec![vec!["1".into(), "−0.5".into()], vec!["−0.5".into(), String::new()]],
            measure: "r".into(),
            limit: 1.0,
        };
        let c = h.chart_panel();
        assert_eq!(c.marks.len(), 4);
        assert_eq!(c.labels.len(), 3, "an empty cell prints nothing");
        // Row 0 sits at the top.
        assert_eq!(c.y_ticks.as_ref().unwrap()[0], (2.0, "a".to_owned()));
        assert_eq!(c.labels[0].color, Rgba::rgb(255, 255, 255), "white on the dark +1 cell");
        assert_eq!(ink_on(DIVERGING_LIGHT[1]), LABEL);
        assert_eq!(h.ramp_position(f64::NAN), 0.0);
        assert_eq!(h.table().rows[1], ["b", "−0.5", ""]);
    }
}
