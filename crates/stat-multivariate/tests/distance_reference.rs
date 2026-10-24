//! Distance matrices, principal coordinates and hierarchical clustering of
//! a distance matrix, compared with SciPy 1.18 (`pdist`, `linkage`) and R
//! 4.3 (`cluster::daisy`, `cmdscale`, `ape::pcoa`, `hclust`); see
//! `tests/data/distance_reference.py` and `distance_reference.R`, which
//! generated the data and `tests/data/distance_reference.json`.

use stat_data::{JsonValue, parse_json};
use stat_multivariate::{
    DistanceColumn, DistanceMatrix, DistanceMetric, Linkage, NegativeCorrection, VariableKind, distance_matrix,
    hierarchical_from_distances, principal_coordinates,
};

fn reference() -> JsonValue {
    parse_json(include_str!("data/distance_reference.json")).unwrap()
}

fn num(v: &JsonValue) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

fn nums(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(num).collect()
}

fn rows(v: &JsonValue) -> Vec<Vec<f64>> {
    v.as_array().unwrap().iter().map(nums).collect()
}

fn columns(data: &[Vec<f64>], kinds: &[VariableKind]) -> Vec<DistanceColumn> {
    kinds
        .iter()
        .enumerate()
        .map(|(k, &kind)| DistanceColumn { values: data.iter().map(|r| r[k]).collect(), kind })
        .collect()
}

/// Every entry within `rel` of the reference, relative to the entry.
fn assert_matrix(got: &DistanceMatrix, want: &[Vec<f64>], rel: f64, what: &str) {
    assert_eq!(got.n(), want.len(), "{what}");
    for (i, row) in want.iter().enumerate() {
        for (j, &w) in row.iter().enumerate() {
            let g = got.get(i, j);
            assert!((g - w).abs() <= rel * w.abs() + 1e-15, "{what} [{i}, {j}]: {g} vs {w}");
        }
    }
}

fn counts() -> Vec<DistanceColumn> {
    let data = rows(reference().get("counts").unwrap());
    columns(&data, &[VariableKind::Numeric; 6])
}

fn gower() -> DistanceMatrix {
    let r = reference();
    let data = rows(r.get("mixed").unwrap());
    let kinds = [
        VariableKind::Numeric,
        VariableKind::Numeric,
        VariableKind::Ordinal,
        VariableKind::Nominal,
        VariableKind::AsymmetricBinary,
    ];
    distance_matrix(&columns(&data, &kinds), DistanceMetric::Gower).unwrap()
}

#[test]
fn distances_match_scipy_pdist() {
    let r = reference();
    let pdist = r.get("pdist").unwrap();
    let c = counts();
    for metric in [
        DistanceMetric::Euclidean,
        DistanceMetric::Manhattan,
        DistanceMetric::BrayCurtis,
        DistanceMetric::Jaccard,
        DistanceMetric::Hamming,
        DistanceMetric::SimpleMatching,
    ] {
        let want = rows(pdist.get(metric.key()).unwrap());
        assert_matrix(&distance_matrix(&c, metric).unwrap(), &want, 1e-12, metric.key());
    }
}

#[test]
fn gower_matches_r_daisy_with_missing_values() {
    let want = rows(reference().get("gower").unwrap());
    assert_matrix(&gower(), &want, 1e-12, "Gower");
}

/// Eigenvalues within `1e-9 · λ₁`, coordinates within `1e-8` of the largest
/// coordinate, each axis compared up to sign.
fn assert_pcoa(
    d: &DistanceMatrix,
    correction: NegativeCorrection,
    eigen: &[f64],
    vectors: &[Vec<f64>],
    what: &str,
) -> stat_multivariate::PrincipalCoordinates {
    let p = principal_coordinates(d, correction, usize::MAX).unwrap();
    assert_eq!(p.eigenvalues.len(), eigen.len(), "{what}");
    let top = eigen[0].abs();
    for (g, w) in p.eigenvalues.iter().zip(eigen) {
        assert!((g - w).abs() <= 1e-9 * top, "{what} eigenvalue: {g} vs {w}");
    }
    let axes = vectors[0].len();
    assert_eq!(p.coordinates[0].len(), axes, "{what} axes");
    let scale = vectors.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()));
    for k in 0..axes {
        let dot: f64 = p.coordinates.iter().zip(vectors).map(|(a, b)| a[k] * b[k]).sum();
        let sign = dot.signum();
        for (a, b) in p.coordinates.iter().zip(vectors) {
            assert!((a[k] - sign * b[k]).abs() <= 1e-8 * scale, "{what} axis {k}: {} vs {}", a[k], b[k]);
        }
    }
    p
}

