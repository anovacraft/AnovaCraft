//! Filled and lined contour maps of a function sampled over a triangle mesh.
//!
//! The surface is taken as linear over each triangle of a
//! [`ContourField`]: a rectangular grid is split into four triangles per
//! cell around the cell's centre (whose value is the mean of the four
//! corners, which settles the saddle cells of marching squares the same
//! way), and a ternary (simplex) plot is covered by the regular triangular
//! lattice of its barycentric grid. On a linear triangle every contour
//! level is a straight chord and every band between two levels is a convex
//! polygon, so:
//!
//! * **Lines** ([`ContourField::isolines`]) are marching triangles: the
//!   chords of each triangle joined into polylines through the shared edge
//!   crossings.
//! * **Bands** ([`ContourField::isobands`]) are the isoband construction of
//!   d3-contour and the `contour` crate, done on triangles: each triangle's
//!   piece of each band is the convex polygon of its corners inside the band
//!   and the band's level crossings on its edges, walked round in order.
//!   The pieces tile the band exactly (they fill on screen as one mesh);
//!   their edges shared by two pieces cancel, and the edges left over join
//!   into the band's outline rings (outer edges and holes), filled with the
//!   even–odd rule in vector files so a band is one shape with no hairline
//!   seams.
//!
//! Lines and band edges are computed from the same crossing points, so the
//! lines lie exactly on the edges of the fills. Triangles with a missing
//! (non-finite) corner are left out: maps end at holes in the data.
//!
//! [`draw_contours`] turns a field and a [`ContourStyle`] into what a plot
//! draws: coloured bands, lines, their labels and the colour-bar key.

use std::collections::HashMap;

use crate::layer::Rgba;
use crate::palette::ColorScale;

/// A function sampled at the vertices of a triangle mesh, ready for
/// contouring.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContourField {
    /// Vertex positions (plot coordinates).
    pub points: Vec<[f64; 2]>,
    /// Value at each vertex (NaN where unknown).
    pub values: Vec<f64>,
    /// Triangles as vertex indices, anticlockwise.
    pub triangles: Vec<[usize; 3]>,
}

/// A point of a band outline or contour line, identified so that the same
/// point reached from two triangles compares equal: a mesh vertex, or the
/// crossing of level `k` on the edge between vertices `a < b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Vertex(usize),
    Crossing(usize, usize, usize),
}

/// One band of a filled contour map: values in `[lower, upper)` (no lower
/// bound for the first band, no upper bound for the last).
#[derive(Debug, Clone, PartialEq)]
pub struct Isoband {
    /// Band number: 0 below the first level, `k` between levels `k − 1` and `k`.
    pub index: usize,
    /// Convex pieces tiling the band (one per triangle it touches).
    pub pieces: Vec<Vec<[f64; 2]>>,
    /// The band's outline: closed rings (the last vertex joins the first),
    /// outer edges anticlockwise and holes clockwise; fill with the
    /// even–odd (or nonzero) rule.
    pub rings: Vec<Vec<[f64; 2]>>,
}

