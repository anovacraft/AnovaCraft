//! Distributions per category: Tukey box plots (optionally notched) and
//! violin plots.

use stat_data::RowStateManager;

use super::{Element, ElementKind, categories_across, group_color, group_prefix, included_by_slot};
use crate::density::{kde_1d, silverman_bandwidth};
use crate::frame::Panel;
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, MarkShape};
use crate::palette;
use crate::stats::box_stats_weighted;

/// Tukey box plots of the vertical variable at each level of a categorical
/// horizontal axis (or of the whole panel when there is no horizontal
/// variable), one per subgroup side by side: a box from Q1 to Q3 with the
/// median, whiskers to the furthest points within the fences, and the points
/// beyond them as hollow circles.
///
/// Notches mark `median ± 1.58 · IQR / √n`, a rough 95% interval for the
/// median: boxes whose notches do not overlap differ in median (McGill,
/// Tukey & Larsen, "Variations of Box Plots", *American Statistician*
/// 32(1), 1978).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxPlotElement {
    /// Mark the observations beyond the fences.
    pub outliers: bool,
    /// Notch the boxes at the median.
    pub notches: bool,
}

impl Default for BoxPlotElement {
    fn default() -> Self {
        Self { outliers: true, notches: false }
    }
}

impl Element for BoxPlotElement {
    fn kind(&self) -> ElementKind {
        ElementKind::BoxPlot
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !categories_across(panel) {
            layer.notes.push("Box plots need categories (or nothing) across and numbers up.".to_owned());
            return layer;
        }
        let half = panel.slot_half_width * 0.7;
        let mut notch_past_hinge = false;
        for ((c, g), pts) in included_by_slot(panel) {
            let values: Vec<f64> = pts.iter().map(|p| p.y).collect();
            let freq: Vec<f64> = pts.iter().map(|p| p.freq).collect();
            let Some(b) = box_stats_weighted(&values, &freq) else { continue };
            let color = group_color(panel, g, palette::BOX);
            let name = format!("{}box plot", group_prefix(panel, g));
            let n: f64 = freq.iter().sum();
            let notch = 1.58 * (b.q3 - b.q1) / n.max(1.0).sqrt();
            if self.notches && notch > 0.0 {
                // Notched outline, drawn as a band of horizontal strips (it is
                // not convex), from Q1 up to Q3.
                let (lo, hi) = ((b.median - notch).max(b.q1), (b.median + notch).min(b.q3));
                notch_past_hinge |= b.median - notch < b.q1 || b.median + notch > b.q3;
                let inner = half * 0.5;
                let ys = [b.q1, lo, b.median, hi, b.q3];
                let widths = [half, half, inner, half, half];
                let left: Vec<[f64; 2]> = ys.iter().zip(&widths).map(|(&y, &w)| [c - w, y]).collect();
                let right: Vec<[f64; 2]> = ys.iter().zip(&widths).map(|(&y, &w)| [c + w, y]).collect();
                let mut outline = left.clone();
                outline.extend(right.iter().rev());
                outline.push(left[0]);
                layer.marks.push(Mark::Band {
                    lower: left,
                    upper: right,
                    color: color.with_alpha(0.15),
                    name: name.clone(),
                });
                layer.marks.push(Mark::Line {
                    points: outline,
                    color,
                    width: 1.5,
                    pattern: LinePattern::Solid,
                    name: name.clone(),
                });
                layer.marks.push(Mark::Line {
                    points: vec![[c - inner, b.median], [c + inner, b.median]],
                    color,
                    width: 2.5,
                    pattern: LinePattern::Solid,
                    name: name.clone(),
                });
            } else {
                layer.marks.push(Mark::Polygon {
                    points: vec![[c - half, b.q1], [c + half, b.q1], [c + half, b.q3], [c - half, b.q3]],
                    fill: color.with_alpha(0.15),
                    stroke: color,
                    name: name.clone(),
                });
                layer.marks.push(Mark::Line {
                    points: vec![[c - half, b.median], [c + half, b.median]],
                    color,
                    width: 2.5,
                    pattern: LinePattern::Solid,
                    name: name.clone(),
                });
            }
            let line = |points: Vec<[f64; 2]>| Mark::Line {
                points,
                color,
                width: 1.2,
                pattern: LinePattern::Solid,
                name: name.clone(),
            };
            layer.marks.push(line(vec![[c, b.q3], [c, b.upper_whisker]]));
            layer.marks.push(line(vec![[c, b.q1], [c, b.lower_whisker]]));
            let cap = half * 0.5;
            layer.marks.push(line(vec![[c - cap, b.upper_whisker], [c + cap, b.upper_whisker]]));
            layer.marks.push(line(vec![[c - cap, b.lower_whisker], [c + cap, b.lower_whisker]]));
            if self.outliers && !b.outliers.is_empty() {
                layer.marks.push(Mark::Points {
                    points: b.outliers.iter().map(|&(_, v)| [c, v]).collect(),
                    color,
                    shape: MarkShape::Circle,
                    radius: 4.5,
                    filled: false,
                    name: format!("{}outliers", group_prefix(panel, g)),
                });
            }
        }
        if notch_past_hinge {
            layer.notes.push("Some notches reach past the quartiles (few rows); they are drawn to the box.".to_owned());
        }
        if !layer.marks.is_empty() {
            let what = if self.notches {
                "Box: quartiles, median with notch (≈95% interval); whiskers to 1.5 × IQR"
            } else {
                "Box: quartiles and median; whiskers to 1.5 × IQR"
            };
            layer.key_entry(what, palette::BOX, KeyGlyph::Area(palette::BOX.with_alpha(0.15)));
            if self.outliers && layer.marks.iter().any(|m| matches!(m, Mark::Points { .. })) {
                layer.key_entry("Beyond the whiskers", palette::BOX, KeyGlyph::Marker(MarkShape::Circle, false));
            }
        }
        layer
    }
}

