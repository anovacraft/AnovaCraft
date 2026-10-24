//! Surfaces through scattered points: a response `z` observed at irregular
//! locations `(x, y)` turned into values on a regular grid, for contour and
//! surface plots of data.
//!
//! Two methods are offered:
//!
//! * **Delaunay triangulation with linear interpolation** ([`LinearSurface`]).
//!   The locations are joined into the Delaunay triangulation (the
//!   triangulation whose triangles have no location inside their
//!   circumcircles; Delaunay 1934; Lee & Schachter 1980), built by
//!   incremental insertion with edge flips (Lawson 1977; de Berg et al.,
//!   *Computational Geometry*, 3rd ed., 2008, ch. 9). Inside each triangle the
//!   surface is the plane through its three corners, so it passes through
//!   every observation; outside the convex hull of the locations it is
//!   undefined (NaN). The orientation and in-circle tests use exact integer
//!   arithmetic on coordinates snapped to a 2²⁴ grid over the data range, so
//!   the triangulation is valid even for gridded designs where four
//!   locations share a circle.
//! * **Thin-plate spline** ([`ThinPlateSpline`]; Duchon 1977; Wahba 1990,
//!   *Spline Models for Observational Data*, ch. 2). The function `f`
//!   minimising
//!
//!   ```text
//!   (1/n) Σ (zᵢ − f(xᵢ, yᵢ))² + λ ∬ (f_xx² + 2 f_xy² + f_yy²) dx dy
//!   ```
//!
//!   is `f(p) = Σ cᵢ E(‖p − pᵢ‖) + d₀ + d₁x + d₂y` with `E(r) = r² ln r / (8π)`,
//!   where `(K + nλI) c + T d = z`, `Tᵀc = 0`, `Kᵢⱼ = E(‖pᵢ − pⱼ‖)` and `T` has
//!   rows `(1, xᵢ, yᵢ)`. With `λ = 0` the spline interpolates the data; with
//!   `λ` chosen by generalized cross-validation (Craven & Wahba 1979) it
//!   smooths them. The system is solved through the QR factorization of `T`
//!   and the eigen-decomposition of `Q₂ᵀ K Q₂` (Wahba 1990, §1.3), which makes
//!   the GCV score cheap to evaluate for any `λ`.
//!
//! Both methods work on coordinates scaled to `[0, 1]` over the data range of
//! each column, so the units of `x` and `y` do not matter. Observations at
//! the same location (closer than 1/2²⁴ of either range) are averaged into
//! one.

use stat_core::{Result, StatError};

/// Most distinct locations a thin-plate spline is fitted to (the fit costs
/// `O(n³)`); larger data sets use the triangulation.
pub const MAX_SPLINE_POINTS: usize = 1000;

/// Snapping grid of the exact triangulation predicates (2²⁴ steps over each
/// coordinate's range).
const SNAP: f64 = 16_777_216.0;

/// Scattered observations, merged where they share a location.
#[derive(Debug, Clone, PartialEq)]
pub struct ScatteredPoints {
    /// Horizontal position of each location.
    pub x: Vec<f64>,
    /// Vertical position of each location.
    pub y: Vec<f64>,
    /// Response at each location (the mean of the observations there).
    pub z: Vec<f64>,
    /// The input observations (indices) merged into each location.
    pub members: Vec<Vec<usize>>,
    /// Smallest and largest `x`.
    pub x_range: (f64, f64),
    /// Smallest and largest `y`.
    pub y_range: (f64, f64),
}

impl ScatteredPoints {
    /// Merge observations `(x[i], y[i], z[i])` that share a location.
    ///
    /// # Errors
    /// [`StatError::DimensionMismatch`] if the lengths differ;
    /// [`StatError::NonFiniteInput`] for NaN or infinite values;
    /// [`StatError::InvalidArgument`] if `x` or `y` takes a single value;
    /// [`StatError::InsufficientData`] for fewer than three distinct
    /// locations.
    pub fn new(x: &[f64], y: &[f64], z: &[f64]) -> Result<Self> {
        if y.len() != x.len() {
            return Err(StatError::DimensionMismatch { context: "y vs x", expected: x.len(), found: y.len() });
        }
        if z.len() != x.len() {
            return Err(StatError::DimensionMismatch { context: "z vs x", expected: x.len(), found: z.len() });
        }
        if x.iter().chain(y).chain(z).any(|v| !v.is_finite()) {
            return Err(StatError::NonFiniteInput { context: "scattered points" });
        }
        let range =
            |v: &[f64]| v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &a| (lo.min(a), hi.max(a)));
        let (x_range, y_range) = (range(x), range(y));
        if x.len() < 3 {
            return Err(StatError::InsufficientData { required: 3, found: x.len() });
        }
        for (name, (lo, hi)) in [("horizontal", x_range), ("vertical", y_range)] {
            if hi <= lo {
                return Err(invalid(format!("the {name} column takes a single value, so there is no surface")));
            }
        }
        let snap = |v: f64, (lo, hi): (f64, f64)| ((v - lo) / (hi - lo) * SNAP).round() as i64;
        let mut order: Vec<usize> = (0..x.len()).collect();
        let key = |i: usize| (snap(x[i], x_range), snap(y[i], y_range));
        order.sort_by_key(|&i| (key(i), i));
        let mut members: Vec<Vec<usize>> = Vec::new();
        let mut last = None;
        for i in order {
            let k = key(i);
            if last == Some(k) {
                members.last_mut().expect("a location").push(i);
            } else {
                members.push(vec![i]);
                last = Some(k);
            }
        }
        // Keep the locations in the order of their first observation.
        members.sort_by_key(|m| m.iter().copied().min().unwrap_or(0));
        for m in &mut members {
            m.sort_unstable();
        }
        if members.len() < 3 {
            return Err(StatError::InsufficientData { required: 3, found: members.len() });
        }
        let mean = |v: &[f64], m: &[usize]| m.iter().map(|&i| v[i]).sum::<f64>() / m.len() as f64;
        Ok(Self {
            x: members.iter().map(|m| mean(x, m)).collect(),
            y: members.iter().map(|m| mean(y, m)).collect(),
            z: members.iter().map(|m| mean(z, m)).collect(),
            members,
            x_range,
            y_range,
        })
    }

    /// Number of distinct locations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// `true` when there are no locations (never, for a constructed value).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// `(x, y)` scaled to `[0, 1]` over the data ranges.
    #[must_use]
    pub fn unit(&self, x: f64, y: f64) -> [f64; 2] {
        let (x0, x1) = self.x_range;
        let (y0, y1) = self.y_range;
        [(x - x0) / (x1 - x0), (y - y0) / (y1 - y0)]
    }

    fn snapped(&self, i: usize) -> [i64; 2] {
        let [u, v] = self.unit(self.x[i], self.y[i]);
        [(u * SNAP).round() as i64, (v * SNAP).round() as i64]
    }
}

// ---------------------------------------------------------------------------
// Delaunay triangulation
// ---------------------------------------------------------------------------