impl ContourField {
    /// A rectangular grid: `z[j][i]` is the value at `(xs[i], ys[j])`. Each
    /// cell becomes four triangles around its centre, which takes the mean
    /// of the corners (NaN if any is missing, so the cell is left out).
    #[must_use]
    pub fn from_grid(xs: &[f64], ys: &[f64], z: &[Vec<f64>]) -> Self {
        let (nx, ny) = (xs.len(), ys.len());
        if nx < 2 || ny < 2 || z.len() < ny || z.iter().take(ny).any(|row| row.len() < nx) {
            return Self::default();
        }
        let mut f = Self::default();
        for (j, &y) in ys.iter().enumerate() {
            for (i, &x) in xs.iter().enumerate() {
                f.points.push([x, y]);
                f.values.push(z[j][i]);
            }
        }
        let at = |i: usize, j: usize| j * nx + i;
        // Rising coordinates keep the triangles anticlockwise; falling ones flip them.
        let flip = (xs[1] < xs[0]) != (ys[1] < ys[0]);
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                let corners = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                let mean = 0.25 * corners.iter().map(|&c| f.values[c]).sum::<f64>();
                let centre = f.points.len();
                f.points.push([0.5 * (xs[i] + xs[i + 1]), 0.5 * (ys[j] + ys[j + 1])]);
                f.values.push(mean);
                for e in 0..4 {
                    let (a, b) = (corners[e], corners[(e + 1) % 4]);
                    f.triangles.push(if flip { [a, centre, b] } else { [a, b, centre] });
                }
            }
        }
        f
    }

    /// The simplex of three proportions `(a, b, c)` summing to one, split
    /// into `n²` triangles by the lattice of step `1/n`, with the value
    /// `value([a, b, c])` at each lattice point placed at `position([a, b, c])`.
    /// The position map must keep the corner order anticlockwise (as the
    /// ternary layout with A lower left, B lower right and C on top does).
    #[must_use]
    pub fn simplex(n: usize, value: impl Fn([f64; 3]) -> f64, position: impl Fn([f64; 3]) -> [f64; 2]) -> Self {
        let n = n.max(1);
        let mut f = Self::default();
        // Lattice point (i, j): b = i/n, c = j/n, i + j ≤ n.
        let mut index = vec![vec![usize::MAX; n + 1]; n + 1];
        for j in 0..=n {
            for i in 0..=n - j {
                let (b, c) = (i as f64 / n as f64, j as f64 / n as f64);
                let p = [(1.0 - b - c).max(0.0), b, c];
                index[j][i] = f.points.len();
                f.points.push(position(p));
                f.values.push(value(p));
            }
        }
        for j in 0..n {
            for i in 0..n - j {
                let (p, q, r) = (index[j][i], index[j][i + 1], index[j + 1][i]);
                f.triangles.push([p, q, r]);
                if i + 1 < n - j {
                    f.triangles.push([q, index[j + 1][i + 1], r]);
                }
            }
        }
        f
    }

    /// Triangles whose three corners have finite values.
    fn usable(&self) -> impl Iterator<Item = [usize; 3]> + '_ {
        self.triangles.iter().copied().filter(|t| t.iter().all(|&v| self.values.get(v).is_some_and(|z| z.is_finite())))
    }

    /// Smallest and largest value over the usable triangles (`None` when
    /// there are none).
    #[must_use]
    pub fn value_range(&self) -> Option<(f64, f64)> {
        let mut r = (f64::INFINITY, f64::NEG_INFINITY);
        for t in self.usable() {
            for v in t {
                r = (r.0.min(self.values[v]), r.1.max(self.values[v]));
            }
        }
        (r.0 <= r.1).then_some(r)
    }

    /// Extent of the usable triangles: `[x_min, y_min, x_max, y_max]`.
    #[must_use]
    pub fn extent(&self) -> Option<[f64; 4]> {
        let mut e = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for t in self.usable() {
            for v in t {
                let [x, y] = self.points[v];
                e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
            }
        }
        (e[0] <= e[2]).then_some(e)
    }

    fn position(&self, key: Key, levels: &[f64]) -> [f64; 2] {
        match key {
            Key::Vertex(v) => self.points[v],
            Key::Crossing(a, b, k) => {
                let (za, zb) = (self.values[a], self.values[b]);
                let t = if zb == za { 0.5 } else { ((levels[k] - za) / (zb - za)).clamp(0.0, 1.0) };
                let (p, q) = (self.points[a], self.points[b]);
                [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]
            }
        }
    }

    /// The crossing of level `k` on edge `(a, b)`, if the edge crosses it
    /// (one end below the level, the other at or above it).
    fn crossing(&self, a: usize, b: usize, k: usize, levels: &[f64]) -> Option<Key> {
        let l = levels[k];
        ((self.values[a] < l) != (self.values[b] < l)).then(|| Key::Crossing(a.min(b), a.max(b), k))
    }

    /// Contour lines at `level`: polylines (closed ones repeat their first
    /// point at the end).
    #[must_use]
    pub fn isolines(&self, level: f64) -> Vec<Vec<[f64; 2]>> {
        let levels = [level];
        let mut segments: Vec<(Key, Key)> = Vec::new();
        for [a, b, c] in self.usable() {
            let hits: Vec<Key> =
                [(a, b), (b, c), (c, a)].into_iter().filter_map(|(p, q)| self.crossing(p, q, 0, &levels)).collect();
            if let [p, q] = hits[..] {
                segments.push((p, q));
            }
        }
        join_segments(&segments)
            .into_iter()
            .map(|line| line.iter().map(|&k| self.position(k, &levels)).collect())
            .collect()
    }

    /// The bands between consecutive `levels` (ascending): `levels.len() + 1`
    /// bands, from below the first level to above the last. Bands the
    /// surface never enters have no pieces.
    #[must_use]
    pub fn isobands(&self, levels: &[f64]) -> Vec<Isoband> {
        let n_bands = levels.len() + 1;
        let band_of = |z: f64| levels.partition_point(|&l| l <= z);
        let mut pieces: Vec<Vec<Vec<Key>>> = vec![Vec::new(); n_bands];
        for tri in self.usable() {
            let bands: Vec<usize> = tri.iter().map(|&v| band_of(self.values[v])).collect();
            let (lo, hi) = (bands.iter().copied().min().unwrap_or(0), bands.iter().copied().max().unwrap_or(0));
            for (band, list) in pieces.iter_mut().enumerate().take(hi + 1).skip(lo) {
                let mut keys: Vec<Key> = Vec::new();
                for e in 0..3 {
                    let (a, b) = (tri[e], tri[(e + 1) % 3]);
                    if band_of(self.values[a]) == band {
                        keys.push(Key::Vertex(a));
                    }
                    // This band's lower (level band − 1) and upper (level band)
                    // crossings on the edge, in order from a.
                    let lower = band.checked_sub(1).and_then(|k| self.crossing(a, b, k, levels));
                    let upper = (band < levels.len()).then(|| self.crossing(a, b, band, levels)).flatten();
                    let rising = self.values[a] < self.values[b];
                    let ordered = if rising { [lower, upper] } else { [upper, lower] };
                    keys.extend(ordered.into_iter().flatten());
                }
                if keys.len() >= 3 {
                    list.push(keys);
                }
            }
        }
        pieces
            .into_iter()
            .enumerate()
            .map(|(index, list)| {
                let rings = outline_rings(&list);
                Isoband {
                    index,
                    pieces: list.iter().map(|p| p.iter().map(|&k| self.position(k, levels)).collect()).collect(),
                    rings: rings.iter().map(|r| r.iter().map(|&k| self.position(k, levels)).collect()).collect(),
                }
            })
            .collect()
    }
}

