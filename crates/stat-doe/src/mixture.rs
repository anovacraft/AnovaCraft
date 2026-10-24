//! Mixture experiments: designs on the simplex `x₁ + … + x_q = 1, xᵢ ≥ 0` and
//! the Scheffé canonical polynomials fitted to them (Scheffé 1958, 1963;
//! Cornell, *Experiments with Mixtures*, 3rd ed., ch. 2; NIST/SEMATECH
//! e-Handbook §5.5.4).
//!
//! * **Simplex-lattice `{q, m}`** — every blend whose proportions are multiples
//!   of `1/m`: `C(q + m − 1, m)` points. It supports the Scheffé polynomial of
//!   degree `m`.
//! * **Simplex-centroid** — the `2^q − 1` blends with equal proportions of a
//!   non-empty subset of components (pure blends, 50:50 binaries, …, the overall
//!   centroid). It supports the special cubic and, in general, the full
//!   `q`-component Scheffé polynomial with all products.
//! * **Axial check blends** (optional) — `(1 + q)/(2q)` of one component and
//!   `1/(2q)` of each other, half-way between the centroid and each vertex;
//!   interior points for checking model fit.
//!
//! * **Extreme vertices** — when components have lower and upper bounds
//!   `Lᵢ ≤ xᵢ ≤ Uᵢ`, the region is a convex polytope inside the simplex. Its
//!   vertices are found by the method of McLean & Anderson (1966),
//!   "Extreme vertices design of mixture experiments", *Technometrics* 8(3),
//!   447–454: set all but one component at a bound in every combination,
//!   give the remaining component `1 − Σ others`, and keep the blend when that
//!   is within its own bounds. Bounds implied by the others
//!   (`Lᵢ' = max(Lᵢ, 1 − Σ_{j≠i} Uⱼ)`, `Uᵢ' = min(Uᵢ, 1 − Σ_{j≠i} Lⱼ)`) are
//!   applied first. Centroids of the edges and two-dimensional faces (the
//!   averages of their vertices) and the overall centroid (the average of all
//!   vertices) can be added as interior points (Cornell, ch. 4).
//!
//! Because the proportions sum to one, the Scheffé models have no intercept:
//!
//! ```text
//! linear          E(y) = Σ βᵢxᵢ
//! quadratic       E(y) = Σ βᵢxᵢ + Σ_{i<j} βᵢⱼxᵢxⱼ
//! special cubic   E(y) = … + Σ_{i<j<k} βᵢⱼₖxᵢxⱼxₖ
//! ```

use stat_core::{Result, StatError};
use stat_data::{Column, DataTable};

/// Most components accepted.
pub const MAX_COMPONENTS: usize = 12;

/// The role of a blend in a mixture design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendType {
    /// A single component.
    Pure,
    /// Equal parts of two components.
    Binary,
    /// Equal parts of three or more components, short of all of them.
    Partial,
    /// Equal parts of every component.
    Centroid,
    /// An interior check blend on an axis.
    Axial,
    /// Any other lattice point.
    Lattice,
    /// A vertex of a constrained region (extreme-vertices design).
    Vertex,
    /// The centroid of an edge of a constrained region.
    EdgeCentroid,
    /// The centroid of a two-dimensional face of a constrained region.
    FaceCentroid,
}

impl BlendType {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pure => "Pure blend",
            Self::Binary => "Binary blend",
            Self::Partial => "Partial centroid",
            Self::Centroid => "Overall centroid",
            Self::Axial => "Axial check blend",
            Self::Lattice => "Lattice point",
            Self::Vertex => "Vertex",
            Self::EdgeCentroid => "Edge centroid",
            Self::FaceCentroid => "Face centroid",
        }
    }
}

/// A mixture design: proportions of each component (each run sums to one).
#[derive(Debug, Clone, PartialEq)]
pub struct MixtureDesign {
    /// Description, e.g. `"Simplex-lattice {3, 2}"`.
    pub description: String,
    /// Component names.
    pub components: Vec<String>,
    /// Proportions, one row per run.
    pub runs: Vec<Vec<f64>>,
    /// Role of each run.
    pub blend_types: Vec<BlendType>,
}