/// `> 0` if `a, b, c` turn counter-clockwise, `< 0` clockwise, `0` collinear.
fn orient(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i128 {
    let w = |v: i64| i128::from(v);
    (w(b[0]) - w(a[0])) * (w(c[1]) - w(a[1])) - (w(b[1]) - w(a[1])) * (w(c[0]) - w(a[0]))
}

/// `> 0` if `d` lies strictly inside the circle through the counter-clockwise
/// triangle `a, b, c`.
fn in_circle(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> i128 {
    let w = |v: i64| i128::from(v);
    let (adx, ady) = (w(a[0]) - w(d[0]), w(a[1]) - w(d[1]));
    let (bdx, bdy) = (w(b[0]) - w(d[0]), w(b[1]) - w(d[1]));
    let (cdx, cdy) = (w(c[0]) - w(d[0]), w(c[1]) - w(d[1]));
    (adx * adx + ady * ady) * (bdx * cdy - cdx * bdy)
        + (bdx * bdx + bdy * bdy) * (cdx * ady - adx * cdy)
        + (cdx * cdx + cdy * cdy) * (adx * bdy - bdx * ady)
}

/// A triangulation under construction: counter-clockwise triangles and, for
/// each, the triangle across the edge opposite each corner.
struct Mesh {
    p: Vec<[i64; 2]>,
    tri: Vec<[usize; 3]>,
    nbr: Vec<[Option<usize>; 3]>,
}

impl Mesh {
    /// Slot of `t`'s neighbour `u` (the corner of `t` opposite their shared edge).
    fn slot(&self, t: usize, u: usize) -> usize {
        (0..3).find(|&k| self.nbr[t][k] == Some(u)).expect("adjacent triangles")
    }

    /// Point `t`'s neighbour pointer that was `old` at `new` instead.
    fn repoint(&mut self, t: Option<usize>, old: usize, new: usize) {
        if let Some(t) = t {
            let k = self.slot(t, old);
            self.nbr[t][k] = Some(new);
        }
    }

    /// Restore the Delaunay property around the edge opposite corner `k` of
    /// `t` (and recursively the edges a flip exposes).
    fn legalize(&mut self, t: usize, k: usize) {
        let mut stack = vec![(t, k)];
        while let Some((t, k)) = stack.pop() {
            let Some(u) = self.nbr[t][k] else { continue };
            let m = self.slot(u, t);
            let [p, a, b] = rotate(self.tri[t], k);
            let q = self.tri[u][m];
            let [ta, tb, tc] = self.tri[t];
            if in_circle(self.p[ta], self.p[tb], self.p[tc], self.p[q]) <= 0 {
                continue;
            }
            // t = (p, a, b), u = (q, b, a): flip the diagonal a–b to p–q.
            let x1 = self.nbr[t][(k + 1) % 3]; // across (b, p)
            let x2 = self.nbr[t][(k + 2) % 3]; // across (p, a)
            let y1 = self.nbr[u][(m + 1) % 3]; // across (a, q)
            let y2 = self.nbr[u][(m + 2) % 3]; // across (q, b)
            debug_assert_eq!(rotate(self.tri[u], m), [q, b, a]);
            self.tri[t] = [p, a, q];
            self.nbr[t] = [y1, Some(u), x2];
            self.tri[u] = [q, b, p];
            self.nbr[u] = [x1, Some(t), y2];
            self.repoint(y1, u, t);
            self.repoint(x1, t, u);
            stack.push((t, 0));
            stack.push((u, 2));
        }
    }

    /// Where `q` lies: inside or on the edges of a triangle (`Ok`), or
    /// outside the hull (`Err`).
    fn locate(&self, q: [i64; 2], start: usize) -> std::result::Result<usize, ()> {
        let mut t = start;
        let limit = 4 * self.tri.len() + 16;
        for _ in 0..limit {
            let [a, b, c] = self.tri[t];
            let corners = [a, b, c];
            let mut moved = false;
            for k in 0..3 {
                let (e0, e1) = (corners[(k + 1) % 3], corners[(k + 2) % 3]);
                if orient(self.p[e0], self.p[e1], q) < 0 {
                    match self.nbr[t][k] {
                        Some(u) => {
                            t = u;
                            moved = true;
                            break;
                        }
                        None => return Err(()),
                    }
                }
            }
            if !moved {
                return Ok(t);
            }
        }
        // A walk that does not settle (never expected in a Delaunay
        // triangulation): search every triangle.
        (0..self.tri.len())
            .find(|&t| {
                let [a, b, c] = self.tri[t];
                [(b, c), (c, a), (a, b)].iter().all(|&(e0, e1)| orient(self.p[e0], self.p[e1], q) >= 0)
            })
            .ok_or(())
    }

    /// Insert point `i`; returns a triangle touching it.
    fn insert(&mut self, i: usize, start: usize) -> usize {
        let q = self.p[i];
        match self.locate(q, start) {
            Ok(t) => {
                let corners = self.tri[t];
                let on_edge =
                    (0..3).find(|&k| orient(self.p[corners[(k + 1) % 3]], self.p[corners[(k + 2) % 3]], q) == 0);
                match on_edge {
                    None => self.split_triangle(t, i),
                    Some(k) => self.split_edge(t, k, i),
                }
            }
            Err(()) => self.extend_hull(i),
        }
    }

    fn split_triangle(&mut self, t: usize, i: usize) -> usize {
        let [a, b, c] = self.tri[t];
        let [na, nb, nc] = self.nbr[t];
        let (t1, t2) = (self.tri.len(), self.tri.len() + 1);
        // t = (a, b, i), t1 = (b, c, i), t2 = (c, a, i).
        self.tri[t] = [a, b, i];
        self.nbr[t] = [Some(t1), Some(t2), nc];
        self.tri.push([b, c, i]);
        self.nbr.push([Some(t2), Some(t), na]);
        self.tri.push([c, a, i]);
        self.nbr.push([Some(t), Some(t1), nb]);
        self.repoint(na, t, t1);
        self.repoint(nb, t, t2);
        for tt in [t, t1, t2] {
            self.legalize(tt, 2);
        }
        t
    }

    /// Insert `i` on the edge opposite corner `k` of `t`.
    fn split_edge(&mut self, t: usize, k: usize, i: usize) -> usize {
        let [c, a, b] = rotate(self.tri[t], k);
        let n_ca = self.nbr[t][(k + 1) % 3]; // across (b, c)
        let n_cb = self.nbr[t][(k + 2) % 3]; // across (c, a)
        let across = self.nbr[t][k];
        let t1 = self.tri.len();
        // t = (c, a, i), t1 = (b, c, i).
        self.tri[t] = [c, a, i];
        self.tri.push([b, c, i]);
        match across {
            None => {
                self.nbr[t] = [None, Some(t1), n_cb];
                self.nbr.push([Some(t), None, n_ca]);
                self.repoint(n_ca, t, t1);
                self.legalize(t, 2);
                self.legalize(t1, 2);
            }
            Some(u) => {
                let m = self.slot(u, t);
                let [d, b2, a2] = rotate(self.tri[u], m);
                debug_assert_eq!((a2, b2), (a, b));
                let n_da = self.nbr[u][(m + 1) % 3]; // across (a, d)
                let n_db = self.nbr[u][(m + 2) % 3]; // across (d, b)
                let u1 = self.tri.len();
                // u = (d, b, i), u1 = (a, d, i).
                self.tri[u] = [d, b, i];
                self.tri.push([a, d, i]);
                self.nbr[t] = [Some(u1), Some(t1), n_cb];
                self.nbr.push([Some(t), Some(u), n_ca]);
                self.nbr[u] = [Some(t1), Some(u1), n_db];
                self.nbr.push([Some(u), Some(t), n_da]);
                self.repoint(n_ca, t, t1);
                self.repoint(n_da, u, u1);
                for tt in [t, t1, u, u1] {
                    self.legalize(tt, 2);
                }
            }
        }
        t
    }

    /// Insert `i`, outside the current hull, joining it to every hull edge it sees.
    fn extend_hull(&mut self, i: usize) -> usize {
        let q = self.p[i];
        let visible: Vec<(usize, usize)> = (0..self.tri.len())
            .flat_map(|t| (0..3).map(move |k| (t, k)))
            .filter(|&(t, k)| {
                self.nbr[t][k].is_none() && {
                    let c = self.tri[t];
                    orient(self.p[c[(k + 1) % 3]], self.p[c[(k + 2) % 3]], q) < 0
                }
            })
            .collect();
        debug_assert!(!visible.is_empty(), "a point outside the hull sees a hull edge");
        let mut open_a: std::collections::HashMap<usize, (usize, usize)> = std::collections::HashMap::new();
        let mut open_b: std::collections::HashMap<usize, (usize, usize)> = std::collections::HashMap::new();
        let mut made = Vec::new();
        for &(t, k) in &visible {
            let c = self.tri[t];
            let (a, b) = (c[(k + 1) % 3], c[(k + 2) % 3]);
            let n = self.tri.len();
            // (b, a, i): opposite b is (a, i), opposite a is (i, b), opposite i is (b, a) = t.
            self.tri.push([b, a, i]);
            self.nbr.push([None, None, Some(t)]);
            self.nbr[t][k] = Some(n);
            open_a.insert(a, (n, 0));
            open_b.insert(b, (n, 1));
            made.push(n);
        }
        // Edge (a, i) of one new triangle is edge (i, b) of the next where b = a.
        for (v, &(n, s)) in &open_a {
            if let Some(&(m, r)) = open_b.get(v) {
                self.nbr[n][s] = Some(m);
                self.nbr[m][r] = Some(n);
            }
        }
        for &n in &made {
            self.legalize(n, 2);
        }
        made[0]
    }
}

#[cfg(test)]
impl Mesh {
    fn check(&self, after: usize) {
        for t in 0..self.tri.len() {
            for k in 0..3 {
                if let Some(u) = self.nbr[t][k] {
                    let c = self.tri[t];
                    let (a, b) = (c[(k + 1) % 3], c[(k + 2) % 3]);
                    let m = (0..3).find(|&m| self.nbr[u][m] == Some(t));
                    assert!(m.is_some(), "after {after}: {u} does not point back to {t}");
                    let m = m.unwrap();
                    let d = self.tri[u];
                    assert_eq!(
                        (d[(m + 1) % 3], d[(m + 2) % 3]),
                        (b, a),
                        "after {after}: t{t}={c:?} k{k} u{u}={d:?} m{m}"
                    );
                }
            }
        }
    }
}

/// `tri` turned so corner `k` comes first (order kept).
fn rotate(tri: [usize; 3], k: usize) -> [usize; 3] {
    [tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]]
}

/// The Delaunay triangulation of `points`: counter-clockwise triangles of
/// location indices covering their convex hull.
///
/// # Errors
/// [`StatError::InvalidArgument`] if every location lies on one line.
pub fn delaunay(points: &ScatteredPoints) -> Result<Vec<[usize; 3]>> {
    let n = points.len();
    let p: Vec<[i64; 2]> = (0..n).map(|i| points.snapped(i)).collect();
    // A first triangle: two points and the first not on their line.
    let third = (2..n)
        .find(|&k| orient(p[0], p[1], p[k]) != 0)
        .ok_or_else(|| invalid("all the locations lie on one line, so they cannot be triangulated"))?;
    let first = if orient(p[0], p[1], p[third]) > 0 { [0, 1, third] } else { [0, third, 1] };
    let mut mesh = Mesh { p, tri: vec![first], nbr: vec![[None; 3]] };
    // Insert the rest in a fixed pseudo-random order (expected O(n log n) flips).
    let mut rest: Vec<usize> = (2..n).filter(|&k| k != third).collect();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for k in (1..rest.len()).rev() {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let j = ((state >> 33) % (k as u64 + 1)) as usize;
        rest.swap(k, j);
    }
    let mut last = 0;
    for i in rest {
        last = mesh.insert(i, last.min(mesh.tri.len() - 1));
        #[cfg(test)]
        mesh.check(i);
    }
    Ok(mesh.tri)
}

/// The piecewise-linear surface over the Delaunay triangulation of the data.
///
/// Point location for [`value`](Self::value) uses a uniform grid of
/// buckets over the unit square (about one triangle per bucket), each
/// listing, in ascending order, the triangles whose bounding box (padded by
/// 10⁻⁹) meets it. A query tests only its bucket's triangles, in index
/// order, so it finds the same (first) containing triangle as testing every
/// triangle would, in O(1) expected time instead of O(triangles).
#[derive(Debug, Clone, PartialEq)]
pub struct LinearSurface {
    /// The (merged) observations.
    pub points: ScatteredPoints,
    /// Counter-clockwise triangles of location indices.
    pub triangles: Vec<[usize; 3]>,
    /// Triangles by bucket, for point location (built from `triangles`).
    buckets: TriangleBuckets,
}

/// Padding of a triangle's bounding box in point location: a point accepted
/// by [`LinearSurface::on_triangle`]'s barycentric tolerance (10⁻¹⁰ in each
/// weight) lies within `2·10⁻¹⁰·√2` of the triangle in the unit square.
const LOCATE_PAD: f64 = 1e-9;

/// A uniform `m × m` grid of buckets over the unit square, each listing the
/// triangles whose padded bounding box meets it, in ascending order.
#[derive(Debug, Clone, PartialEq)]
struct TriangleBuckets {
    m: usize,
    cells: Vec<Vec<usize>>,
}

impl TriangleBuckets {
    /// The bucket (row or column) of a unit coordinate, clamped to the grid.
    fn cell(&self, v: f64) -> usize {
        // Monotone in `v`, so a bounding box [lo, hi] holding v covers cell(v).
        ((v * self.m as f64).floor().max(0.0) as usize).min(self.m - 1)
    }

    fn new(corners: &[[[f64; 2]; 3]]) -> Self {
        let m = ((corners.len() as f64).sqrt().ceil() as usize).clamp(1, 1024);
        let mut b = Self { m, cells: vec![Vec::new(); m * m] };
        for (t, c) in corners.iter().enumerate() {
            let lo = |k: usize| c[0][k].min(c[1][k]).min(c[2][k]) - LOCATE_PAD;
            let hi = |k: usize| c[0][k].max(c[1][k]).max(c[2][k]) + LOCATE_PAD;
            let (i0, i1) = (b.cell(lo(0)), b.cell(hi(0)));
            let (j0, j1) = (b.cell(lo(1)), b.cell(hi(1)));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    b.cells[j * m + i].push(t);
                }
            }
        }
        b
    }

    /// Triangles that may hold the unit point `q`, ascending; none for a
    /// point outside the padded unit square (no triangle reaches it).
    fn candidates(&self, q: [f64; 2]) -> &[usize] {
        let inside = |v: f64| (-LOCATE_PAD..=1.0 + LOCATE_PAD).contains(&v);
        if !(inside(q[0]) && inside(q[1])) {
            return &[];
        }
        &self.cells[self.cell(q[1]) * self.m + self.cell(q[0])]
    }
}