/// The edges of `pieces` (each a closed polygon of keys, all anticlockwise)
/// that no other piece shares, joined into closed rings.
fn outline_rings(pieces: &[Vec<Key>]) -> Vec<Vec<Key>> {
    let mut count: HashMap<(Key, Key), i32> = HashMap::new();
    for p in pieces {
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            if a == b {
                continue;
            }
            // An edge cancels against the same edge run the other way.
            match count.get_mut(&(b, a)) {
                Some(c) if *c > 0 => *c -= 1,
                _ => *count.entry((a, b)).or_insert(0) += 1,
            }
        }
    }
    let mut next: HashMap<Key, Vec<Key>> = HashMap::new();
    let mut edges: Vec<(Key, Key)> =
        count.into_iter().filter(|(_, c)| *c > 0).flat_map(|(e, c)| (0..c).map(move |_| e)).collect();
    // A fixed order, so the same field always gives the same file.
    edges.sort_by_key(|&(a, b)| (key_order(a), key_order(b)));
    for &(a, b) in &edges {
        next.entry(a).or_default().push(b);
    }
    let mut rings = Vec::new();
    for &(start, _) in &edges {
        while next.get(&start).is_some_and(|v| !v.is_empty()) {
            let mut ring = vec![start];
            let mut at = start;
            while let Some(b) = next.get_mut(&at).and_then(Vec::pop) {
                if b == start {
                    break;
                }
                ring.push(b);
                at = b;
            }
            if ring.len() >= 3 {
                rings.push(ring);
            }
        }
    }
    rings
}

fn key_order(k: Key) -> (usize, usize, usize, usize) {
    match k {
        Key::Vertex(v) => (0, v, 0, 0),
        Key::Crossing(a, b, l) => (1, a, b, l),
    }
}

