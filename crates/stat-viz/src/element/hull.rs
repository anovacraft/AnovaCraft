//! Convex hulls around the points of each subgroup.

use stat_data::RowStateManager;

use super::{Element, ElementKind, both_numeric, group_prefix, marker_color};
use crate::frame::Panel;
use crate::layer::{KeyGlyph, Layer, LinePattern, Mark, Region};

/// The convex hull of `points` (non-finite points ignored), counter-clockwise
/// from the lowest-leftmost vertex, without repeated or collinear vertices.
///
/// Andrew's monotone chain (Andrew, "Another efficient algorithm for convex
/// hulls in two dimensions", *Information Processing Letters* 9(5), 1979):
/// sort the points by `x` then `y`, build the lower chain left to right and
/// the upper chain right to left, popping the last vertex while it does not
/// make a strict left (counter-clockwise) turn, and join the chains.
/// `O(n log n)`. Turns straight to within `10⁻¹²` of the edge lengths
/// count as straight. Fewer than three distinct points give those points (one or
/// two); points all on a line give the line's two ends.
#[must_use]
pub fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = points.iter().copied().filter(|q| q[0].is_finite() && q[1].is_finite()).collect();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    // Cross product of (a − o) × (b − o), positive for a left turn; turns
    // within rounding of straight (relative to the edge lengths) count as
    // straight, so points on an edge are dropped as Qhull drops them.
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        let (ax, ay, bx, by) = (a[0] - o[0], a[1] - o[1], b[0] - o[0], b[1] - o[1]);
        let c = ax * by - ay * bx;
        if c.abs() <= 1e-12 * ax.hypot(ay) * bx.hypot(by) { 0.0 } else { c }
    };
    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(2 * p.len());
    for &q in &p {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0 {
            hull.pop();
        }
        hull.push(q);
    }
    let lower = hull.len() + 1;
    for &q in p.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0 {
            hull.pop();
        }
        hull.push(q);
    }
    hull.pop();
    hull
}

/// Line patterns of the hulls, by subgroup, so neighboring groups differ by
/// pattern as well as color.
const PATTERNS: [LinePattern; 3] = [LinePattern::Solid, LinePattern::Dashed, LinePattern::Dotted];

/// Convex hulls: the smallest convex polygon around the drawn points of each
/// subgroup (color or subgroup column), outlined in the subgroup's color
/// with its own line pattern (solid, dashed, dotted in turn), optionally
/// with a light fill. Excluded rows and rows outside the graph's filter are
/// left out. Clicking inside a hull selects its rows. Panels need two
/// continuous axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HullElement {
    /// Shade the inside of each hull lightly.
    pub fill: bool,
}

impl Default for HullElement {
    fn default() -> Self {
        Self { fill: true }
    }
}

impl Element for HullElement {
    fn kind(&self) -> ElementKind {
        ElementKind::Hull
    }

    fn layer(&self, panel: &Panel, _state: &RowStateManager) -> Layer {
        let mut layer = Layer::default();
        if !both_numeric(panel) {
            layer.notes.push("Convex hulls need continuous columns on both axes.".to_owned());
            return layer;
        }
        let n_groups = panel.groups.len().max(1);
        let mut members: Vec<(Vec<[f64; 2]>, Vec<usize>)> = vec![(Vec::new(), Vec::new()); n_groups];
        for p in panel.points.iter().filter(|p| !p.excluded && !p.filtered) {
            let g = p.group.min(n_groups - 1);
            members[g].0.push([p.x, p.y]);
            members[g].1.push(p.row);
        }
        for (g, (points, mut rows)) in members.into_iter().enumerate() {
            let hull = convex_hull(&points);
            if hull.len() < 2 {
                continue;
            }
            let color = marker_color(panel, g);
            let pattern = PATTERNS[g % PATTERNS.len()];
            let name = format!("{}convex hull", group_prefix(panel, g));
            if self.fill && hull.len() >= 3 {
                layer.marks.push(Mark::Polygon {
                    points: hull.clone(),
                    fill: color.with_alpha(0.1),
                    stroke: color.with_alpha(0.0),
                    name: name.clone(),
                });
            }
            let mut ring = hull.clone();
            ring.push(hull[0]);
            layer.marks.push(Mark::Line { points: ring, color, width: 1.6, pattern, name });
            if hull.len() >= 3 {
                rows.sort_unstable();
                rows.dedup();
                layer.regions.push(Region { outline: hull, rows });
            }
            let label = if panel.groups.len() > 1 {
                format!("Convex hull: {}", panel.groups[g].label)
            } else {
                "Convex hull".to_owned()
            };
            layer.key_entry(label, color, KeyGlyph::Line(pattern));
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotone_chain_finds_the_hull() {
        // A square with interior, edge and repeated points.
        let pts = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [1.0, 1.0], [1.0, 0.0], [2.0, 2.0], [0.5, 1.5]];
        assert_eq!(convex_hull(&pts), [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]);
        // Collinear points: the two ends.
        assert_eq!(convex_hull(&[[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]), [[0.0, 0.0], [2.0, 2.0]]);
        assert_eq!(convex_hull(&[[1.0, 1.0], [f64::NAN, 0.0]]), [[1.0, 1.0]]);
        assert!(convex_hull(&[]).is_empty());
        // Every input point is inside or on the hull, which turns left throughout.
        let cloud: Vec<[f64; 2]> =
            (0..200).map(|i| [((i * 73) % 97) as f64 / 9.0, ((i * 31) % 89) as f64 / 7.0]).collect();
        let hull = convex_hull(&cloud);
        let k = hull.len();
        // SciPy 1.18 `spatial.ConvexHull` (Qhull): 11 vertices, area 128.4047619047619.
        assert_eq!(k, 11, "{hull:?}");
        let area =
            0.5 * (0..k).map(|i| hull[i][0] * hull[(i + 1) % k][1] - hull[(i + 1) % k][0] * hull[i][1]).sum::<f64>();
        assert!((area - 128.404_761_904_761_9).abs() < 1e-9, "{area}");
        assert_eq!(hull[1], [10.555_555_555_555_555, 0.0]);
        for i in 0..k {
            let (a, b) = (hull[i], hull[(i + 1) % k]);
            for p in &cloud {
                let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                assert!(c >= -1e-9, "{p:?} is outside edge {a:?}–{b:?}");
            }
        }
    }
}