impl LinearSurface {
    /// Triangulate `points`.
    ///
    /// # Errors
    /// As [`delaunay`].
    pub fn new(points: ScatteredPoints) -> Result<Self> {
        let triangles = delaunay(&points)?;
        let corners: Vec<[[f64; 2]; 3]> =
            triangles.iter().map(|t| t.map(|i| points.unit(points.x[i], points.y[i]))).collect();
        let buckets = TriangleBuckets::new(&corners);
        Ok(Self { points, triangles, buckets })
    }

    fn corner(&self, i: usize) -> [f64; 2] {
        self.points.unit(self.points.x[i], self.points.y[i])
    }

    /// Value at `(x, y)` on the triangle `t`, if the point lies in it.
    fn on_triangle(&self, t: usize, q: [f64; 2]) -> Option<f64> {
        let [a, b, c] = self.triangles[t];
        let (pa, pb, pc) = (self.corner(a), self.corner(b), self.corner(c));
        let det = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pc[0] - pa[0]) * (pb[1] - pa[1]);
        if det.abs() < f64::MIN_POSITIVE {
            return None;
        }
        let wb = ((q[0] - pa[0]) * (pc[1] - pa[1]) - (pc[0] - pa[0]) * (q[1] - pa[1])) / det;
        let wc = ((pb[0] - pa[0]) * (q[1] - pa[1]) - (q[0] - pa[0]) * (pb[1] - pa[1])) / det;
        let wa = 1.0 - wb - wc;
        let tol = -1e-10;
        (wa >= tol && wb >= tol && wc >= tol).then(|| {
            let z = &self.points.z;
            wa * z[a] + wb * z[b] + wc * z[c]
        })
    }

    /// The surface at `(x, y)`: NaN outside the convex hull of the data.
    #[must_use]
    pub fn value(&self, x: f64, y: f64) -> f64 {
        let q = self.points.unit(x, y);
        self.buckets.candidates(q).iter().find_map(|&t| self.on_triangle(t, q)).unwrap_or(f64::NAN)
    }

    /// [`value`](Self::value) by testing every triangle in order (the
    /// reference the bucketed search must match).
    #[cfg(test)]
    fn value_brute_force(&self, x: f64, y: f64) -> f64 {
        let q = self.points.unit(x, y);
        (0..self.triangles.len()).find_map(|t| self.on_triangle(t, q)).unwrap_or(f64::NAN)
    }

    /// The surface at every `(xs[i], ys[j])`, as `z[j][i]` (NaN outside the hull).
    #[must_use]
    pub fn grid(&self, xs: &[f64], ys: &[f64]) -> Vec<Vec<f64>> {
        let mut z = vec![vec![f64::NAN; xs.len()]; ys.len()];
        let ux: Vec<f64> = xs.iter().map(|&x| self.points.unit(x, 0.0)[0]).collect();
        let uy: Vec<f64> = ys.iter().map(|&y| self.points.unit(0.0, y)[1]).collect();
        let pad = LOCATE_PAD;
        for t in 0..self.triangles.len() {
            let c = self.triangles[t].map(|i| self.corner(i));
            let (lo_x, hi_x) = (c[0][0].min(c[1][0]).min(c[2][0]) - pad, c[0][0].max(c[1][0]).max(c[2][0]) + pad);
            let (lo_y, hi_y) = (c[0][1].min(c[1][1]).min(c[2][1]) - pad, c[0][1].max(c[1][1]).max(c[2][1]) + pad);
            for (j, &v) in uy.iter().enumerate() {
                if v < lo_y || v > hi_y {
                    continue;
                }
                for (i, &u) in ux.iter().enumerate() {
                    if u < lo_x || u > hi_x || !z[j][i].is_nan() {
                        continue;
                    }
                    if let Some(value) = self.on_triangle(t, [u, v]) {
                        z[j][i] = value;
                    }
                }
            }
        }
        z
    }
}

