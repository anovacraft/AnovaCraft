//! Response surface designs for fitting second-order (quadratic) models.
//!
//! # Central composite designs
//! A two-level factorial portion (full, or a resolution V fraction so that all
//! two-factor interactions remain estimable), `2k` axial points at distance `α`
//! on each axis, and centre points (Box & Wilson 1951, *JRSS B* 13; Montgomery,
//! *Design and Analysis of Experiments*, §11.4). With `F` factorial runs:
//!
//! * **rotatable** `α = F^{1/4}`, which makes the prediction variance depend only
//!   on the distance from the centre (`Σxᵢ⁴ = 3Σxᵢ²xⱼ²`);
//! * **spherical** `α = √k`, putting every non-centre point on one sphere;
//! * **face-centred** `α = 1`, so every factor uses only three levels;
//! * or any `α > 0`.
//!
//! The usual (circumscribed) design has its axial points outside the factorial
//! cube; an **inscribed** design divides every coordinate by `α` so the axial
//! points sit at the factor limits ±1.
//!
//! Default numbers of centre points: for rotatable designs, the
//! uniform-precision number of Box & Hunter (1957), *Ann. Math. Statist.* 28,
//! computed for the factorial portion used (see [`uniform_precision_centers`];
//! it reproduces their table: k = 2…7: 5, 6, 7, 6, 9, 14 with half fractions
//! for k ≥ 5; 10, 15 and 21 for full 2⁵, 2⁶ and 2⁷ portions; 20 for 2⁸⁻¹),
//! otherwise 3 for face-centred designs and 6 for the rest.
//!
//! # Box–Behnken designs
//! Three-level designs that combine two-level factorials on subsets of factors
//! with the remaining factors at their centre, never using the corners of the
//! cube (Box & Behnken 1960, *Technometrics* 2). For `k = 3, 4, 5` every pair of
//! factors gets a `2²` factorial; for `k = 6` and `k = 7` the published
//! arrangements of factor triples with `2³` factorials are used.

use crate::design::{Design, Factor, PointType, default_factor_names};
use crate::factorial::{fractional_factorial, resolution_label, smallest_fraction};
use stat_core::text::superscript;
use stat_core::{Result, StatError};

/// How far the axial points of a central composite design are from the centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AxialDistance {
    /// `α = F^{1/4}` (rotatable).
    Rotatable,
    /// `α = √k`.
    Spherical,
    /// `α = 1`.
    FaceCentered,
    /// A given `α > 0`.
    Custom(f64),
}

/// Options for [`central_composite`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CcdOptions {
    /// Axial distance.
    pub alpha: AxialDistance,
    /// Scale the design so the axial points are at ±1 (inscribed design).
    pub inscribed: bool,
    /// Number of centre points (`None` for the default).
    pub center_points: Option<usize>,
    /// Fraction exponent `p` of the factorial portion (`None`: the smallest
    /// tabulated resolution V fraction, or the full factorial).
    pub fraction: Option<usize>,
}

impl Default for CcdOptions {
    fn default() -> Self {
        Self { alpha: AxialDistance::Rotatable, inscribed: false, center_points: None, fraction: None }
    }
}