/// Violin plots: the kernel density of the vertical variable at each level
/// of a categorical horizontal axis, mirrored about the slot centre, each
/// scaled to the slot's width (so shapes compare, not areas). The density
/// is Gaussian with Silverman's bandwidth times `bandwidth_scale`, drawn over
/// the data's range. Optional dashed quartile lines and a solid median.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViolinElement {
    /// Multiplier of the default bandwidth (larger is smoother).
    pub bandwidth_scale: f64,
    /// Draw the quartiles and median across each violin.
    pub quartiles: bool,
}

impl Default for ViolinElement {
    fn default() -> Self {
        Self { bandwidth_scale: 1.0, quartiles: true }
    }
}

/// Vertical points along each violin.
const VIOLIN_POINTS: usize = 80;

impl Element for ViolinElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Violin
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !categories_across(panel) {
            layer.notes.push("Violins need categories (or nothing) across and numbers up.".to_owned());
            return layer;
        }
        let half = panel.slot_half_width * 0.9;
        let scale =
            if self.bandwidth_scale.is_finite() && self.bandwidth_scale > 0.0 { self.bandwidth_scale } else { 1.0 };
        for ((c, g), pts) in included_by_slot(panel) {
            let values: Vec<f64> = pts.iter().map(|p| p.y).collect();
            let freq: Vec<f64> = pts.iter().map(|p| p.freq).collect();
            let (lo, hi) = values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
            if !(lo.is_finite() && hi > lo) {
                continue; // one value or none: no shape to draw
            }
            let h = silverman_bandwidth(&values, &freq) * scale;
            let density = kde_1d(&values, &freq, h, lo, hi, VIOLIN_POINTS);
            let peak = density.iter().map(|d| d[1]).fold(0.0, f64::max);
            if peak <= 0.0 {
                continue;
            }
            let width = |d: f64| half * d / peak;
            let left: Vec<[f64; 2]> = density.iter().map(|d| [c - width(d[1]), d[0]]).collect();
            let right: Vec<[f64; 2]> = density.iter().map(|d| [c + width(d[1]), d[0]]).collect();
            let color = group_color(panel, g, palette::BOX);
            let name = format!("{}violin", group_prefix(panel, g));
            let mut outline = left.clone();
            outline.extend(right.iter().rev());
            outline.push(left[0]);
            layer.marks.push(Mark::Band {
                lower: left,
                upper: right,
                color: color.with_alpha(0.2),
                name: name.clone(),
            });
            layer.marks.push(Mark::Line {
                points: outline,
                color,
                width: 1.3,
                pattern: LinePattern::Solid,
                name: name.clone(),
            });
            if self.quartiles {
                if let Some(b) = box_stats_weighted(&values, &freq) {
                    // Each line spans the violin's width at its height.
                    let at = |y: f64| -> f64 {
                        let i = density.partition_point(|d| d[0] < y).min(density.len() - 1);
                        width(density[i][1])
                    };
                    for (y, pattern, w) in [
                        (b.q1, LinePattern::Dashed, 1.0),
                        (b.median, LinePattern::Solid, 2.0),
                        (b.q3, LinePattern::Dashed, 1.0),
                    ] {
                        let half_w = at(y);
                        layer.marks.push(Mark::Line {
                            points: vec![[c - half_w, y], [c + half_w, y]],
                            color,
                            width: w,
                            pattern,
                            name: name.clone(),
                        });
                    }
                }
            }
        }
        if !layer.marks.is_empty() {
            layer.key_entry("Violin: density of values", palette::BOX, KeyGlyph::Area(palette::BOX.with_alpha(0.2)));
            if self.quartiles {
                layer.key_entry("Violin quartiles (median solid)", palette::BOX, KeyGlyph::Line(LinePattern::Dashed));
            }
        }
        layer
    }
}
