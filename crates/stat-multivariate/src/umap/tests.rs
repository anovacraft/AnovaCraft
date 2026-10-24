//! UMAP tests. The fuzzy graph and curve parameters are checked against
//! umap-learn (`tests/data/umap_reference.py`); the spectral start against a
//! dense eigen-decomposition; embeddings structurally.

use super::*;
use crate::embedding::trustworthiness;

const DATA: &str = include_str!("../../tests/data/tsne_data.csv");

fn data() -> Vec<Vec<f64>> {
    DATA.lines().skip(1).map(|l| l.split(',').take(5).map(|s| s.parse().unwrap()).collect()).collect()
}

fn rel(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * b.abs().max(1e-3), "{a} vs {b}");
}

#[test]
#[allow(clippy::unreadable_literal)]
fn fuzzy_graph_matches_umap_learn() {
    let x = data();
    let g = fuzzy_graph(&Proximity::Points(&x), 10).unwrap();
    for (i, rho, sigma) in [
        (0, 1.8720855712890625, 0.22109603881835938),
        (25, 1.7502355575561523, 0.20150375366210938),
        (59, 0.9861235618591309, 0.8153610229492188),
    ] {
        rel(g.rho[i], rho, 1e-6);
        rel(g.sigma[i], sigma, 1e-4);
    }
    let w = |i: usize, j: usize| g.rows[i].iter().find(|e| e.0 == j).map_or(0.0, |e| e.1);
    rel(w(0, 18), 1.0, 1e-12);
    rel(w(0, 9), 0.012392697855830193, 1e-3);
    rel(w(25, 39), 0.4812752604484558, 1e-4);
    rel(w(59, 57), 0.5480189323425293, 1e-4);
    assert_eq!(g.rows.iter().map(Vec::len).sum::<usize>(), 702);
    // Each local fuzzy set sums to log₂ k over the neighbours.
    let prox = Proximity::Points(&x);
    let nn = prox.nearest_neighbours(9);
    for i in [0, 30] {
        let s: f64 = nn[i].iter().map(|&(_, d)| (-(d - g.rho[i]).max(0.0) / g.sigma[i]).exp()).sum();
        assert!((s - 10f64.log2()).abs() < 1e-9);
    }
}

#[test]
#[allow(clippy::unreadable_literal)]
fn curve_parameters_match_umap_learn() {
    for (spread, min_dist, a, b) in [
        (1.0, 0.1, 1.5769434602697652, 0.8950608778515733),
        (1.0, 0.5, 0.5830300203414425, 1.3341669924314914),
        (2.0, 0.0, 0.6460466719525826, 0.7904944906889044),
    ] {
        let (fa, fb) = curve_parameters(spread, min_dist).unwrap();
        rel(fa, a, 1e-6);
        rel(fb, b, 1e-6);
    }
    assert!(curve_parameters(1.0, 1.0).is_err());
}

#[test]
fn subspace_iteration_matches_the_dense_spectral_start() {
    // A ring of 200 points in 3-D (one connected graph with clear gaps).
    let x: Vec<Vec<f64>> = (0..200)
        .map(|i| {
            let t = f64::from(i) * std::f64::consts::TAU / 200.0;
            vec![t.cos() * 3.0, t.sin() * 2.0, (3.0 * t).sin() * 0.3]
        })
        .collect();
    let g = fuzzy_graph(&Proximity::Points(&x), 8).unwrap();
    let dense = spectral_start_with(&g, 2, 1000).unwrap();
    let iter = spectral_start_with(&g, 2, 10).unwrap();
    // Same two-dimensional subspace: each iterated vector lies in the span
    // of the dense ones.
    for k in 0..2 {
        let v: Vec<f64> = iter.iter().map(|r| r[k]).collect();
        let proj: f64 = (0..2)
            .map(|q| {
                let u: Vec<f64> = dense.iter().map(|r| r[q]).collect();
                dot(&v, &u).powi(2)
            })
            .sum();
        assert!((proj - 1.0).abs() < 1e-6, "{proj}");
    }
}

/// Share of points whose nearest embedded cluster centre is their own.
fn separation(y: &[Vec<f64>], labels: &[usize], k: usize) -> f64 {
    let d = y[0].len();
    let mut c = vec![vec![0.0; d]; k];
    let mut m = vec![0.0; k];
    for (p, &l) in y.iter().zip(labels) {
        m[l] += 1.0;
        (0..d).for_each(|a| c[l][a] += p[a]);
    }
    c.iter_mut().zip(&m).for_each(|(c, m)| c.iter_mut().for_each(|v| *v /= m));
    let d2 = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(u, v)| (u - v) * (u - v)).sum::<f64>();
    let right = y
        .iter()
        .zip(labels)
        .filter(|(p, l)| (0..k).min_by(|&a, &b| d2(p, &c[a]).total_cmp(&d2(p, &c[b]))).unwrap() == **l)
        .count();
    right as f64 / y.len() as f64
}

#[test]
fn embedding_separates_clusters_and_is_reproducible() {
    let mut rng = Rng::new(21);
    let mut x = Vec::new();
    let mut labels = Vec::new();
    for i in 0..300 {
        let g = i % 3;
        let mut p: Vec<f64> = (0..8).map(|_| rng.normal()).collect();
        p[g] += 7.0;
        x.push(p);
        labels.push(g);
    }
    let options = UmapOptions { epochs: Some(200), ..UmapOptions::new() };
    let u = umap(&Proximity::Points(&x), &options).unwrap();
    assert!(u.spectral);
    assert!(separation(&u.coordinates, &labels, 3) > 0.99);
    assert!(trustworthiness(&Proximity::Points(&x), &u.coordinates, 10).unwrap() > 0.85);
    assert_eq!(u, umap(&Proximity::Points(&x), &options).unwrap());
    let r = umap(&Proximity::Points(&x), &UmapOptions { random_start: true, ..options }).unwrap();
    assert!(!r.spectral);
    assert!(separation(&r.coordinates, &labels, 3) > 0.95);
    assert!(umap(&Proximity::Points(&x), &UmapOptions { neighbours: 1, ..options }).is_err());
    assert!(umap(&Proximity::Points(&x), &UmapOptions { dims: 4, ..options }).is_err());
}