/// Central composite design for `k ≥ 2` factors.
///
/// # Errors
/// [`StatError::InvalidArgument`] for `k < 2` or `k > 11`, a non-positive or
/// non-finite custom `α`, or a fraction that is not tabulated or has resolution
/// below V (which would alias two-factor interactions).
pub fn central_composite(k: usize, options: &CcdOptions) -> Result<Design> {
    if !(2..=11).contains(&k) {
        return Err(invalid("central composite designs here support 2 to 11 factors"));
    }
    let p = options.fraction.unwrap_or_else(|| smallest_fraction(k, 5));
    let factorial = fractional_factorial(k, p)?;
    if factorial.resolution < 5 {
        return Err(invalid(format!(
            "the factorial portion must have resolution V or more (2{} has resolution {})",
            superscript(&format!("{k}−{p}")),
            resolution_label(factorial.resolution)
        )));
    }
    let f_runs = factorial.design.n_runs();
    let alpha = match options.alpha {
        AxialDistance::Rotatable => (f_runs as f64).powf(0.25),
        AxialDistance::Spherical => (k as f64).sqrt(),
        AxialDistance::FaceCentered => 1.0,
        AxialDistance::Custom(a) if a.is_finite() && a > 0.0 => a,
        AxialDistance::Custom(a) => return Err(invalid(format!("the axial distance must be positive (got {a})"))),
    };
    let centers = options.center_points.unwrap_or(match options.alpha {
        // Box & Hunter's values depend on the factorial portion's size as
        // well as k: a full 2⁵ or 2⁶ needs more centre runs than a half
        // fraction for uniform precision. Computed for any portion; the
        // results reproduce the published table.
        AxialDistance::Rotatable => uniform_precision_centers(k, f_runs),
        AxialDistance::FaceCentered => 3,
        _ => 6,
    });
    let mut runs = factorial.design.runs.clone();
    let mut types = vec![PointType::Factorial; f_runs];
    for j in 0..k {
        for sign in [-1.0, 1.0] {
            let mut r = vec![0.0; k];
            r[j] = sign * alpha;
            runs.push(r);
            types.push(PointType::Axial);
        }
    }
    let scale = if options.inscribed { 1.0 / alpha } else { 1.0 };
    for r in &mut runs {
        r.iter_mut().for_each(|v| *v *= scale);
    }
    let factors = default_factor_names(k).into_iter().map(Factor::coded).collect();
    let kind = match (options.alpha, options.inscribed) {
        (AxialDistance::FaceCentered, _) => "face-centered",
        (_, true) => "inscribed",
        _ => "circumscribed",
    };
    let mut d = Design::new(String::new(), factors, runs, types)?;
    d.add_center_points(centers);
    let n = d.n_runs();
    d.description = format!("Central composite ({kind}), {k} factors ({n} runs)");
    let portion = if p == 0 {
        format!("full 2{} factorial", superscript(&k.to_string()))
    } else {
        format!("2{} fraction, resolution {}", superscript(&format!("{k}−{p}")), resolution_label(factorial.resolution))
    };
    d.notes.push(format!(
        "Factorial portion: {portion} ({f_runs} runs); {} axial runs at α = {alpha:.4}{}; {centers} center runs.",
        2 * k,
        if options.inscribed { " (scaled so axial points are at ±1)" } else { "" }
    ));
    if p > 0 {
        d.notes.extend(factorial.design.notes.iter().filter(|n| n.starts_with("Generators")).cloned());
    }
    Ok(d)
}

/// Number of centre runs that gives a rotatable central composite design
/// (factorial portion of `f_runs` runs, resolution V or more, `2k` axial runs
/// at `α = F^{1/4}`) uniform precision: the scaled prediction variance
/// `N f(x)′(X′X)⁻¹f(x)` of the full quadratic model as nearly equal at the
/// centre and at distance 1 as a whole number `n₀ ≥ 1` of centre runs allows
/// (Box & Hunter 1957, *Ann. Math. Statist.* 28; ties go to fewer runs).
///
/// With the coordinates scaled so that the second moment `λ₂ = Σxᵢ²/N` is 1,
/// a rotatable design's moment matrix depends only on
/// `λ₄ = (Σxᵢ²xⱼ²/N)/λ₂² = F·N/(F + 2α²)²`: linear terms have moment 1,
/// interactions `λ₄`, and the intercept and pure quadratics form the block
/// `M = [[1, 1′], [1, λ₄(2I + J)]]`. So the variance is `(M⁻¹)₀₀` at the
/// centre and `(M⁻¹)₀₀ + 2(M⁻¹)₀₁ + (M⁻¹)₁₁ + 1` on an axis at distance 1.
#[must_use]
pub fn uniform_precision_centers(k: usize, f_runs: usize) -> usize {
    let f = f_runs as f64;
    let alpha2 = f.sqrt();
    let size = k + 1;
    let mut best = (f64::INFINITY, 1);
    for n0 in 1..=f_runs.max(100) {
        let n = (f_runs + 2 * k + n0) as f64;
        let lambda2 = (f + 2.0 * alpha2) / n;
        let lambda4 = (f / n) / (lambda2 * lambda2);
        let m: Vec<Vec<f64>> = (0..size)
            .map(|i| {
                (0..size)
                    .map(|j| match (i, j) {
                        (0, _) | (_, 0) => 1.0,
                        _ if i == j => 3.0 * lambda4,
                        _ => lambda4,
                    })
                    .collect()
            })
            .collect();
        let (Some(c0), Some(c1)) = (inverse_column(&m, 0), inverse_column(&m, 1)) else { continue };
        let gap = (2.0 * c0[1] + c1[1] + 1.0).abs();
        if gap < best.0 {
            best = (gap, n0);
        }
    }
    best.1
}