impl MixtureDesign {
    /// Number of runs.
    #[must_use]
    pub fn n_runs(&self) -> usize {
        self.runs.len()
    }

    /// The design as a table: one column per component and an empty
    /// `response` column, ready for results.
    ///
    /// # Errors
    /// Errors from building the table (e.g. duplicate names).
    pub fn to_table(&self, name: &str, response: &str) -> Result<DataTable> {
        let mut columns: Vec<Column> = self
            .components
            .iter()
            .enumerate()
            .map(|(j, c)| Column::numeric(c.clone(), self.runs.iter().map(|r| r[j]).collect()))
            .collect();
        columns.push(Column::numeric(response, vec![f64::NAN; self.runs.len()]));
        DataTable::new(name, columns)
    }
}

/// Scheffé canonical mixture model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheffeModel {
    /// `Σ βᵢxᵢ`.
    Linear,
    /// Adds all `xᵢxⱼ`.
    Quadratic,
    /// Adds all `xᵢxⱼxₖ` to the quadratic model.
    SpecialCubic,
}

impl ScheffeModel {
    /// Display name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Quadratic => "Quadratic",
            Self::SpecialCubic => "Special cubic",
        }
    }

    /// Component index sets of the terms, in order (singletons, pairs, triples).
    #[must_use]
    pub fn terms(self, q: usize) -> Vec<Vec<usize>> {
        let max = match self {
            Self::Linear => 1,
            Self::Quadratic => 2,
            Self::SpecialCubic => 3,
        };
        let mut out: Vec<Vec<usize>> = (0..q).map(|i| vec![i]).collect();
        if max >= 2 {
            out.extend((0..q).flat_map(|i| (i + 1..q).map(move |j| vec![i, j])));
        }
        if max >= 3 {
            out.extend((0..q).flat_map(|i| (i + 1..q).flat_map(move |j| (j + 1..q).map(move |k| vec![i, j, k]))));
        }
        out
    }

    /// Number of terms for `q` components.
    #[must_use]
    pub fn n_terms(self, q: usize) -> usize {
        self.terms(q).len()
    }

    /// Model row `f(x)` at proportions `x`.
    #[must_use]
    pub fn expand(self, x: &[f64]) -> Vec<f64> {
        self.terms(x.len()).iter().map(|t| t.iter().map(|&i| x[i]).product()).collect()
    }

    /// Term labels such as `A`, `A*B`, `A*B*C`.
    #[must_use]
    pub fn labels(self, names: &[String]) -> Vec<String> {
        self.terms(names.len())
            .iter()
            .map(|t| t.iter().map(|&i| names[i].as_str()).collect::<Vec<_>>().join("*"))
            .collect()
    }
}

