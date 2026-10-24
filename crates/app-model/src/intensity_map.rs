//! A dense grid of non-negative values over two numeric axes, drawn as a
//! raster of coloured cells (a time–frequency map such as a spectrogram or
//! a wavelet scalogram). Values are binned into [`BANDS`] equal steps of the
//! range, coloured along the sequential scale; a colour bar with the values
//! at the band boundaries sits beside the map and hovering a cell reports its
//! coordinates and value, so colour is never the only way to read it.

use stat_viz::export::ChartPanel;
use stat_viz::palette::ColorScale;
use stat_viz::{ColorBar, Mark, Rgba};

use crate::format::format_number;
use crate::report::TextTable;

/// Colour bands of the map (values are binned into equal steps of the range).
pub const BANDS: usize = 12;

/// A raster of values over evenly spaced cell centres.
#[derive(Debug, Clone, PartialEq)]
pub struct IntensityMapSpec {
    /// Horizontal axis title.
    pub x_label: String,
    /// Vertical axis title.
    pub y_label: String,
    /// What the values are (the key's title), e.g. `Power (dB)`.
    pub value_label: String,
    /// Horizontal cell centres, evenly spaced and increasing.
    pub x: Vec<f64>,
    /// Vertical cell centres (in plot coordinates), evenly spaced and
    /// increasing.
    pub y: Vec<f64>,
    /// `values[i][j]`: the value at `(x[i], y[j])` (NaN for none).
    pub values: Vec<Vec<f64>>,
    /// Labelled ticks for the vertical axis when its plot coordinate is a
    /// transform (e.g. log₂ frequency); `None` for numeric ticks.
    pub y_ticks: Option<Vec<(f64, String)>>,
    /// How to show a vertical coordinate in hover text (e.g. the frequency
    /// for a log₂ coordinate); `None` shows it as it is.
    pub y_display: Option<YDisplay>,
    /// Values at the two ends of the colour ramp.
    pub range: (f64, f64),
}

/// How a vertical plot coordinate maps to the quantity it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YDisplay {
    /// The plot coordinate is `log₂` of the quantity.
    Log2,
}

impl IntensityMapSpec {
    /// Width and height of one cell in plot coordinates.
    #[must_use]
    pub fn cell_size(&self) -> (f64, f64) {
        let step = |v: &[f64]| if v.len() >= 2 { (v[v.len() - 1] - v[0]) / (v.len() - 1) as f64 } else { 1.0 };
        (step(&self.x), step(&self.y))
    }