// ---------------------------------------------------------------------------
// Thin-plate spline
// ---------------------------------------------------------------------------

/// How much a thin-plate spline smooths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplineSmoothing {
    /// None: the spline passes through every (merged) observation (`λ = 0`).
    Interpolate,
    /// `λ` minimising the generalized cross-validation score.
    CrossValidated,
    /// A given `λ ≥ 0` (on coordinates scaled to `[0, 1]`).
    Lambda(f64),
}

/// A fitted thin-plate spline.
#[derive(Debug, Clone, PartialEq)]
pub struct ThinPlateSpline {
    /// The (merged) observations it was fitted to.
    pub points: ScatteredPoints,
    /// Kernel coefficients `c`, one per location.
    coef: Vec<f64>,
    /// Polynomial coefficients `d₀ + d₁u + d₂v` (scaled coordinates).
    poly: [f64; 3],
    /// Smoothing parameter `λ` (0 when interpolating).
    pub lambda: f64,
    /// Effective degrees of freedom, `tr A(λ)` (from 3 for a plane to `n`
    /// when interpolating).
    pub effective_df: f64,
    /// Generalized cross-validation score `n ‖(I − A)z‖² / tr(I − A)²`
    /// (`None` when interpolating).
    pub gcv: Option<f64>,
    /// Fitted value at each location.
    pub fitted: Vec<f64>,
}

/// `E(r) = r² ln r / (8π)` from the squared distance `r²`.
fn kernel(r2: f64) -> f64 {
    if r2 > 0.0 { r2 * r2.ln() / (16.0 * std::f64::consts::PI) } else { 0.0 }
}

