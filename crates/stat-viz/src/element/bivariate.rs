//! Bivariate density layers: normal density ellipses, kernel-density
//! contours and binned counts (rectangles or hexagons).

use stat_data::RowStateManager;

use super::{
    Element, ElementKind, both_numeric, group_color, group_prefix, included_by_group, percent, split_key_color,
};
use crate::density::{contour_segments, density_ellipse, kde_2d, mass_thresholds};
use crate::frame::Panel;
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, Ramp, TextAnchor};
use crate::palette;

/// Bivariate-normal density ellipses per subgroup at one or more
/// probability levels (e.g. 90%, 95%, 99%): the region the normal
/// distribution with the subgroup's mean and covariance assigns that
/// probability. The first level is solid, others dashed; the correlation
/// can be written at the top of the outermost ellipse.
#[derive(Debug, Clone, PartialEq)]
pub struct EllipseElement {
    /// Probability levels, each in (0, 1).
    pub levels: Vec<f64>,
    /// Write the correlation coefficient.
    pub correlation: bool,
}

impl Default for EllipseElement {
    fn default() -> Self {
        Self { levels: vec![0.95], correlation: true }
    }
}

impl Element for EllipseElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Ellipse
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !both_numeric(panel) {
            layer.notes.push("Density ellipses need numbers on both axes.".to_owned());
            return layer;
        }
        let mut levels: Vec<f64> = self.levels.iter().copied().filter(|l| *l > 0.0 && *l < 1.0).collect();
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        let mut singular = 0;
        for (g, pts) in included_by_group(panel).iter().enumerate() {
            if pts.is_empty() {
                continue;
            }
            let x: Vec<f64> = pts.iter().map(|p| p.x).collect();
            let y: Vec<f64> = pts.iter().map(|p| p.y).collect();
            let w: Vec<f64> = pts.iter().map(|p| p.freq).collect();
            let color = group_color(panel, g, palette::FIT);
            let mut top: Option<([f64; 2], f64)> = None;
            for (k, &level) in levels.iter().enumerate() {
                let Some((outline, r)) = density_ellipse(&x, &y, &w, level, 96) else {
                    singular += usize::from(k == 0);
                    break;
                };
                let highest =
                    outline.iter().copied().fold([0.0, f64::NEG_INFINITY], |a, p| if p[1] > a[1] { p } else { a });
                top = Some((highest, r));
                layer.marks.push(Mark::Line {
                    points: outline,
                    color,
                    width: 1.5,
                    pattern: if k == 0 { LinePattern::Solid } else { LinePattern::Dashed },
                    name: format!("{}{} density ellipse", group_prefix(panel, g), percent(level)),
                });
            }
            if let (true, Some((at, r))) = (self.correlation, top) {
                layer.marks.push(Mark::Text {
                    at,
                    text: format!("{}r = {r:.3}", group_prefix(panel, g)),
                    color,
                    // Above the ellipse's top, clear of its outline and points.
                    anchor: TextAnchor::Above,
                    movable: Some(format!("ellipse/{g}")),
                });
            }
        }
        if singular > 0 {
            layer.notes.push(format!("{singular} subgroup(s) have too few rows or lie on a line: no density ellipse."));
        }
        if !layer.marks.is_empty() {
            let names: Vec<String> = levels.iter().map(|l| percent(*l)).collect();
            let color = split_key_color(panel, palette::FIT);
            layer.key_entry(
                format!("Normal density ellipse ({})", names.join(", ")),
                color,
                KeyGlyph::Line(LinePattern::Solid),
            );
        }
        layer
    }
}

/// Kernel-density contours per subgroup: lines bounding the smallest regions
/// that hold the given fractions of the estimated density (e.g. 50% and 90%
/// of the data), from a Gaussian product-kernel estimate with each axis's
/// default bandwidth times `bandwidth_scale`. Inner contours are drawn
/// stronger. Filled, each region is also shaded in a translucent wash of
/// the group's colour; the washes stack, so the densest region is darkest.
#[derive(Debug, Clone, PartialEq)]
pub struct ContourElement {
    /// Fractions of the probability mass enclosed, each in (0, 1).
    pub fractions: Vec<f64>,
    /// Multiplier of the default bandwidths (larger is smoother).
    pub bandwidth_scale: f64,
    /// Shade the regions inside the contours.
    pub filled: bool,
}