#[test]
fn principal_coordinates_match_r_cmdscale_and_ape() {
    let r = reference();
    let c = counts();
    let cases =
        [("pcoa_gower", gower()), ("pcoa_braycurtis", distance_matrix(&c, DistanceMetric::BrayCurtis).unwrap())];
    for (tag, d) in cases {
        let refs = r.get(tag).unwrap();
        let none = assert_pcoa(
            &d,
            NegativeCorrection::None,
            &nums(refs.get("eigenvalues").unwrap()),
            &rows(refs.get("none_vectors").unwrap()),
            tag,
        );
        assert!(none.has_negative_eigenvalues(), "{tag}");
        for corr in [NegativeCorrection::Lingoes, NegativeCorrection::Cailliez] {
            let what = format!("{tag} {}", corr.key());
            let cr = refs.get(corr.key()).unwrap();
            let p =
                assert_pcoa(&d, corr, &nums(cr.get("eigenvalues").unwrap()), &rows(cr.get("vectors").unwrap()), &what);
            let c0 = num(cr.get("constant").unwrap());
            let got = p.correction_constant.unwrap();
            assert!((got - c0).abs() <= 1e-9 * c0, "{what} constant: {got} vs {c0}");
            assert_eq!(p.raw_eigenvalues, none.eigenvalues, "{what}");
            // ape's relative eigenvalues and broken stick, over its positive axes.
            let k = p.positive_axes();
            let rel = nums(cr.get("relative").unwrap());
            for (g, w) in p.relative.iter().zip(&rel).take(k) {
                assert!((g - w).abs() <= 1e-9, "{what} relative: {g} vs {w}");
            }
            let bs = nums(cr.get("broken_stick").unwrap());
            for (g, w) in p.broken_stick.iter().zip(&bs) {
                assert!((g - w).abs() <= 1e-12, "{what} broken stick: {g} vs {w}");
            }
            let pos = nums(cr.get("positive_eigenvalues").unwrap());
            for (g, w) in p.eigenvalues.iter().zip(&pos).take(k) {
                assert!((g - w).abs() <= 1e-9 * pos[0], "{what} ape eigenvalue: {g} vs {w}");
            }
        }
    }
}

#[test]
fn clustering_a_gower_matrix_matches_r_hclust_and_scipy() {
    let r = reference();
    let refs = r.get("hclust").unwrap();
    let d = gower();
    for (name, linkage) in [
        ("single", Linkage::Single),
        ("complete", Linkage::Complete),
        ("average", Linkage::Average),
        ("ward.D2", Linkage::Ward),
    ] {
        let tree = hierarchical_from_distances(&d, linkage).unwrap();
        let c = refs.get(name).unwrap();
        for key in ["height", "scipy_height"] {
            let want = nums(c.get(key).unwrap());
            assert_eq!(tree.merges.len(), want.len());
            for (m, w) in tree.merges.iter().zip(&want) {
                assert!((m.height - w).abs() <= 1e-12 * w.abs().max(1e-3), "{name} {key}: {} vs {w}", m.height);
            }
        }
        // Same three-cluster partition (labels may be numbered differently).
        let want: Vec<usize> = nums(c.get("cut3").unwrap()).into_iter().map(|v| v as usize).collect();
        let got = tree.cut(3).unwrap();
        for i in 0..got.len() {
            for j in 0..got.len() {
                assert_eq!(got[i] == got[j], want[i] == want[j], "{name}: rows {i}, {j}");
            }
        }
    }
}