/// Join segments that share end points into polylines.
fn join_segments(segments: &[(Key, Key)]) -> Vec<Vec<Key>> {
    let mut by_end: HashMap<Key, Vec<usize>> = HashMap::new();
    for (s, &(a, b)) in segments.iter().enumerate() {
        by_end.entry(a).or_default().push(s);
        by_end.entry(b).or_default().push(s);
    }
    let mut used = vec![false; segments.len()];
    let mut out = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (a, b) = segments[start];
        let mut chain = std::collections::VecDeque::from([a, b]);
        for forward in [true, false] {
            loop {
                let end = if forward { chain[chain.len() - 1] } else { chain[0] };
                let Some(s) = by_end.get(&end).and_then(|l| l.iter().copied().find(|&s| !used[s])) else { break };
                used[s] = true;
                let (p, q) = segments[s];
                let other = if p == end { q } else { p };
                if forward {
                    chain.push_back(other);
                } else {
                    chain.push_front(other);
                }
            }
        }
        out.push(chain.into_iter().collect());
    }
    out
}

/// About `n` contour levels at round values (steps of 1, 2, 2.5 or 5 × 10ᵏ)
/// strictly inside `(lo, hi)`, and the step. Empty when the range is empty
/// or `n` is zero.
#[must_use]
pub fn nice_levels(lo: f64, hi: f64, n: usize) -> (Vec<f64>, f64) {
    if n == 0 || !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return (Vec::new(), 0.0);
    }
    let raw = (hi - lo) / n as f64;
    let magnitude = 10f64.powf(raw.log10().floor());
    let m = raw / magnitude;
    let nice = [1.0, 2.0, 2.5, 5.0].into_iter().find(|&s| m <= s + 1e-9).unwrap_or(10.0);
    let step = nice * magnitude;
    let mut out = Vec::new();
    let mut k = (lo / step).floor() + 1.0;
    while k * step < hi && out.len() < 4 * n + 4 {
        // Divide by a whole power of ten rather than multiply by a
        // fraction, so 3 × 0.2 is 0.6 and not 0.6000000000000001.
        let v = if magnitude < 1.0 { k * nice / (1.0 / magnitude).round() } else { k * nice * magnitude };
        out.push(if v == 0.0 { 0.0 } else { v });
        k += 1.0;
    }
    (out, step)
}

/// How a contour map is drawn: filled bands and their colour scale, level
/// lines and labels, and the levels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContourStyle {
    /// Fill the bands between levels.
    pub fill: bool,
    /// Draw the level lines.
    pub lines: bool,
    /// Write each level's value on its longest line.
    pub labels: bool,
    /// Colour scale of the fill.
    pub scale: ColorScale,
    /// Run the scale from high to low.
    pub reverse: bool,
    /// About this many levels; `None` keeps the plot's own number.
    pub levels: Option<usize>,
    /// Fixed `(low, high)` range of the levels and colours; `None` spans
    /// the surface.
    pub range: Option<(f64, f64)>,
    /// Value at the middle of a diverging scale (a target, say); `None`
    /// puts the middle at the centre of the range.
    pub centre: Option<f64>,
    /// Opacity of the fill (0–1).
    pub opacity: f64,
}

impl Default for ContourStyle {
    fn default() -> Self {
        Self {
            fill: true,
            lines: true,
            labels: true,
            scale: ColorScale::Sequential,
            reverse: false,
            levels: None,
            range: None,
            centre: None,
            opacity: 1.0,
        }
    }
}

impl ContourStyle {
    /// Lines and labels only, with no fill (the look before filled maps).
    #[must_use]
    pub fn lines_only() -> Self {
        Self { fill: false, ..Self::default() }
    }

    /// Position on the colour scale (0–1) of value `v` when the levels and
    /// colours span `(lo, hi)`: reversed when asked, and, for the diverging
    /// scale, with the centre at ½ and the farther end of the range at 0 or 1.
    #[must_use]
    pub fn scale_position(&self, v: f64, lo: f64, hi: f64) -> f64 {
        let t = if self.scale == ColorScale::Diverging {
            let c = self.centre.unwrap_or(0.5 * (lo + hi));
            let half = (lo - c).abs().max((hi - c).abs());
            if half > 0.0 { 0.5 + 0.5 * (v - c) / half } else { 0.5 }
        } else if hi > lo {
            (v - lo) / (hi - lo)
        } else {
            0.5
        };
        let t = t.clamp(0.0, 1.0);
        if self.reverse { 1.0 - t } else { t }
    }
}

/// A colour-bar key: the bands' colours from low to high and the level
/// values between them.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorBar {
    /// Title (the mapped quantity).
    pub title: String,
    /// Band colours, low to high (one more than the levels).
    pub colors: Vec<Rgba>,
    /// Level values between consecutive bands, written as labels.
    pub labels: Vec<String>,
}