impl Default for ContourElement {
    fn default() -> Self {
        Self { fractions: vec![0.25, 0.5, 0.75, 0.9], bandwidth_scale: 1.0, filled: false }
    }
}

/// Opacity of each stacked wash of a filled density contour.
const CONTOUR_WASH: f64 = 0.16;

/// Grid nodes along each axis of a density estimate.
const CONTOUR_GRID: usize = 70;

impl Element for ContourElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Contour
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !both_numeric(panel) {
            layer.notes.push("Density contours need numbers on both axes.".to_owned());
            return layer;
        }
        let mut fractions: Vec<f64> = self.fractions.iter().copied().filter(|f| *f > 0.0 && *f < 1.0).collect();
        fractions.sort_by(f64::total_cmp);
        fractions.dedup();
        for (g, pts) in included_by_group(panel).iter().enumerate() {
            let x: Vec<f64> = pts.iter().map(|p| p.x).collect();
            let y: Vec<f64> = pts.iter().map(|p| p.y).collect();
            let w: Vec<f64> = pts.iter().map(|p| p.freq * p.weight).collect();
            let Some(grid) = kde_2d(&x, &y, &w, self.bandwidth_scale, CONTOUR_GRID, CONTOUR_GRID) else { continue };
            let thresholds = mass_thresholds(&grid, &fractions);
            let color = group_color(panel, g, palette::SMOOTH);
            let n = thresholds.len().max(1) as f64;
            if self.filled {
                let z: Vec<Vec<f64>> = grid.z.chunks(grid.xs.len()).map(<[f64]>::to_vec).collect();
                let field = crate::contour::ContourField::from_grid(&grid.xs, &grid.ys, &z);
                for (&level, &fraction) in thresholds.iter().zip(&fractions) {
                    let bands = field.isobands(&[level]);
                    let Some(inside) = bands.into_iter().nth(1).filter(|b| !b.pieces.is_empty()) else { continue };
                    layer.marks.push(Mark::Area {
                        rings: inside.rings,
                        pieces: inside.pieces,
                        color: color.with_alpha(CONTOUR_WASH),
                        name: format!("{}{} density region", group_prefix(panel, g), percent(fraction)),
                    });
                }
            }
            for (k, (&level, &fraction)) in thresholds.iter().zip(&fractions).enumerate() {
                let segments = contour_segments(&grid, level);
                if segments.is_empty() {
                    continue;
                }
                // Inner (smaller) regions stronger.
                let strength = 1.0 - 0.6 * k as f64 / n;
                layer.marks.push(Mark::Segments {
                    segments,
                    color: color.with_alpha(strength),
                    width: 1.4,
                    name: format!("{}{} density contour", group_prefix(panel, g), percent(fraction)),
                });
            }
        }
        if !layer.marks.is_empty() {
            let names: Vec<String> = fractions.iter().map(|f| percent(*f)).collect();
            layer.key_entry(
                format!("Density contours holding {} of rows", names.join(", ")),
                palette::SMOOTH,
                KeyGlyph::Line(LinePattern::Solid),
            );
        } else if pts_too_few(panel) {
            layer.notes.push("Density contours need at least two rows with different values.".to_owned());
        }
        layer
    }
}

fn pts_too_few(panel: &Panel) -> bool {
    panel.points.iter().filter(|p| p.in_analysis()).count() < 2
}

/// Counts of rows (frequency-weighted) in a grid of rectangles or hexagons
/// over the panel, each cell shaded on a sequential ramp from few to many —
/// a density view for large data where points overlap (Carr et al., "Scatterplot
/// Matrix Techniques for Large N", *JASA* 82(398), 1987). Empty cells are
/// not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinElement {
    /// Hexagonal cells (otherwise rectangles).
    pub hexagonal: bool,
    /// Cells across the horizontal range.
    pub bins: usize,
}

impl Default for BinElement {
    fn default() -> Self {
        Self { hexagonal: true, bins: 20 }
    }
}

/// Opacity of binned cells.
const BIN_ALPHA: f64 = 0.85;