impl ThinPlateSpline {
    /// Fit the spline to `points` with the chosen smoothing.
    ///
    /// # Errors
    /// [`StatError::InvalidArgument`] for more than [`MAX_SPLINE_POINTS`]
    /// locations, locations on one line, or a negative `λ`.
    pub fn fit(points: ScatteredPoints, smoothing: SplineSmoothing) -> Result<Self> {
        let n = points.len();
        if n > MAX_SPLINE_POINTS {
            return Err(invalid(format!(
                "a thin-plate spline is limited to {MAX_SPLINE_POINTS} distinct locations ({n} here); use the \
                 triangulation instead"
            )));
        }
        if let SplineSmoothing::Lambda(l) = smoothing {
            if !(l.is_finite() && l >= 0.0) {
                return Err(invalid("the smoothing parameter must be zero or positive"));
            }
        }
        let u: Vec<[f64; 2]> = (0..n).map(|i| points.unit(points.x[i], points.y[i])).collect();
        let mut k = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..i {
                let v = kernel((u[i][0] - u[j][0]).powi(2) + (u[i][1] - u[j][1]).powi(2));
                k[i * n + j] = v;
                k[j * n + i] = v;
            }
        }
        // Householder QR of T = [1, u, v]: Q = H₀H₁H₂, R the leading 3 × 3.
        let mut t: Vec<[f64; 3]> = u.iter().map(|p| [1.0, p[0], p[1]]).collect();
        let mut reflectors: Vec<Vec<f64>> = Vec::new();
        for col in 0..3 {
            let norm = (col..n).map(|i| t[i][col] * t[i][col]).sum::<f64>().sqrt();
            let alpha = if t[col][col] > 0.0 { -norm } else { norm };
            let mut v = vec![0.0; n];
            for i in col..n {
                v[i] = t[i][col];
            }
            v[col] -= alpha;
            let vn = v.iter().map(|a| a * a).sum::<f64>().sqrt();
            if vn > 0.0 {
                v.iter_mut().for_each(|a| *a /= vn);
            }
            for c2 in col..3 {
                let s: f64 = (col..n).map(|i| v[i] * t[i][c2]).sum();
                for i in col..n {
                    t[i][c2] -= 2.0 * s * v[i];
                }
            }
            reflectors.push(v);
        }
        let r = [t[0], t[1], t[2]];
        let scale = r[0][0].abs().max(1.0);
        if (0..3).any(|i| r[i][i].abs() <= 1e-9 * scale) {
            return Err(invalid("all the locations lie on one line, so no surface can be fitted"));
        }
        // QᵀKQ and Qᵀz (each reflector is symmetric and its own inverse).
        let mut qz = points.z.clone();
        for v in &reflectors {
            let w: Vec<f64> = (0..n).map(|i| (0..n).map(|j| k[i * n + j] * v[j]).sum()).collect();
            let s: f64 = v.iter().zip(&w).map(|(a, b)| a * b).sum();
            for i in 0..n {
                for j in 0..n {
                    k[i * n + j] += -2.0 * v[i] * w[j] - 2.0 * w[i] * v[j] + 4.0 * s * v[i] * v[j];
                }
            }
            let s: f64 = v.iter().zip(&qz).map(|(a, b)| a * b).sum();
            qz.iter_mut().zip(v).for_each(|(a, b)| *a -= 2.0 * s * b);
        }
        let m = n - 3;
        let mut sub = vec![0.0; m * m];
        for i in 0..m {
            for j in 0..m {
                sub[i * m + j] = 0.5 * (k[(i + 3) * n + j + 3] + k[(j + 3) * n + i + 3]);
            }
        }
        let (eig, vecs) = symmetric_eigen(m, sub);
        // w = Uᵀ Q₂ᵀ z.
        let w: Vec<f64> = (0..m).map(|c| (0..m).map(|i| vecs[i * m + c] * qz[i + 3]).sum()).collect();
        let e_max = eig.iter().copied().fold(0.0_f64, f64::max).max(f64::MIN_POSITIVE);
        let gcv_at = |nl: f64| -> f64 {
            let (mut rss, mut tr) = (0.0, 0.0);
            for (e, wi) in eig.iter().zip(&w) {
                let f = nl / (e.max(0.0) + nl);
                rss += f * f * wi * wi;
                tr += f;
            }
            n as f64 * rss / (tr * tr)
        };
        let (nl, gcv) = match smoothing {
            SplineSmoothing::Interpolate => (0.0, None),
            SplineSmoothing::Lambda(l) => (n as f64 * l, (l > 0.0).then(|| gcv_at(n as f64 * l))),
            SplineSmoothing::CrossValidated => {
                if m == 0 {
                    (0.0, None)
                } else {
                    // Search log₁₀(nλ) on a grid, then refine by golden section.
                    let lo = e_max.log10() - 12.0;
                    let steps = 140;
                    let at = |s: f64| gcv_at(10f64.powf(s));
                    let grid: Vec<(f64, f64)> =
                        (0..=steps).map(|k| lo + 14.0 * k as f64 / steps as f64).map(|s| (s, at(s))).collect();
                    let best = (0..grid.len())
                        .filter(|&k| grid[k].1.is_finite())
                        .min_by(|&a, &b| grid[a].1.total_cmp(&grid[b].1));
                    let Some(best) = best else {
                        return Err(invalid("the cross-validation score could not be computed"));
                    };
                    let (mut a, mut b) = (grid[best.saturating_sub(1)].0, grid[(best + 1).min(steps)].0);
                    let phi = 0.5 * (5f64.sqrt() - 1.0);
                    let (mut c, mut d) = (b - phi * (b - a), a + phi * (b - a));
                    let (mut fc, mut fd) = (at(c), at(d));
                    for _ in 0..60 {
                        if fc < fd {
                            b = d;
                            d = c;
                            fd = fc;
                            c = b - phi * (b - a);
                            fc = at(c);
                        } else {
                            a = c;
                            c = d;
                            fc = fd;
                            d = a + phi * (b - a);
                            fd = at(d);
                        }
                    }
                    let s = 0.5 * (a + b);
                    let s = if at(s) <= grid[best].1 { s } else { grid[best].0 };
                    let nl = 10f64.powf(s);
                    (nl, Some(gcv_at(nl)))
                }
            }
        };
        // c = Q₂ U (D + nλ)⁻¹ w.
        let y2: Vec<f64> =
            (0..m).map(|i| (0..m).map(|c| vecs[i * m + c] * w[c] / (eig[c] + nl)).sum::<f64>()).collect();
        let mut coef = vec![0.0; n];
        coef[3..].copy_from_slice(&y2);
        for v in reflectors.iter().rev() {
            let s: f64 = v.iter().zip(&coef).map(|(a, b)| a * b).sum();
            coef.iter_mut().zip(v).for_each(|(a, b)| *a -= 2.0 * s * b);
        }
        // T d = z − (K + nλ I) c, solved by R d = (Qᵀ(…))[..3].
        let kern = |i: usize, j: usize| kernel((u[i][0] - u[j][0]).powi(2) + (u[i][1] - u[j][1]).powi(2));
        let mut rhs: Vec<f64> =
            (0..n).map(|i| points.z[i] - (0..n).map(|j| kern(i, j) * coef[j]).sum::<f64>() - nl * coef[i]).collect();
        for v in &reflectors {
            let s: f64 = v.iter().zip(&rhs).map(|(a, b)| a * b).sum();
            rhs.iter_mut().zip(v).for_each(|(a, b)| *a -= 2.0 * s * b);
        }
        let mut poly = [0.0; 3];
        for i in (0..3).rev() {
            let s: f64 = (i + 1..3).map(|j| r[i][j] * poly[j]).sum();
            poly[i] = (rhs[i] - s) / r[i][i];
        }
        let fitted: Vec<f64> = (0..n).map(|i| points.z[i] - nl * coef[i]).collect();
        let effective_df =
            n as f64 - eig.iter().map(|e| if nl > 0.0 { nl / (e.max(0.0) + nl) } else { 0.0 }).sum::<f64>();
        Ok(Self { points, coef, poly, lambda: nl / n as f64, effective_df, gcv, fitted })
    }

    /// The spline at `(x, y)`.
    #[must_use]
    pub fn value(&self, x: f64, y: f64) -> f64 {
        let q = self.points.unit(x, y);
        let mut f = self.poly[0] + self.poly[1] * q[0] + self.poly[2] * q[1];
        for (i, c) in self.coef.iter().enumerate() {
            let p = self.points.unit(self.points.x[i], self.points.y[i]);
            f += c * kernel((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2));
        }
        f
    }

    /// The spline at every `(xs[i], ys[j])`, as `z[j][i]`.
    #[must_use]
    pub fn grid(&self, xs: &[f64], ys: &[f64]) -> Vec<Vec<f64>> {
        ys.iter().map(|&y| xs.iter().map(|&x| self.value(x, y)).collect()).collect()
    }

    /// Residual sum of squares at the locations (observed minus fitted).
    #[must_use]
    pub fn residual_ss(&self) -> f64 {
        self.points.z.iter().zip(&self.fitted).map(|(z, f)| (z - f).powi(2)).sum()
    }
}