/// Column `col` of the inverse of a small matrix (Gauss–Jordan elimination
/// with partial pivoting), or `None` when it is singular.
fn inverse_column(m: &[Vec<f64>], col: usize) -> Option<Vec<f64>> {
    let n = m.len();
    let mut a: Vec<Vec<f64>> = m
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut row = r.clone();
            row.push(if i == col { 1.0 } else { 0.0 });
            row
        })
        .collect();
    for c in 0..n {
        let p = (c..n).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs()))?;
        if a[p][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, p);
        for r in 0..n {
            if r != c {
                let factor = a[r][c] / a[c][c];
                for j in c..=n {
                    a[r][j] -= factor * a[c][j];
                }
            }
        }
    }
    Some((0..n).map(|i| a[i][n] / a[i][i]).collect())
}

/// Factor subsets (1-based) for the Box–Behnken designs with `2³` blocks.
const BBD6: [[usize; 3]; 6] = [[1, 2, 4], [2, 3, 5], [3, 4, 6], [1, 4, 5], [2, 5, 6], [1, 3, 6]];
const BBD7: [[usize; 3]; 7] = [[4, 5, 6], [1, 6, 7], [2, 5, 7], [1, 2, 4], [3, 4, 7], [1, 3, 5], [2, 3, 6]];