impl Element for BinElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Bins
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !both_numeric(panel) {
            layer.notes.push("Binned counts need numbers on both axes.".to_owned());
            return layer;
        }
        let pts: Vec<(f64, f64, f64)> =
            panel.points.iter().filter(|p| p.in_analysis()).map(|p| (p.x, p.y, p.freq)).collect();
        if pts.is_empty() {
            return layer;
        }
        let bounds = |f: fn(&(f64, f64, f64)) -> f64| {
            pts.iter().map(f).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)))
        };
        let ((x0, x1), (y0, y1)) = (bounds(|p| p.0), bounds(|p| p.1));
        // Degenerate ranges get a unit-wide window (wider about huge values,
        // where ±0.5 would round away).
        let window = |a: f64| {
            let half = 0.5_f64.max(a.abs() * 1e-6);
            (a - half, a + half)
        };
        let (x0, x1) = if x1 > x0 { (x0, x1) } else { window(x0) };
        let (y0, y1) = if y1 > y0 { (y0, y1) } else { window(y0) };
        // A range wider than the largest float (±1e300 on a reciprocal
        // scale) cannot be divided into cells.
        if !((x1 - x0).is_finite() && (y1 - y0).is_finite() && x1 > x0 && y1 > y0) {
            layer.notes.push("The data's range is too wide to bin.".to_owned());
            return layer;
        }
        let bins = self.bins.clamp(2, 200);
        // Work in units where both ranges span `bins` cells.
        let to_u = |x: f64| (x - x0) / (x1 - x0) * bins as f64;
        let to_v = |y: f64| (y - y0) / (y1 - y0) * bins as f64;
        let from = |u: f64, v: f64| [x0 + u / bins as f64 * (x1 - x0), y0 + v / bins as f64 * (y1 - y0)];
        let mut cells: std::collections::BTreeMap<(i64, i64), (f64, f64, f64)> = std::collections::BTreeMap::new();
        let r = 1.0 / 3f64.sqrt(); // hexagon circumradius for a column spacing of 1
        for &(x, y, f) in &pts {
            let (u, v) = (to_u(x), to_v(y));
            let (key, cu, cv) = if self.hexagonal {
                // Two rectangular lattices of centres; the nearer centre wins.
                let (w, h) = (1.0, 3.0 * r);
                let (i1, j1) = ((u / w).round(), (v / h).round());
                let (c1u, c1v) = (i1 * w, j1 * h);
                let (i2, j2) = (((u - 0.5 * w) / w).round(), ((v - 0.5 * h) / h).round());
                let (c2u, c2v) = (i2 * w + 0.5 * w, j2 * h + 0.5 * h);
                if (u - c1u).powi(2) + (v - c1v).powi(2) <= (u - c2u).powi(2) + (v - c2v).powi(2) {
                    ((2 * i1 as i64, 2 * j1 as i64), c1u, c1v)
                } else {
                    ((2 * i2 as i64 + 1, 2 * j2 as i64 + 1), c2u, c2v)
                }
            } else {
                let (i, j) = (u.floor().min(bins as f64 - 1.0), v.floor().min(bins as f64 - 1.0));
                ((i as i64, j as i64), i + 0.5, j + 0.5)
            };
            let cell = cells.entry(key).or_insert((cu, cv, 0.0));
            cell.2 += f;
        }
        let max = cells.values().map(|c| c.2).fold(0.0, f64::max);
        if max <= 0.0 {
            return layer;
        }
        for &(cu, cv, count) in cells.values() {
            let outline: Vec<[f64; 2]> = if self.hexagonal {
                (0..6)
                    .map(|k| {
                        let a = std::f64::consts::PI / 180.0 * (90.0 + 60.0 * f64::from(k));
                        from(cu + r * a.cos(), cv + r * a.sin())
                    })
                    .collect()
            } else {
                vec![
                    from(cu - 0.5, cv - 0.5),
                    from(cu + 0.5, cv - 0.5),
                    from(cu + 0.5, cv + 0.5),
                    from(cu - 0.5, cv + 0.5),
                ]
            };
            let t = if max > 1.0 { (count - 1.0).max(0.0) / (max - 1.0) } else { 1.0 };
            // Light (few) to dark (many): the ramp runs dark to light.
            let color = palette::ramp(1.0 - t).with_alpha(BIN_ALPHA);
            layer.marks.push(Mark::Polygon {
                points: outline,
                fill: color,
                stroke: color,
                name: "Rows per cell".to_owned(),
            });
        }
        layer.ramp = Some(Ramp { title: "Rows per cell".to_owned(), min: 1.0_f64.min(max), max, reversed: true });
        layer
    }
}