/// Everything a contour map draws, in plot coordinates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContourDrawing {
    /// The levels.
    pub levels: Vec<f64>,
    /// Spacing of the levels (0 with none).
    pub step: f64,
    /// Filled bands with their colours (empty without fill).
    pub bands: Vec<(Rgba, Isoband)>,
    /// Lines at each level (empty without lines).
    pub lines: Vec<(f64, Vec<Vec<[f64; 2]>>)>,
    /// Each level's value at the middle of its longest line, as
    /// `(position, level, text)` (empty without labels).
    pub labels: Vec<([f64; 2], f64, String)>,
    /// The colour bar (with fill only).
    pub bar: Option<ColorBar>,
}

/// What `field` draws in `style`: about `default_levels` levels (unless the
/// style sets a number) at round values over the surface's range or the
/// style's fixed range, filled bands coloured at their middle values (the
/// outer bands at the middle between the range's end and the nearest
/// level), lines and labels as asked. Colours use the dark variant of the
/// scale when `dark`; the fill's opacity is in each band colour's alpha.
/// `title` heads the colour bar and `format` writes the level values.
#[must_use]
pub fn draw_contours(
    field: &ContourField,
    style: &ContourStyle,
    default_levels: usize,
    title: &str,
    dark: bool,
    format: &dyn Fn(f64) -> String,
) -> ContourDrawing {
    let Some(data) = field.value_range() else { return ContourDrawing::default() };
    let (lo, hi) = style.range.filter(|r| r.1 > r.0 && r.0.is_finite() && r.1.is_finite()).unwrap_or(data);
    let (levels, step) = nice_levels(lo, hi, style.levels.unwrap_or(default_levels));
    let mut out = ContourDrawing { levels: levels.clone(), step, ..ContourDrawing::default() };
    if style.fill {
        // Band k spans [levels[k−1], levels[k]); the outer ones end at the range.
        let middle = |k: usize| {
            let below = if k == 0 { lo.min(*levels.first().unwrap_or(&lo)) } else { levels[k - 1] };
            let above = if k == levels.len() { hi.max(*levels.last().unwrap_or(&hi)) } else { levels[k] };
            0.5 * (below + above)
        };
        let colors: Vec<Rgba> = (0..=levels.len())
            .map(|k| style.scale.color(style.scale_position(middle(k), lo, hi), dark).with_alpha(style.opacity))
            .collect();
        out.bands = field
            .isobands(&levels)
            .into_iter()
            .filter(|b| !b.pieces.is_empty())
            .map(|b| (colors[b.index], b))
            .collect();
        out.bar = Some(ColorBar {
            title: title.to_owned(),
            colors: colors.iter().map(|c| c.with_alpha(1.0)).collect(),
            labels: levels.iter().map(|&l| format(l)).collect(),
        });
    }
    if style.lines || style.labels {
        for &level in &levels {
            let lines = field.isolines(level);
            if style.labels {
                if let Some(longest) = lines.iter().max_by_key(|l| l.len()) {
                    out.labels.push((longest[longest.len() / 2], level, format(level)));
                }
            }
            if style.lines && !lines.is_empty() {
                out.lines.push((level, lines));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shoelace area (positive anticlockwise).
    fn area(ring: &[[f64; 2]]) -> f64 {
        let n = ring.len();
        0.5 * (0..n).map(|i| ring[i][0] * ring[(i + 1) % n][1] - ring[(i + 1) % n][0] * ring[i][1]).sum::<f64>()
    }

    /// z = x on the unit square, 3 × 3 nodes.
    fn ramp_x() -> ContourField {
        let xs = [0.0, 0.5, 1.0];
        let z: Vec<Vec<f64>> = (0..3).map(|_| xs.to_vec()).collect();
        ContourField::from_grid(&xs, &xs, &z)
    }

    /// A cone 1 − r over [−1, 1]², `n + 1` nodes a side.
    fn cone(n: usize) -> ContourField {
        let xs: Vec<f64> = (0..=n).map(|i| -1.0 + 2.0 * i as f64 / n as f64).collect();
        let z: Vec<Vec<f64>> = xs.iter().map(|&y| xs.iter().map(|&x| 1.0 - (x * x + y * y).sqrt()).collect()).collect();
        ContourField::from_grid(&xs, &xs, &z)
    }

    #[test]
    fn grid_is_four_anticlockwise_triangles_per_cell() {
        let f = ramp_x();
        assert_eq!(f.triangles.len(), 16);
        assert_eq!(f.points.len(), 9 + 4);
        for t in &f.triangles {
            assert!(area(&t.map(|v| f.points[v])) > 0.0);
        }
        assert_eq!(f.value_range(), Some((0.0, 1.0)));
        assert_eq!(f.extent(), Some([0.0, 0.0, 1.0, 1.0]));
    }

    #[test]
    fn bands_of_a_plane_are_strips_with_exact_areas() {
        let f = ramp_x();
        let bands = f.isobands(&[0.25, 0.6]);
        assert_eq!(bands.len(), 3);
        let expected = [0.25, 0.35, 0.4];
        for (b, want) in bands.iter().zip(expected) {
            let pieces: f64 = b.pieces.iter().map(|p| area(p)).sum();
            let rings: f64 = b.rings.iter().map(|r| area(r)).sum();
            assert!((pieces - want).abs() < 1e-12, "band {} pieces {pieces}", b.index);
            assert!((rings - want).abs() < 1e-12, "band {} rings {rings}", b.index);
            assert_eq!(b.rings.len(), 1, "one strip, no holes");
            assert!(b.pieces.iter().all(|p| area(p) > -1e-15), "pieces are anticlockwise (or empty)");
        }
        // The bands tile the square.
        let total: f64 = bands.iter().flat_map(|b| &b.rings).map(|r| area(r)).sum();
        assert!((total - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cone_bands_nest_as_discs_and_rings_with_holes() {
        let f = cone(40);
        let levels = [0.25, 0.5, 0.75];
        let bands = f.isobands(&levels);
        // The top band is a disc of radius 0.25: one outer ring, area ≈ π/16.
        let top = &bands[3];
        assert_eq!(top.rings.len(), 1);
        assert!((area(&top.rings[0]) - std::f64::consts::PI / 16.0).abs() < 0.01);
        // Middle bands are annuli: an anticlockwise outer ring and a clockwise hole.
        for (k, b) in bands.iter().enumerate().take(3).skip(1) {
            assert_eq!(b.rings.len(), 2, "band {k}");
            let mut areas: Vec<f64> = b.rings.iter().map(|r| area(r)).collect();
            areas.sort_by(f64::total_cmp);
            assert!(areas[0] < 0.0 && areas[1] > 0.0, "band {k}: {areas:?}");
            let (r_out, r_in) = (1.0 - levels[k - 1], 1.0 - levels[k]);
            let want = std::f64::consts::PI * (r_out * r_out - r_in * r_in);
            let net = areas[0] + areas[1];
            assert!((net - want).abs() < 0.02, "band {k}: {net} vs {want}");
            let pieces: f64 = b.pieces.iter().map(|p| area(p)).sum();
            assert!((pieces - net).abs() < 1e-9, "pieces and outline agree");
        }
        // Every ring is closed: consecutive points are near (no long jumps).
        for b in &bands {
            for r in &b.rings {
                let n = r.len();
                for i in 0..n {
                    let (p, q) = (r[i], r[(i + 1) % n]);
                    assert!((p[0] - q[0]).hypot(p[1] - q[1]) < 0.1, "a ring jumps between {p:?} and {q:?}");
                }
            }
        }
        let total: f64 = bands.iter().flat_map(|b| &b.rings).map(|r| area(r)).sum();
        assert!((total - 4.0).abs() < 1e-9);
    }

    #[test]
    fn lines_lie_on_band_edges_and_close_round_the_cone() {
        let f = cone(30);
        let lines = f.isolines(0.5);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.first(), line.last(), "closed");
        for p in line {
            assert!((p[0].hypot(p[1]) - 0.5).abs() < 0.01);
        }
        assert!(f.isolines(2.0).is_empty());
    }

    #[test]
    fn missing_values_leave_holes() {
        let xs = [0.0, 1.0, 2.0];
        let z = vec![vec![0.0, 1.0, 2.0], vec![0.0, f64::NAN, 2.0], vec![0.0, 1.0, 2.0]];
        let f = ContourField::from_grid(&xs, &xs, &z);
        // Every cell touches the missing centre node.
        assert_eq!(f.value_range(), None);
        assert!(f.isobands(&[1.0]).iter().all(|b| b.pieces.is_empty()));
        assert!(f.isolines(0.5).is_empty());
    }

    #[test]
    fn simplex_lattice_covers_the_triangle() {
        let pos = |p: [f64; 3]| [p[1] + p[2] / 2.0, p[2] * 3f64.sqrt() / 2.0];
        let f = ContourField::simplex(6, |p| p[0], pos);
        assert_eq!(f.triangles.len(), 36);
        let total: f64 = f.triangles.iter().map(|t| area(&t.map(|v| f.points[v]))).sum();
        assert!((total - 3f64.sqrt() / 4.0).abs() < 1e-12);
        // a = proportion of A: the band a ≥ ½ is the corner triangle of a quarter of the area.
        let bands = f.isobands(&[0.5]);
        let top: f64 = bands[1].rings.iter().map(|r| area(r)).sum();
        assert!((top - 3f64.sqrt() / 16.0).abs() < 1e-12);
    }

    #[test]
    fn nice_levels_are_round() {
        assert_eq!(nice_levels(0.0, 1.0, 4), (vec![0.25, 0.5, 0.75], 0.25));
        assert_eq!(nice_levels(-3.2, 7.9, 5), (vec![-2.5, 0.0, 2.5, 5.0, 7.5], 2.5));
        assert_eq!(nice_levels(1.0, 1.0, 5).0, Vec::<f64>::new());
        assert!(nice_levels(0.0, 1.0, 0).0.is_empty());
    }

    #[test]
    fn drawing_follows_the_style() {
        let f = cone(20);
        let fmt = |v: f64| format!("{v}");
        let (lo, hi) = f.value_range().unwrap();
        assert_eq!(hi, 1.0);
        let style = ContourStyle::default();
        let d = draw_contours(&f, &style, 4, "Height", false, &fmt);
        assert_eq!((d.levels.clone(), d.step), (vec![0.0, 0.5], 0.5));
        let bar = d.bar.as_ref().expect("a colour bar with fill");
        assert_eq!(bar.colors.len(), 3);
        assert_eq!(bar.labels, ["0", "0.5"]);
        assert_eq!(bar.title, "Height");
        // Each band is coloured at its middle; the outer ones end at the range.
        let mids = [0.5 * lo, 0.25, 0.75];
        for (k, m) in mids.into_iter().enumerate() {
            assert_eq!(bar.colors[k], ColorScale::Sequential.color(style.scale_position(m, lo, hi), false));
        }
        assert_eq!(d.bands.len(), 3);
        assert_eq!((d.lines.len(), d.labels.len()), (2, 2));
        assert_eq!((d.labels[1].1, d.labels[1].2.as_str()), (0.5, "0.5"));
        // Lines only: no bands or bar; labels can go too.
        let plain = ContourStyle { fill: false, labels: false, ..ContourStyle::default() };
        let d = draw_contours(&f, &plain, 4, "Height", false, &fmt);
        assert!(d.bands.is_empty() && d.bar.is_none() && d.labels.is_empty());
        assert_eq!(d.lines.len(), 2);
        // A fixed range and level count; reversed colours; opacity in alpha.
        let fixed = ContourStyle {
            range: Some((0.0, 2.0)),
            levels: Some(2),
            reverse: true,
            opacity: 0.5,
            ..ContourStyle::default()
        };
        let d = draw_contours(&cone(21), &fixed, 4, "", true, &fmt);
        assert_eq!(d.levels, vec![1.0]);
        assert_eq!(d.bands.len(), 1, "the surface stays below 1");
        assert_eq!(d.bands[0].0, ColorScale::Sequential.color(0.75, true).with_alpha(0.5));
    }

    #[test]
    fn diverging_scale_centres_on_the_target() {
        let s = ContourStyle { scale: ColorScale::Diverging, centre: Some(10.0), ..ContourStyle::default() };
        assert_eq!(s.scale_position(10.0, 0.0, 40.0), 0.5);
        assert_eq!(s.scale_position(40.0, 0.0, 40.0), 1.0);
        assert_eq!(s.scale_position(0.0, 0.0, 40.0), 0.5 - 10.0 / 60.0);
        let plain = ContourStyle::default();
        assert_eq!(plain.scale_position(10.0, 0.0, 40.0), 0.25);
        assert_eq!(ContourStyle { reverse: true, ..plain }.scale_position(10.0, 0.0, 40.0), 0.75);
    }
}