/// Simplex-lattice `{q, m}` design, optionally with the overall centroid and
/// axial check blends added (each only if not already present).
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `2 ≤ q ≤ 12` and `1 ≤ m ≤ 6`.
pub fn simplex_lattice(q: usize, m: usize, add_centroid: bool, add_axial: bool) -> Result<MixtureDesign> {
    check_components(q)?;
    if !(1..=6).contains(&m) {
        return Err(invalid("the lattice degree m must be between 1 and 6"));
    }
    let mut runs = Vec::new();
    compositions(q, &mut vec![0; q], 0, m, &mut runs);
    let mut runs: Vec<Vec<f64>> = runs.into_iter().map(|c| c.iter().map(|&a| a as f64 / m as f64).collect()).collect();
    // Order: pure blends first, then by how many components are present.
    runs.sort_by(|a, b| {
        present(a).cmp(&present(b)).then_with(|| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut d = finish(format!("Simplex-lattice {{{q}, {m}}}"), q, runs, BlendType::Lattice);
    augment(&mut d, add_centroid, add_axial);
    Ok(d)
}

/// Simplex-centroid design for `q` components, optionally with axial check
/// blends.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `2 ≤ q ≤ 12`.
pub fn simplex_centroid(q: usize, add_axial: bool) -> Result<MixtureDesign> {
    check_components(q)?;
    let mut runs: Vec<Vec<f64>> = (1..(1u32 << q))
        .map(|mask| {
            let size = mask.count_ones() as f64;
            (0..q).map(|i| if mask & (1 << i) != 0 { 1.0 / size } else { 0.0 }).collect()
        })
        .collect();
    runs.sort_by_key(|r| present(r));
    let mut d = finish(format!("Simplex-centroid, {q} components"), q, runs, BlendType::Partial);
    augment(&mut d, false, add_axial);
    Ok(d)
}

/// Most vertices for which two-dimensional face centroids are found.
pub const MAX_VERTICES_FOR_FACES: usize = 300;

/// Extreme-vertices design for components with bounds `lower[i] ≤ xᵢ ≤
/// upper[i]` (McLean & Anderson 1966): the vertices of the constrained
/// region, then — up to `centroid_dimension` (0 none, 1 edges, 2 edges and
/// two-dimensional faces) — the centroids of its faces, then the overall
/// centroid if `overall_centroid`.
///
/// # Errors
/// [`StatError::InvalidArgument`] unless `2 ≤ q ≤ 12` with one lower and one
/// upper bound per component, `0 ≤ Lᵢ < Uᵢ ≤ 1`, `ΣL < 1 < ΣU` (otherwise
/// no blend, or a single blend, satisfies the bounds), every component still
/// free to vary once the implied bounds are applied, `centroid_dimension ≤ 2`,
/// and at most [`MAX_VERTICES_FOR_FACES`] vertices when face centroids are
/// requested; [`StatError::NonFiniteInput`] for NaN or ±∞ bounds.
pub fn extreme_vertices(
    lower: &[f64],
    upper: &[f64],
    centroid_dimension: usize,
    overall_centroid: bool,
) -> Result<MixtureDesign> {
    const TOL: f64 = 1e-9;
    let q = lower.len();
    check_components(q)?;
    if upper.len() != q {
        return Err(invalid("give one lower and one upper bound per component"));
    }
    if lower.iter().chain(upper).any(|v| !v.is_finite()) {
        return Err(StatError::NonFiniteInput { context: "mixture component bounds" });
    }
    if lower.iter().zip(upper).any(|(&l, &u)| !(0.0..1.0).contains(&l) || u <= l || u > 1.0) {
        return Err(invalid("each component needs bounds 0 ≤ lower < upper ≤ 1"));
    }
    if centroid_dimension > 2 {
        return Err(invalid("face centroids are available for edges (1) and two-dimensional faces (2)"));
    }
    let (sum_l, sum_u) = (lower.iter().sum::<f64>(), upper.iter().sum::<f64>());
    if sum_l > 1.0 - TOL || sum_u < 1.0 + TOL {
        return Err(invalid(format!(
            "the lower bounds sum to {sum_l:.6} and the upper bounds to {sum_u:.6}; they must leave room on both \
             sides of 1 for the blends to vary"
        )));
    }
    // Implied bounds.
    let lo: Vec<f64> = (0..q).map(|i| lower[i].max(1.0 - (sum_u - upper[i]))).collect();
    let hi: Vec<f64> = (0..q).map(|i| upper[i].min(1.0 - (sum_l - lower[i]))).collect();
    if let Some(i) = (0..q).find(|&i| hi[i] - lo[i] < TOL) {
        return Err(invalid(format!("the bounds fix component {} at {:.6}; leave it out", i + 1, lo[i])));
    }
    // McLean–Anderson: all but one component at a bound.
    let mut vertices: Vec<Vec<f64>> = Vec::new();
    for free in 0..q {
        for mask in 0..(1usize << (q - 1)) {
            let mut x = vec![0.0; q];
            for (bit, j) in (0..q).filter(|&j| j != free).enumerate() {
                x[j] = if mask & (1 << bit) != 0 { hi[j] } else { lo[j] };
            }
            let rest = 1.0 - x.iter().sum::<f64>();
            if rest >= lo[free] - TOL && rest <= hi[free] + TOL {
                x[free] = rest.clamp(lo[free], hi[free]);
                if !vertices.iter().any(|v| v.iter().zip(&x).all(|(a, b)| (a - b).abs() < 1e-7)) {
                    vertices.push(x);
                }
            }
        }
    }
    // Active bounds of each vertex: bit 2i lower, bit 2i + 1 upper.
    let active: Vec<u32> = vertices
        .iter()
        .map(|v| {
            (0..q).fold(0u32, |m, i| {
                m | (u32::from((v[i] - lo[i]).abs() < 1e-7) << (2 * i))
                    | (u32::from((v[i] - hi[i]).abs() < 1e-7) << (2 * i + 1))
            })
        })
        .collect();
    let mut runs = vertices.clone();
    let mut blend_types = vec![BlendType::Vertex; vertices.len()];
    for d in 1..=centroid_dimension.min(q - 2) {
        if d == 2 && vertices.len() > MAX_VERTICES_FOR_FACES {
            return Err(invalid(format!(
                "the region has {} vertices; face centroids are found for at most {MAX_VERTICES_FOR_FACES}",
                vertices.len()
            )));
        }
        for face in faces(&vertices, &active, d) {
            let size = face.len() as f64;
            runs.push((0..q).map(|j| face.iter().map(|&v| vertices[v][j]).sum::<f64>() / size).collect());
            blend_types.push(if d == 1 { BlendType::EdgeCentroid } else { BlendType::FaceCentroid });
        }
    }
    if overall_centroid {
        let n = vertices.len() as f64;
        runs.push((0..q).map(|j| vertices.iter().map(|v| v[j]).sum::<f64>() / n).collect());
        blend_types.push(BlendType::Centroid);
    }
    let description = format!("Extreme vertices, {q} components ({} vertices)", vertices.len());
    Ok(MixtureDesign { description, components: crate::design::default_factor_names(q), runs, blend_types })
}

/// The `d`-dimensional faces of the polytope with these vertices, as sorted
/// vertex index lists. A face is the set of vertices sharing the bounds that
/// `d + 1` affinely independent vertices have in common; its dimension is the
/// affine rank of that set.
fn faces(vertices: &[Vec<f64>], active: &[u32], d: usize) -> Vec<Vec<usize>> {
    let n = vertices.len();
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut consider = |common: u32| {
        if common == 0 || !seen.insert(common) {
            return;
        }
        let members: Vec<usize> = (0..n).filter(|&v| active[v] & common == common).collect();
        if affine_rank(vertices, &members) == d && !out.contains(&members) {
            out.push(members);
        }
    };
    for a in 0..n {
        for b in a + 1..n {
            let ab = active[a] & active[b];
            if d == 1 {
                consider(ab);
            } else {
                for &c in &active[b + 1..] {
                    consider(ab & c);
                }
            }
        }
    }
    out
}

/// Affine rank of the points `vertices[members]` (Gaussian elimination with
/// partial pivoting on the differences from the first point).
fn affine_rank(vertices: &[Vec<f64>], members: &[usize]) -> usize {
    let Some((&first, rest)) = members.split_first() else { return 0 };
    let mut rows: Vec<Vec<f64>> =
        rest.iter().map(|&m| vertices[m].iter().zip(&vertices[first]).map(|(a, b)| a - b).collect()).collect();
    let mut rank = 0;
    for c in 0..vertices[first].len() {
        let Some(p) = (rank..rows.len()).max_by(|&i, &j| rows[i][c].abs().total_cmp(&rows[j][c].abs())) else {
            break;
        };
        if rows[p][c].abs() < 1e-9 {
            continue;
        }
        rows.swap(rank, p);
        let pivot = rows[rank].clone();
        for r in rows.iter_mut().skip(rank + 1) {
            let f = r[c] / pivot[c];
            r.iter_mut().zip(&pivot).for_each(|(x, y)| *x -= f * y);
        }
        rank += 1;
    }
    rank
}

fn check_components(q: usize) -> Result<()> {
    if !(2..=MAX_COMPONENTS).contains(&q) {
        return Err(invalid(format!("a mixture needs 2 to {MAX_COMPONENTS} components")));
    }
    Ok(())
}

/// All `q`-part compositions of `total` into non-negative integers.
fn compositions(q: usize, current: &mut Vec<usize>, index: usize, left: usize, out: &mut Vec<Vec<usize>>) {
    if index == q - 1 {
        current[index] = left;
        out.push(current.clone());
        return;
    }
    for a in (0..=left).rev() {
        current[index] = a;
        compositions(q, current, index + 1, left - a, out);
    }
}

fn present(x: &[f64]) -> usize {
    x.iter().filter(|&&v| v > 0.0).count()
}

/// Classify runs: pure, binary 50:50, partial / overall centroids, otherwise `other`.
fn finish(description: String, q: usize, runs: Vec<Vec<f64>>, other: BlendType) -> MixtureDesign {
    let blend_types = runs.iter().map(|r| classify(r, q, other)).collect();
    MixtureDesign { description, components: crate::design::default_factor_names(q), runs, blend_types }
}

fn classify(r: &[f64], q: usize, other: BlendType) -> BlendType {
    let k = present(r);
    let equal = r.iter().filter(|&&v| v > 0.0).all(|&v| (v - 1.0 / k as f64).abs() < 1e-12);
    match (k, equal) {
        (1, _) => BlendType::Pure,
        (2, true) => BlendType::Binary,
        (k, true) if k == q => BlendType::Centroid,
        (_, true) if other == BlendType::Partial => BlendType::Partial,
        _ => BlendType::Lattice,
    }
}

fn augment(d: &mut MixtureDesign, centroid: bool, axial: bool) {
    let q = d.components.len();
    let contains =
        |runs: &[Vec<f64>], p: &[f64]| runs.iter().any(|r| r.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-12));
    if centroid {
        let c = vec![1.0 / q as f64; q];
        if !contains(&d.runs, &c) {
            d.runs.push(c);
            d.blend_types.push(BlendType::Centroid);
        }
    }
    if axial {
        for i in 0..q {
            let p: Vec<f64> = (0..q)
                .map(|j| if j == i { (1.0 + q as f64) / (2.0 * q as f64) } else { 1.0 / (2.0 * q as f64) })
                .collect();
            if !contains(&d.runs, &p) {
                d.runs.push(p);
                d.blend_types.push(BlendType::Axial);
            }
        }
        d.description.push_str(" with axial check blends");
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binomial(n: usize, k: usize) -> usize {
        (0..k).fold(1, |acc, i| acc * (n - i) / (i + 1))
    }

    #[test]
    fn lattice_sizes_and_points() {
        for q in 2..=6 {
            for m in 1..=4 {
                let d = simplex_lattice(q, m, false, false).unwrap();
                assert_eq!(d.n_runs(), binomial(q + m - 1, m), "{{{q}, {m}}}");
                for r in &d.runs {
                    assert!((r.iter().sum::<f64>() - 1.0).abs() < 1e-12);
                    assert!(r.iter().all(|v| (v * m as f64 - (v * m as f64).round()).abs() < 1e-12 && *v >= 0.0));
                }
            }
        }
        // {3, 2}: 3 pure blends then 3 binary 50:50 blends (Cornell Table 2.1).
        let d = simplex_lattice(3, 2, false, false).unwrap();
        assert_eq!(d.runs[0], vec![1.0, 0.0, 0.0]);
        assert_eq!(&d.blend_types[..3], &[BlendType::Pure; 3]);
        assert_eq!(&d.blend_types[3..], &[BlendType::Binary; 3]);
        assert!(d.runs.contains(&vec![0.5, 0.0, 0.5]));
        // {3, 3} has 10 points, among them (1/3, 2/3, 0) and the centroid.
        let d = simplex_lattice(3, 3, false, false).unwrap();
        assert_eq!(d.n_runs(), 10);
        assert_eq!(d.blend_types.iter().filter(|&&b| b == BlendType::Centroid).count(), 1);
        assert!(d.runs.iter().any(|r| (r[0] - 1.0 / 3.0).abs() < 1e-12 && (r[1] - 2.0 / 3.0).abs() < 1e-12));
        // Adding the centroid to {3, 3} does not duplicate it; to {3, 2} it adds one run.
        assert_eq!(simplex_lattice(3, 3, true, false).unwrap().n_runs(), 10);
        assert_eq!(simplex_lattice(3, 2, true, false).unwrap().n_runs(), 7);
    }

    #[test]
    fn centroid_design_and_axial_blends() {
        let d = simplex_centroid(3, false).unwrap();
        assert_eq!(d.n_runs(), 7);
        assert_eq!(d.blend_types.iter().filter(|&&b| b == BlendType::Pure).count(), 3);
        assert_eq!(d.blend_types.iter().filter(|&&b| b == BlendType::Binary).count(), 3);
        assert_eq!(*d.blend_types.last().unwrap(), BlendType::Centroid);
        assert_eq!(simplex_centroid(4, false).unwrap().n_runs(), 15);
        let a = simplex_centroid(3, true).unwrap();
        assert_eq!(a.n_runs(), 10);
        // Axial blend for q = 3: (2/3, 1/6, 1/6).
        let axial = &a.runs[7];
        assert!((axial[0] - 2.0 / 3.0).abs() < 1e-12 && (axial[1] - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!(a.blend_types[7], BlendType::Axial);
        assert!(simplex_centroid(1, false).is_err() && simplex_centroid(13, false).is_err());
        assert!(simplex_lattice(3, 0, false, false).is_err());
    }

    #[test]
    fn scheffe_models_are_estimable_on_their_designs() {
        // Term counts: q, q + C(q,2), q + C(q,2) + C(q,3).
        assert_eq!(ScheffeModel::Linear.n_terms(4), 4);
        assert_eq!(ScheffeModel::Quadratic.n_terms(4), 10);
        assert_eq!(ScheffeModel::SpecialCubic.n_terms(4), 14);
        let names: Vec<String> = ["A", "B", "C"].map(String::from).to_vec();
        assert_eq!(ScheffeModel::SpecialCubic.labels(&names), ["A", "B", "C", "A*B", "A*C", "B*C", "A*B*C"]);
        let x = ScheffeModel::Quadratic.expand(&[0.5, 0.5, 0.0]);
        assert_eq!(x, vec![0.5, 0.5, 0.0, 0.25, 0.0, 0.0]);
        // The {q, 2} lattice is saturated for the quadratic model, the simplex-
        // centroid for the special cubic when q = 3: XᵀX is non-singular.
        for (d, model) in [
            (simplex_lattice(3, 2, false, false).unwrap(), ScheffeModel::Quadratic),
            (simplex_lattice(4, 2, false, false).unwrap(), ScheffeModel::Quadratic),
            (simplex_centroid(3, false).unwrap(), ScheffeModel::SpecialCubic),
        ] {
            let rows: Vec<Vec<f64>> = d.runs.iter().map(|r| model.expand(r)).collect();
            assert_eq!(rows.len(), model.n_terms(d.components.len()));
            let m = stat_core::Matrix::from_rows(&rows).unwrap();
            let svd = stat_core::svd(&m).unwrap();
            let s = &svd.singular_values;
            assert!(s.iter().copied().fold(f64::INFINITY, f64::min) > 1e-6, "{}", d.description);
        }
    }

    /// The flare example of McLean & Anderson (1966) (also Cornell,
    /// *Experiments with Mixtures*, ch. 4): magnesium 0.40–0.60, sodium
    /// nitrate 0.10–0.50, strontium nitrate 0.10–0.50, binder 0.03–0.08. The
    /// published vertices are the 8 below; the region is combinatorially a
    /// cube (12 edges, 6 faces), and the overall centroid — the mean of the
    /// vertices, by hand — is (0.5, 0.2225, 0.2225, 0.055).
    #[test]
    fn extreme_vertices_flare_example() {
        let (lower, upper) = ([0.4, 0.1, 0.1, 0.03], [0.6, 0.5, 0.5, 0.08]);
        let d = extreme_vertices(&lower, &upper, 0, false).unwrap();
        let mut v = d.runs.clone();
        v.sort_by_key(|r| r.iter().map(|x| (x * 1e6).round() as i64).collect::<Vec<_>>());
        let published = [
            [0.4, 0.1, 0.42, 0.08],
            [0.4, 0.1, 0.47, 0.03],
            [0.4, 0.42, 0.1, 0.08],
            [0.4, 0.47, 0.1, 0.03],
            [0.6, 0.1, 0.22, 0.08],
            [0.6, 0.1, 0.27, 0.03],
            [0.6, 0.22, 0.1, 0.08],
            [0.6, 0.27, 0.1, 0.03],
        ];
        assert_eq!(v.len(), 8);
        for (a, b) in v.iter().zip(published) {
            assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-12), "{a:?} vs {b:?}");
        }
        assert!(d.blend_types.iter().all(|&t| t == BlendType::Vertex));
        let full = extreme_vertices(&lower, &upper, 2, true).unwrap();
        let count = |t: BlendType| full.blend_types.iter().filter(|&&b| b == t).count();
        assert_eq!(
            (count(BlendType::Vertex), count(BlendType::EdgeCentroid), count(BlendType::FaceCentroid)),
            (8, 12, 6)
        );
        let c = full.runs.last().unwrap();
        for (a, b) in c.iter().zip([0.5, 0.2225, 0.2225, 0.055]) {
            assert!((a - b).abs() < 1e-12);
        }
        assert_eq!(full.n_runs(), 27);
        // The face with magnesium at 0.4 has centroid (0.4, 0.2725, 0.2725, 0.055).
        assert!(full.runs.iter().zip(&full.blend_types).any(|(r, &t)| t == BlendType::FaceCentroid
            && [0.4, 0.2725, 0.2725, 0.055].iter().zip(r).all(|(a, b)| (a - b).abs() < 1e-12)));
        for r in &full.runs {
            assert!((r.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            assert!(r.iter().zip(lower.iter().zip(upper)).all(|(x, (l, u))| *x >= l - 1e-12 && *x <= u + 1e-12));
        }
    }

    #[test]
    fn extreme_vertices_of_the_whole_simplex_and_errors() {
        // Bounds that do not bind give the simplex: q pure blends, C(q, 2) edges.
        let d = extreme_vertices(&[0.0; 3], &[1.0; 3], 1, true).unwrap();
        assert_eq!(d.runs.iter().filter(|r| r.contains(&1.0)).count(), 3);
        assert_eq!(d.blend_types.iter().filter(|&&t| t == BlendType::EdgeCentroid).count(), 3);
        assert!(extreme_vertices(&[0.5, 0.6], &[1.0, 1.0], 0, false).is_err(), "lower bounds exceed 1");
        assert!(extreme_vertices(&[0.0, 0.0], &[0.4, 0.5], 0, false).is_err(), "upper bounds below 1");
        assert!(extreme_vertices(&[0.2, 0.0], &[0.1, 1.0], 0, false).is_err(), "lower above upper");
        assert!(extreme_vertices(&[0.0, 0.0, 0.0], &[1.0, 1.0], 0, false).is_err());
        assert!(extreme_vertices(&[0.0; 3], &[1.0; 3], 3, false).is_err());
    }

    #[test]
    fn table_export() {
        let d = simplex_lattice(3, 2, false, true).unwrap();
        let t = d.to_table("Mixture", "Yield").unwrap();
        assert_eq!(t.n_cols(), 4);
        assert_eq!(t.n_rows(), 9);
        assert!(d.description.ends_with("axial check blends"));
    }
}