/// Eigenvalues and eigenvectors of the symmetric `n × n` matrix `a` (row
/// major): Householder reduction to tridiagonal form and the implicit QL
/// method (Martin, Reinsch & Wilkinson 1968; the EISPACK `tred2`/`tql2`
/// routines). Returns the eigenvalues and the eigenvectors as the columns of
/// an `n × n` row-major matrix.
fn symmetric_eigen(n: usize, a: Vec<f64>) -> (Vec<f64>, Vec<f64>) {
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    let mut v = a;
    let at = |i: usize, j: usize| i * n + j;
    let mut d = vec![0.0; n];
    let mut e = vec![0.0; n];
    // tred2.
    for j in 0..n {
        d[j] = v[at(n - 1, j)];
    }
    for i in (1..n).rev() {
        let mut scale = 0.0;
        let mut h = 0.0;
        for dk in &d[..i] {
            scale += dk.abs();
        }
        if scale == 0.0 {
            e[i] = d[i - 1];
            for j in 0..i {
                d[j] = v[at(i - 1, j)];
                v[at(i, j)] = 0.0;
                v[at(j, i)] = 0.0;
            }
        } else {
            for dk in &mut d[..i] {
                *dk /= scale;
                h += *dk * *dk;
            }
            let mut f = d[i - 1];
            let mut g = h.sqrt();
            if f > 0.0 {
                g = -g;
            }
            e[i] = scale * g;
            h -= f * g;
            d[i - 1] = f - g;
            for ej in &mut e[..i] {
                *ej = 0.0;
            }
            for j in 0..i {
                f = d[j];
                v[at(j, i)] = f;
                g = e[j] + v[at(j, j)] * f;
                for k in j + 1..i {
                    g += v[at(k, j)] * d[k];
                    e[k] += v[at(k, j)] * f;
                }
                e[j] = g;
            }
            f = 0.0;
            for j in 0..i {
                e[j] /= h;
                f += e[j] * d[j];
            }
            let hh = f / (h + h);
            for j in 0..i {
                e[j] -= hh * d[j];
            }
            for j in 0..i {
                f = d[j];
                g = e[j];
                for k in j..i {
                    v[at(k, j)] -= f * e[k] + g * d[k];
                }
                d[j] = v[at(i - 1, j)];
                v[at(i, j)] = 0.0;
            }
        }
        d[i] = h;
    }
    for i in 0..n - 1 {
        v[at(n - 1, i)] = v[at(i, i)];
        v[at(i, i)] = 1.0;
        let h = d[i + 1];
        if h != 0.0 {
            for k in 0..=i {
                d[k] = v[at(k, i + 1)] / h;
            }
            for j in 0..=i {
                let mut g = 0.0;
                for k in 0..=i {
                    g += v[at(k, i + 1)] * v[at(k, j)];
                }
                for k in 0..=i {
                    v[at(k, j)] -= g * d[k];
                }
            }
        }
        for k in 0..=i {
            v[at(k, i + 1)] = 0.0;
        }
    }
    for j in 0..n {
        d[j] = v[at(n - 1, j)];
        v[at(n - 1, j)] = 0.0;
    }
    v[at(n - 1, n - 1)] = 1.0;
    e[0] = 0.0;
    // tql2.
    for i in 1..n {
        e[i - 1] = e[i];
    }
    e[n - 1] = 0.0;
    let mut f = 0.0;
    let mut tst1: f64 = 0.0;
    let eps = f64::EPSILON;
    for l in 0..n {
        tst1 = tst1.max(d[l].abs() + e[l].abs());
        let mut m = l;
        while m < n - 1 && e[m].abs() > eps * tst1 {
            m += 1;
        }
        if m > l {
            for _ in 0..60 {
                let mut g = d[l];
                let mut p = (d[l + 1] - g) / (2.0 * e[l]);
                let mut r = p.hypot(1.0);
                if p < 0.0 {
                    r = -r;
                }
                d[l] = e[l] / (p + r);
                d[l + 1] = e[l] * (p + r);
                let dl1 = d[l + 1];
                let mut h = g - d[l];
                for di in &mut d[l + 2..n] {
                    *di -= h;
                }
                f += h;
                p = d[m];
                let mut c = 1.0;
                let mut c2 = c;
                let mut c3 = c;
                let el1 = e[l + 1];
                let mut s = 0.0;
                let mut s2 = 0.0;
                for i in (l..m).rev() {
                    c3 = c2;
                    c2 = c;
                    s2 = s;
                    g = c * e[i];
                    h = c * p;
                    r = p.hypot(e[i]);
                    e[i + 1] = s * r;
                    s = e[i] / r;
                    c = p / r;
                    p = c * d[i] - s * g;
                    d[i + 1] = h + s * (c * g + s * d[i]);
                    for k in 0..n {
                        h = v[at(k, i + 1)];
                        v[at(k, i + 1)] = s * v[at(k, i)] + c * h;
                        v[at(k, i)] = c * v[at(k, i)] - s * h;
                    }
                }
                p = -s * s2 * c3 * el1 * e[l] / dl1;
                e[l] = s * p;
                d[l] = c * p;
                if e[l].abs() <= eps * tst1 {
                    break;
                }
            }
        }
        d[l] += f;
        e[l] = 0.0;
    }
    (d, v)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random numbers in [0, 1) (the reference script
    /// `tests/data/interpolation_reference.py` uses the same generator).
    fn uniforms(seed: u64, n: usize) -> Vec<f64> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                (s >> 11) as f64 / (1u64 << 53) as f64
            })
            .collect()
    }

    /// The reference data: 40 locations on [0, 10] × [−5, 5] with a smooth
    /// response.
    fn reference_data() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let a = uniforms(7, 40);
        let b = uniforms(11, 40);
        let x: Vec<f64> = a.iter().map(|v| 10.0 * v).collect();
        let y: Vec<f64> = b.iter().map(|v| 10.0 * v - 5.0).collect();
        let z = x.iter().zip(&y).map(|(x, y)| (0.5 * x).sin() * (0.3 * y).cos() + 0.1 * x * y / 5.0).collect();
        (x, y, z)
    }

    fn assert_close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    /// The bucketed point location gives bit-for-bit the values of testing
    /// every triangle in order: on random scattered data, on a regular grid
    /// (queries on shared edges and corners), and at queries outside the
    /// hull and outside the data range.
    #[test]
    fn bucketed_point_location_matches_brute_force() {
        let mut cases = Vec::new();
        for (seed, n) in [(3_u64, 7_usize), (5, 60), (13, 400)] {
            let a = uniforms(seed, n);
            let b = uniforms(seed + 100, n);
            let x: Vec<f64> = a.iter().map(|v| 4.0 * v - 1.0).collect();
            let y: Vec<f64> = b.iter().map(|v| 50.0 * v).collect();
            let z: Vec<f64> = x.iter().zip(&y).map(|(x, y)| x * x - 0.02 * y + (x * y).sin()).collect();
            cases.push((x, y, z));
        }
        // A 9 × 9 lattice: many queries fall on edges shared by two triangles.
        let (mut gx, mut gy) = (Vec::new(), Vec::new());
        for i in 0..9 {
            for j in 0..9 {
                gx.push(f64::from(i));
                gy.push(f64::from(j));
            }
        }
        let gz = gx.iter().zip(&gy).map(|(a, b)| a * 0.3 + b * b * 0.1).collect();
        cases.push((gx, gy, gz));
        for (x, y, z) in cases {
            let s = LinearSurface::new(ScatteredPoints::new(&x, &y, &z).unwrap()).unwrap();
            let (x0, x1) = s.points.x_range;
            let (y0, y1) = s.points.y_range;
            let q = uniforms(x.len() as u64 + 1, 4000);
            let mut queries: Vec<(f64, f64)> = q
                .chunks(2)
                .map(|c| (x0 - 0.1 * (x1 - x0) + 1.2 * (x1 - x0) * c[0], y0 - 0.1 * (y1 - y0) + 1.2 * (y1 - y0) * c[1]))
                .collect();
            // The locations themselves, edge midpoints and grid nodes.
            queries.extend(x.iter().zip(&y).map(|(&a, &b)| (a, b)));
            for t in &s.triangles {
                let (a, b) = (t[0], t[1]);
                queries.push((0.5 * (s.points.x[a] + s.points.x[b]), 0.5 * (s.points.y[a] + s.points.y[b])));
            }
            for i in 0..=40 {
                for j in 0..=40 {
                    queries.push((x0 + (x1 - x0) * f64::from(i) / 40.0, y0 + (y1 - y0) * f64::from(j) / 40.0));
                }
            }
            queries.push((f64::NAN, y0));
            let mut inside = 0;
            for (u, v) in queries {
                let (fast, slow) = (s.value(u, v), s.value_brute_force(u, v));
                assert!(
                    fast.to_bits() == slow.to_bits() || (fast.is_nan() && slow.is_nan()),
                    "({u}, {v}): {fast} vs {slow}"
                );
                inside += usize::from(fast.is_finite());
            }
            assert!(inside > 100);
        }
    }

    /// Every triangle's circumcircle is empty of other locations, and the
    /// triangles tile the convex hull (their areas add up to its area).
    fn check_delaunay(points: &ScatteredPoints, tri: &[[usize; 3]]) {
        let p: Vec<[i64; 2]> = (0..points.len()).map(|i| points.snapped(i)).collect();
        for t in tri {
            assert!(orient(p[t[0]], p[t[1]], p[t[2]]) > 0, "counter-clockwise and not degenerate");
            for (i, &q) in p.iter().enumerate() {
                if !t.contains(&i) {
                    assert!(in_circle(p[t[0]], p[t[1]], p[t[2]], q) <= 0, "empty circumcircle");
                }
            }
        }
        // Hull area by the monotone chain.
        let mut s = p.clone();
        s.sort_unstable();
        s.dedup();
        let mut hull: Vec<[i64; 2]> = Vec::new();
        for pass in 0..2 {
            let start = hull.len();
            let pts: Vec<[i64; 2]> = if pass == 0 { s.clone() } else { s.iter().rev().copied().collect() };
            for q in pts {
                while hull.len() >= start + 2 && orient(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0 {
                    hull.pop();
                }
                hull.push(q);
            }
            hull.pop();
        }
        let twice_hull: i128 = (0..hull.len())
            .map(|k| {
                let (a, b) = (hull[k], hull[(k + 1) % hull.len()]);
                i128::from(a[0]) * i128::from(b[1]) - i128::from(b[0]) * i128::from(a[1])
            })
            .sum();
        let twice_tri: i128 = tri.iter().map(|t| orient(p[t[0]], p[t[1]], p[t[2]])).sum();
        assert_eq!(twice_tri, twice_hull, "triangles tile the hull");
    }

    #[test]
    fn random_points_are_delaunay() {
        let (x, y, z) = reference_data();
        let points = ScatteredPoints::new(&x, &y, &z).unwrap();
        let tri = delaunay(&points).unwrap();
        check_delaunay(&points, &tri);
        // n points with h on the hull make 2n − 2 − h triangles; Qhull
        // (scipy.spatial.Delaunay on the scaled coordinates) gives 66.
        assert_eq!(tri.len(), 66);
    }

    #[test]
    fn grids_with_shared_circumcircles_triangulate() {
        // A 7 × 5 grid: every square's four corners share a circle.
        let mut x = Vec::new();
        let mut y = Vec::new();
        for j in 0..5 {
            for i in 0..7 {
                x.push(f64::from(i));
                y.push(f64::from(j) * 2.0);
            }
        }
        let z: Vec<f64> = x.iter().zip(&y).map(|(a, b)| 2.0 * a - b + 1.0).collect();
        let s = LinearSurface::new(ScatteredPoints::new(&x, &y, &z).unwrap()).unwrap();
        check_delaunay(&s.points, &s.triangles);
        assert_eq!(s.triangles.len(), 2 * 6 * 4);
        // A plane is reproduced exactly, including on edges and corners.
        for (u, v) in [(0.0, 0.0), (6.0, 8.0), (2.5, 3.0), (3.0, 3.0), (0.25, 7.9)] {
            assert_close(s.value(u, v), 2.0 * u - v + 1.0, 1e-12);
        }
        assert!(s.value(-0.1, 1.0).is_nan(), "outside the hull");
    }

    #[test]
    fn mixed_grid_and_scattered_points_with_collinear_runs() {
        // A 12 × 12 grid, 150 scattered points and a diagonal line of points.
        let (mut x, mut y) = (Vec::new(), Vec::new());
        for j in 0..12 {
            for i in 0..12 {
                x.push(f64::from(i));
                y.push(f64::from(j));
            }
        }
        x.extend(uniforms(21, 150).iter().map(|v| 11.0 * v));
        y.extend(uniforms(22, 150).iter().map(|v| 11.0 * v));
        for k in 0..23 {
            x.push(0.5 * f64::from(k));
            y.push(0.5 * f64::from(k));
        }
        let z = vec![0.0; x.len()];
        let points = ScatteredPoints::new(&x, &y, &z).unwrap();
        let tri = delaunay(&points).unwrap();
        check_delaunay(&points, &tri);
    }

    #[test]
    fn collinear_and_duplicate_locations() {
        let line = ScatteredPoints::new(&[0.0, 1.0, 2.0, 3.0], &[0.0, 1.0, 2.0, 3.0], &[1.0; 4]).unwrap();
        assert!(delaunay(&line).is_err());
        assert!(ThinPlateSpline::fit(line, SplineSmoothing::Interpolate).is_err());
        let p =
            ScatteredPoints::new(&[0.0, 1.0, 0.0, 1.0, 1.0], &[0.0, 0.0, 1.0, 1.0, 1.0], &[0.0, 1.0, 2.0, 3.0, 5.0])
                .unwrap();
        assert_eq!(p.len(), 4);
        assert_eq!(p.members[3], vec![3, 4]);
        assert_close(p.z[3], 4.0, 1e-15);
        assert!(ScatteredPoints::new(&[1.0, 1.0, 1.0], &[0.0, 1.0, 2.0], &[0.0; 3]).is_err());
        assert!(ScatteredPoints::new(&[0.0, 1.0, f64::NAN], &[0.0, 1.0, 2.0], &[0.0; 3]).is_err());
    }

    #[test]
    fn linear_interpolation_matches_qhull() {
        // scipy.interpolate.LinearNDInterpolator on the scaled coordinates
        // (tests/data/interpolation_reference.py).
        let (x, y, z) = reference_data();
        let s = LinearSurface::new(ScatteredPoints::new(&x, &y, &z).unwrap()).unwrap();
        for &(u, v, want) in LINEAR_REFERENCE {
            let got = s.value(u, v);
            if want.is_nan() {
                assert!(got.is_nan(), "({u}, {v}): {got}");
            } else {
                assert_close(got, want, 1e-12);
            }
        }
        let xs = [1.0, 5.0, 9.0];
        let ys = [-4.0, 0.0, 4.0];
        let g = s.grid(&xs, &ys);
        for (j, &v) in ys.iter().enumerate() {
            for (i, &u) in xs.iter().enumerate() {
                let one = s.value(u, v);
                assert!(g[j][i] == one || (g[j][i].is_nan() && one.is_nan()), "grid matches value");
            }
        }
    }

    #[test]
    fn interpolating_spline_matches_reference() {
        let (x, y, z) = reference_data();
        let points = ScatteredPoints::new(&x, &y, &z).unwrap();
        let s = ThinPlateSpline::fit(points, SplineSmoothing::Interpolate).unwrap();
        for (f, z) in s.fitted.iter().zip(&s.points.z) {
            assert_close(*f, *z, 1e-12);
        }
        for i in 0..s.points.len() {
            assert_close(s.value(s.points.x[i], s.points.y[i]), s.points.z[i], 1e-8);
        }
        // scipy.interpolate.RBFInterpolator(kernel="thin_plate_spline", degree=1).
        for &(u, v, want) in SPLINE_REFERENCE {
            assert_close(s.value(u, v), want, 1e-8);
        }
        assert_close(s.effective_df, 40.0, 1e-12);
    }

    #[test]
    fn cross_validated_spline_matches_reference() {
        let (x, y, mut z) = reference_data();
        // Add noise so smoothing is worth while.
        for (zi, e) in z.iter_mut().zip(uniforms(3, 40)) {
            *zi += 0.4 * (e - 0.5);
        }
        let points = ScatteredPoints::new(&x, &y, &z).unwrap();
        let s = ThinPlateSpline::fit(points.clone(), SplineSmoothing::CrossValidated).unwrap();
        // Dense influence-matrix GCV minimised by scipy (reference script).
        let (lambda, gcv, df, value) = GCV_REFERENCE;
        assert_close(s.lambda.log10(), lambda.log10(), 1e-4);
        assert_close(s.gcv.unwrap(), gcv, 1e-6);
        assert_close(s.effective_df, df, 1e-4);
        assert_close(s.value(5.0, 0.0), value, 1e-5);
        // A fixed λ reproduces the same fit.
        let fixed = ThinPlateSpline::fit(points, SplineSmoothing::Lambda(s.lambda)).unwrap();
        assert_close(fixed.value(5.0, 0.0), s.value(5.0, 0.0), 1e-10);
        assert_close(fixed.gcv.unwrap(), s.gcv.unwrap(), 1e-10);
    }

    #[test]
    fn heavy_smoothing_tends_to_the_least_squares_plane() {
        let (x, y, _) = reference_data();
        let z: Vec<f64> = x.iter().zip(&y).map(|(a, b)| 3.0 + 0.5 * a - 2.0 * b + (a * b).sin()).collect();
        let s = ThinPlateSpline::fit(ScatteredPoints::new(&x, &y, &z).unwrap(), SplineSmoothing::Lambda(1e9)).unwrap();
        assert_close(s.effective_df, 3.0, 1e-6);
        // Least-squares plane through the data.
        let n = x.len() as f64;
        let mean = |v: &[f64]| v.iter().sum::<f64>() / n;
        let (mx, my, mz) = (mean(&x), mean(&y), mean(&z));
        let c = |a: &[f64], ma: f64, b: &[f64], mb: f64| a.iter().zip(b).map(|(p, q)| (p - ma) * (q - mb)).sum::<f64>();
        let (sxx, sxy, syy) = (c(&x, mx, &x, mx), c(&x, mx, &y, my), c(&y, my, &y, my));
        let (sxz, syz) = (c(&x, mx, &z, mz), c(&y, my, &z, mz));
        let det = sxx * syy - sxy * sxy;
        let bx = (sxz * syy - syz * sxy) / det;
        let by = (syz * sxx - sxz * sxy) / det;
        let plane = |u: f64, v: f64| mz + bx * (u - mx) + by * (v - my);
        assert_close(s.value(2.0, 1.0), plane(2.0, 1.0), 1e-6);
    }

    #[test]
    fn eigen_decomposition_reconstructs() {
        let n = 6;
        let r = uniforms(5, n * n);
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                a[i * n + j] = r[i * n + j] + r[j * n + i];
            }
        }
        let (d, v) = symmetric_eigen(n, a.clone());
        for i in 0..n {
            for j in 0..n {
                let back: f64 = (0..n).map(|k| v[i * n + k] * d[k] * v[j * n + k]).sum();
                assert_close(back, a[i * n + j], 1e-12);
            }
        }
    }

    /// `(x, y, value)` from `LinearNDInterpolator` (NaN outside the hull).
    const LINEAR_REFERENCE: &[(f64, f64, f64)] = &[
        (1.0, -4.0, f64::NAN),
        (5.0, 0.0, 0.488_371_784_491_793_5),
        (9.0, 4.0, f64::NAN),
        (2.5, 2.5, 0.713_745_657_309_970_6),
        (7.3, -1.2, -0.597_352_899_500_020_6),
        (0.05, 4.9, f64::NAN),
        (4.0, -2.0, 0.588_506_918_716_069_3),
    ];
    /// `(x, y, value)` from `RBFInterpolator`.
    const SPLINE_REFERENCE: &[(f64, f64, f64)] = &[
        (1.0, -4.0, 0.093_557_370_394_204_3),
        (5.0, 0.0, 0.576_995_950_509_990_3),
        (9.0, 4.0, -0.008_035_482_381_038_246),
        (2.5, 2.5, 0.783_669_929_793_684_9),
        (7.3, -1.2, -0.632_916_713_610_237_7),
        (0.05, 4.9, -0.018_948_802_151_634_45),
        (4.0, -2.0, 0.590_597_818_764_546_1),
    ];
    /// `(λ, GCV, effective df, value at (5, 0))`.
    const GCV_REFERENCE: (f64, f64, f64, f64) =
        (1.707_999_720_431_098_2e-5, 0.013_247_424_765_568_535, 22.796_322_837_219_09, 0.597_540_990_106_198_2);
}