/// Box–Behnken design for `3 ≤ k ≤ 7` factors with `center_points` centre runs
/// (`None`: 3 for `k ≤ 4`, 6 otherwise).
///
/// # Errors
/// [`StatError::InvalidArgument`] for `k` outside `3..=7`.
pub fn box_behnken(k: usize, center_points: Option<usize>) -> Result<Design> {
    let blocks: Vec<Vec<usize>> = match k {
        3..=5 => (0..k).flat_map(|a| (a + 1..k).map(move |b| vec![a, b])).collect(),
        6 => BBD6.iter().map(|b| b.iter().map(|i| i - 1).collect()).collect(),
        7 => BBD7.iter().map(|b| b.iter().map(|i| i - 1).collect()).collect(),
        _ => return Err(invalid("Box–Behnken designs here support 3 to 7 factors")),
    };
    let mut runs = Vec::new();
    for block in &blocks {
        let m = block.len();
        for code in 0..(1usize << m) {
            let mut r = vec![0.0; k];
            for (bit, &factor) in block.iter().enumerate() {
                r[factor] = if code & (1 << bit) == 0 { -1.0 } else { 1.0 };
            }
            runs.push(r);
        }
    }
    let n_edge = runs.len();
    let factors = default_factor_names(k).into_iter().map(Factor::coded).collect();
    let mut d = Design::new(String::new(), factors, runs, vec![PointType::Edge; n_edge])?;
    let centers = center_points.unwrap_or(if k <= 4 { 3 } else { 6 });
    d.add_center_points(centers);
    d.description = format!("Box–Behnken, {k} factors ({} runs)", d.n_runs());
    let size = blocks[0].len();
    d.notes.push(format!(
        "{} blocks of {size} factors, each a 2{} factorial with the other factors at their center; {centers} center runs.",
        blocks.len(),
        superscript(&size.to_string())
    ));
    Ok(d)
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Σ xᵢ⁴` and `Σ xᵢ² xⱼ²` over the design, for rotatability checks.
    fn moments(d: &Design, i: usize, j: usize) -> (f64, f64) {
        let fourth = d.runs.iter().map(|r| r[i].powi(4)).sum();
        let mixed = d.runs.iter().map(|r| r[i] * r[i] * r[j] * r[j]).sum();
        (fourth, mixed)
    }

    fn assert_first_and_odd_moments_vanish(d: &Design) {
        let k = d.n_factors();
        for a in 0..k {
            assert!(d.column(a).iter().sum::<f64>().abs() < 1e-12);
            for b in a + 1..k {
                let ab: f64 = d.runs.iter().map(|r| r[a] * r[b]).sum();
                assert!(ab.abs() < 1e-12, "x{a}·x{b}");
                let a2b: f64 = d.runs.iter().map(|r| r[a] * r[a] * r[b]).sum();
                assert!(a2b.abs() < 1e-12, "x{a}²·x{b}");
            }
        }
    }

    #[test]
    fn rotatable_centre_points_follow_box_hunter_by_portion_size() {
        // Box & Hunter (1957) uniform-precision n₀: half fractions 6 (k = 5),
        // 9 (k = 6); full factorials 10 (2⁵) and 15 (2⁶).
        let centres = |k: usize, fraction: Option<usize>| {
            let d = central_composite(k, &CcdOptions { fraction, ..CcdOptions::default() }).unwrap();
            d.point_types.iter().filter(|t| **t == PointType::Center).count()
        };
        assert_eq!(centres(5, None), 6);
        assert_eq!(centres(5, Some(0)), 10);
        assert_eq!(centres(6, None), 9);
        assert_eq!(centres(6, Some(0)), 15);
        assert_eq!(centres(4, Some(0)), 7);
        // k = 7: 14 with the half fraction, 21 with the full 2⁷; k = 8: 20
        // with the half fraction 2⁸⁻¹ (Box & Hunter's table, reproduced in
        // `tests/data/ccd_center_points_reference.py`).
        assert_eq!(centres(7, None), 14);
        assert_eq!(centres(7, Some(0)), 21);
        assert_eq!(centres(8, Some(1)), 20);
    }

    #[test]
    fn rotatable_centre_points_cover_every_portion_size() {
        // n₀ from `tests/data/ccd_center_points_reference.py`, which finds
        // it from the full prediction variance of the actual designs:
        // (k, p, n₀) for every resolution V portion up to k = 11.
        let expected = [
            (2, 0, 5),
            (3, 0, 6),
            (4, 0, 7),
            (5, 0, 10),
            (5, 1, 6),
            (6, 0, 15),
            (6, 1, 9),
            (7, 0, 21),
            (7, 1, 14),
            (8, 0, 28),
            (8, 1, 20),
            (8, 2, 13),
            (9, 0, 37),
            (9, 1, 29),
            (9, 2, 20),
            (10, 0, 43),
            (10, 1, 38),
            (10, 3, 19),
            (11, 0, 41),
            (11, 1, 47),
            (11, 4, 18),
        ];
        for (k, p, n0) in expected {
            assert_eq!(uniform_precision_centers(k, 1 << (k - p)), n0, "k = {k}, p = {p}");
            let d = central_composite(k, &CcdOptions { fraction: Some(p), ..CcdOptions::default() }).unwrap();
            assert_eq!(d.point_types.iter().filter(|t| **t == PointType::Center).count(), n0, "k = {k}, p = {p}");
        }
    }

    #[test]
    fn rotatable_ccd() {
        for k in 2..=8 {
            let d = central_composite(k, &CcdOptions::default()).unwrap();
            let (fourth, mixed) = moments(&d, 0, 1);
            assert!((fourth - 3.0 * mixed).abs() < 1e-9, "k = {k}: Σx⁴ {fourth} vs 3Σx²y² {mixed}");
            assert_first_and_odd_moments_vanish(&d);
        }
        let d = central_composite(2, &CcdOptions::default()).unwrap();
        assert_eq!(d.n_runs(), 4 + 4 + 5);
        assert!((d.runs[4][0] + 2f64.sqrt()).abs() < 1e-12, "α = √2 for k = 2");
        let d = central_composite(5, &CcdOptions::default()).unwrap();
        assert_eq!(d.n_runs(), 16 + 10 + 6, "half fraction for k = 5");
        assert!(d.notes[0].contains("resolution V"));
        let d = central_composite(5, &CcdOptions { fraction: Some(0), ..CcdOptions::default() }).unwrap();
        assert_eq!(d.n_runs(), 32 + 10 + 10);
        assert!(central_composite(5, &CcdOptions { fraction: Some(2), ..CcdOptions::default() }).is_err(), "res III");
    }

    #[test]
    fn ccd_variants() {
        let face =
            central_composite(3, &CcdOptions { alpha: AxialDistance::FaceCentered, ..CcdOptions::default() }).unwrap();
        assert!(face.runs.iter().flatten().all(|v| [-1.0, 0.0, 1.0].contains(v)));
        assert_eq!(face.n_runs(), 8 + 6 + 3);
        assert!(face.description.contains("face-centered"));
        let ins = central_composite(3, &CcdOptions { inscribed: true, ..CcdOptions::default() }).unwrap();
        let max = ins.runs.iter().flatten().fold(0f64, |m, v| m.max(v.abs()));
        assert!((max - 1.0).abs() < 1e-12);
        assert!((ins.runs[0][0].abs() - 1.0 / 8f64.powf(0.25)).abs() < 1e-12);
        let sph = central_composite(
            4,
            &CcdOptions { alpha: AxialDistance::Spherical, center_points: Some(2), ..CcdOptions::default() },
        )
        .unwrap();
        for (r, t) in sph.runs.iter().zip(&sph.point_types) {
            let radius: f64 = r.iter().map(|v| v * v).sum::<f64>().sqrt();
            if *t != PointType::Center {
                assert!((radius - 2.0).abs() < 1e-12, "all on the sphere of radius √k");
            }
        }
        assert_eq!(sph.point_types.iter().filter(|t| **t == PointType::Center).count(), 2);
        assert!(
            central_composite(3, &CcdOptions { alpha: AxialDistance::Custom(-1.0), ..CcdOptions::default() }).is_err()
        );
        assert!(central_composite(1, &CcdOptions::default()).is_err());
    }

    #[test]
    fn box_behnken_designs() {
        let sizes = [(3, 12 + 3), (4, 24 + 3), (5, 40 + 6), (6, 48 + 6), (7, 56 + 6)];
        for (k, n) in sizes {
            let d = box_behnken(k, None).unwrap();
            assert_eq!(d.n_runs(), n, "k = {k}");
            assert!(d.runs.iter().all(|r| r.contains(&0.0)), "no corner points");
            assert_first_and_odd_moments_vanish(&d);
            // Every factor is used equally often at ±1.
            let counts: Vec<usize> = (0..k).map(|j| d.column(j).iter().filter(|v| **v != 0.0).count()).collect();
            assert!(counts.iter().all(|&c| c == counts[0]), "k = {k}: {counts:?}");
        }
        // k = 4 and k = 7 are rotatable; k = 3 is not.
        for k in [4, 7] {
            let d = box_behnken(k, None).unwrap();
            let (fourth, mixed) = moments(&d, 0, 1);
            assert!((fourth - 3.0 * mixed).abs() < 1e-12, "k = {k}");
        }
        let (fourth, mixed) = moments(&box_behnken(3, None).unwrap(), 0, 1);
        assert_ne!(fourth, 3.0 * mixed);
        // The 7-factor design pairs every two factors exactly once.
        let d = box_behnken(7, Some(0)).unwrap();
        for a in 0..7 {
            for b in a + 1..7 {
                let together = d.runs.iter().filter(|r| r[a] != 0.0 && r[b] != 0.0).count();
                assert_eq!(together, 8, "factors {a}, {b} share one 2³ block");
            }
        }
        assert!(box_behnken(2, None).is_err());
        assert!(box_behnken(8, None).is_err());
    }
}