    /// The plotted extent `[x_min, x_max] × [y_min, y_max]` (cell edges).
    #[must_use]
    pub fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        let (dx, dy) = self.cell_size();
        let x0 = self.x.first().copied().unwrap_or(0.0) - dx / 2.0;
        let x1 = self.x.last().copied().unwrap_or(0.0) + dx / 2.0;
        let y0 = self.y.first().copied().unwrap_or(0.0) - dy / 2.0;
        let y1 = self.y.last().copied().unwrap_or(0.0) + dy / 2.0;
        ([x0, x1], [y0, y1])
    }

    /// Position of `value` on the ramp, in [0, 1] (NaN for none).
    #[must_use]
    pub fn ramp_position(&self, value: f64) -> f64 {
        let (lo, hi) = self.range;
        if !value.is_finite() {
            return f64::NAN;
        }
        if hi > lo { ((value - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.5 }
    }

    /// The cell containing plot point `(x, y)`, if any.
    #[must_use]
    pub fn cell_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let ([x0, x1], [y0, y1]) = self.bounds();
        if !(x >= x0 && x < x1 && y >= y0 && y < y1) || self.x.is_empty() || self.y.is_empty() {
            return None;
        }
        let (dx, dy) = self.cell_size();
        let i = (((x - x0) / dx).floor() as usize).min(self.x.len() - 1);
        let j = (((y - y0) / dy).floor() as usize).min(self.y.len() - 1);
        Some((i, j))
    }

    /// The vertical quantity a plot coordinate stands for.
    #[must_use]
    pub fn y_value(&self, y: f64) -> f64 {
        match self.y_display {
            Some(YDisplay::Log2) => y.exp2(),
            None => y,
        }
    }

    /// Hover text of cell `(i, j)`.
    #[must_use]
    pub fn describe_cell(&self, i: usize, j: usize) -> String {
        let v = self.values.get(i).and_then(|c| c.get(j)).copied().unwrap_or(f64::NAN);
        format!(
            "{}: {}\n{}: {}\n{}: {}",
            self.x_label,
            format_number(self.x[i], 5),
            self.y_label,
            format_number(self.y_value(self.y[j]), 5),
            self.value_label,
            format_number(v, 4)
        )
    }

    /// Colour band (0 = lowest) of `value`, if it has one.
    #[must_use]
    pub fn band_of(&self, value: f64) -> Option<usize> {
        let t = self.ramp_position(value);
        if t.is_nan() { None } else { Some(((t * BANDS as f64).floor() as usize).min(BANDS - 1)) }
    }

    /// Colour of band `k` (the sequential scale at the band's middle).
    #[must_use]
    pub fn band_color(k: usize, dark: bool) -> Rgba {
        ColorScale::Sequential.color((k as f64 + 0.5) / BANDS as f64, dark)
    }

    /// The colour key: one box per band and the values at their boundaries.
    #[must_use]
    pub fn color_bar(&self, dark: bool) -> ColorBar {
        let (lo, hi) = self.range;
        ColorBar {
            title: self.value_label.clone(),
            colors: (0..BANDS).map(|k| Self::band_color(k, dark)).collect(),
            labels: (1..BANDS).map(|k| format_number(lo + (hi - lo) * k as f64 / BANDS as f64, 3)).collect(),
        }
    }

    /// The filled cells as `(band, rectangle)`: runs of neighbouring cells
    /// in one column that share a band are merged; cells without a value are
    /// left out.
    #[must_use]
    pub fn rectangles(&self) -> Vec<(usize, Vec<[f64; 2]>)> {
        let (dx, dy) = self.cell_size();
        let mut out = Vec::new();
        for (i, col) in self.values.iter().enumerate() {
            let x = self.x[i];
            let mut j = 0;
            while j < col.len() {
                let Some(band) = self.band_of(col[j]) else {
                    j += 1;
                    continue;
                };
                let mut k = j + 1;
                while k < col.len() && self.band_of(col[k]) == Some(band) {
                    k += 1;
                }
                let (y0, y1) = (self.y[j] - dy / 2.0, self.y[k - 1] + dy / 2.0);
                out.push((band, vec![[x - dx / 2.0, y0], [x + dx / 2.0, y0], [x + dx / 2.0, y1], [x - dx / 2.0, y1]]));
                j = k;
            }
        }
        out
    }

    /// The grid in long form: one row per cell (`x`, `y`, value).
    #[must_use]
    pub fn table(&self) -> TextTable {
        let mut t = TextTable::new([self.x_label.clone(), self.y_label.clone(), self.value_label.clone()]);
        for (i, col) in self.values.iter().enumerate() {
            for (j, &v) in col.iter().enumerate() {
                t.push([format_number(self.x[i], 6), format_number(self.y_value(self.y[j]), 6), format_number(v, 6)]);
            }
        }
        t
    }

    /// The map as a figure panel: the filled cells and a colour bar.
    #[must_use]
    pub fn chart_panel(&self) -> ChartPanel {
        let mut c =
            ChartPanel { x_title: self.x_label.clone(), y_title: self.y_label.clone(), ..ChartPanel::default() };
        for (band, points) in self.rectangles() {
            let fill = Self::band_color(band, false);
            c.marks.push(Mark::Polygon { points, fill, stroke: fill, name: self.value_label.clone() });
        }
        let ([x0, x1], [y0, y1]) = self.bounds();
        c.x_range = (x0, x1);
        c.y_range = (y0, y1);
        c.y_ticks = self.y_ticks.clone();
        c.color_bar = Some(self.color_bar(false));
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> IntensityMapSpec {
        IntensityMapSpec {
            x_label: "Time".into(),
            y_label: "Frequency".into(),
            value_label: "Power".into(),
            x: vec![0.5, 1.5, 2.5],
            y: vec![0.0, 1.0],
            values: vec![vec![0.0, 1.0], vec![2.0, 3.0], vec![4.0, 5.0]],
            y_ticks: None,
            y_display: None,
            range: (0.0, 5.0),
        }
    }

    #[test]
    fn cells_and_bounds() {
        let s = spec();
        assert_eq!(s.bounds(), ([0.0, 3.0], [-0.5, 1.5]));
        assert_eq!(s.cell_at(1.2, 0.9), Some((1, 1)));
        assert_eq!(s.cell_at(3.5, 0.0), None);
        assert!((s.ramp_position(2.5) - 0.5).abs() < 1e-12);
        assert!(s.describe_cell(2, 0).contains("Power: 4"));
        assert_eq!(s.table().rows.len(), 6);
        let panel = s.chart_panel();
        assert_eq!(panel.marks.len(), 6);
        assert_eq!(panel.color_bar.map(|b| b.colors.len()), Some(BANDS));
        assert_eq!(s.band_of(5.0), Some(BANDS - 1));
        assert_eq!(s.band_of(f64::NAN), None);
    }
}
