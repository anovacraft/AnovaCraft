//! t-SNE tests. Affinity references come from `tests/data/tsne_reference.py`
//! (float64 calibration by Brent's method, checked there against
//! scikit-learn); the gradient is checked against finite differences, the
//! Barnes–Hut gradient against the exact one, and embeddings structurally.

use super::*;
use crate::embedding::trustworthiness;

const DATA: &str = include_str!("../../tests/data/tsne_data.csv");

/// The 60 five-dimensional points (three clusters of 20) and a fixed layout.
pub(crate) fn data() -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let mut x = Vec::new();
    let mut y = Vec::new();
    for line in DATA.lines().skip(1) {
        let v: Vec<f64> = line.split(',').map(|s| s.parse().unwrap()).collect();
        x.push(v[..5].to_vec());
        y.push(v[5..].to_vec());
    }
    (x, y)
}

fn rel(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol * b.abs(), "{a} vs {b}");
}

#[test]
#[allow(clippy::unreadable_literal)]
fn exact_affinities_match_the_reference() {
    let (x, _) = data();
    let prox = Proximity::Points(&x);
    let p = joint_probabilities(&prox, 10.0).unwrap();
    rel(p[0][1], 0.00036301755494062973, 1e-7);
    rel(p[0][59], 7.586397010155231e-10, 1e-6);
    rel(p[10][11], 5.271802287908016e-05, 1e-7);
    rel(p[30][45], 4.166028419228351e-15, 1e-5);
    rel(p[59][58], 0.0017539066007124298, 1e-7);
    let total: f64 = p.iter().flatten().sum();
    assert!((total - 1.0).abs() < 1e-12);
    // Each row's conditional distribution has perplexity 10.
    for i in [0, 17, 42] {
        let sq: Vec<f64> = (0..60).filter(|&j| j != i).map(|j| prox.distance(i, j).powi(2)).collect();
        let (c, beta) = calibrate(&sq, 10.0);
        let h = -c.iter().filter(|v| **v > 0.0).map(|v| v * v.ln()).sum::<f64>();
        assert!((h.exp() - 10.0).abs() < 1e-8);
        assert!(beta > 0.0);
    }
}

#[test]
#[allow(clippy::unreadable_literal)]
fn sparse_affinities_match_the_reference() {
    let (x, _) = data();
    let p = sparse_joint_probabilities(&Proximity::Points(&x), 5.0).unwrap();
    let get = |i: usize, j: usize| p[i].iter().find(|e| e.0 == j).map_or(0.0, |e| e.1);
    rel(get(0, 1), 5.366405916441074e-05, 1e-7);
    rel(get(10, 11), 5.241567690740936e-06, 1e-6);
    rel(get(59, 58), 0.0015788978718084647, 1e-7);
    assert_eq!(get(0, 59), 0.0);
    assert_eq!(get(30, 45), 0.0);
    assert_eq!(p.iter().map(Vec::len).sum::<usize>(), 1084);
    // Symmetric.
    for (i, row) in p.iter().enumerate() {
        for &(j, v) in row {
            assert!((get(j, i) - v).abs() < 1e-18);
        }
    }
}

#[test]
#[allow(clippy::unreadable_literal)]
fn trustworthiness_matches_scikit_learn() {
    let (x, y) = data();
    let t = trustworthiness(&Proximity::Points(&x), &y, 5).unwrap();
    assert!((t - 0.9106410256410257).abs() < 1e-12, "{t}");
}

#[test]
fn gradient_matches_finite_differences() {
    let (x, _) = data();
    let p = joint_probabilities(&Proximity::Points(&x), 10.0).unwrap();
    let mut rng = Rng::new(3);
    let y: Vec<Vec<f64>> = (0..60).map(|_| vec![rng.normal(), rng.normal()]).collect();
    let (_, grad) = exact_gradient(&p, &y);
    let h = 1e-6;
    for &(i, k) in &[(0, 0), (7, 1), (33, 0), (59, 1)] {
        let mut up = y.clone();
        up[i][k] += h;
        let mut down = y.clone();
        down[i][k] -= h;
        let fd = (exact_gradient(&p, &up).0 - exact_gradient(&p, &down).0) / (2.0 * h);
        assert!((fd - grad[i][k]).abs() < 1e-7 * (1.0 + fd.abs()), "{fd} vs {}", grad[i][k]);
    }
}

#[test]
fn barnes_hut_with_theta_zero_is_exact() {
    let (x, _) = data();
    let dense = joint_probabilities(&Proximity::Points(&x), 10.0).unwrap();
    let sparse: SparseAffinities = dense
        .iter()
        .enumerate()
        .map(|(i, r)| r.iter().enumerate().filter(|(j, _)| *j != i).map(|(j, &v)| (j, v)).collect())
        .collect();
    let mut rng = Rng::new(5);
    let y: Vec<Vec<f64>> = (0..60).map(|_| vec![3.0 * rng.normal(), 3.0 * rng.normal()]).collect();
    let (_, exact) = exact_gradient(&dense, &y);
    let (bh, _) = barnes_hut_gradient(&sparse, &y, 0.0);
    for (a, b) in exact.iter().flatten().zip(bh.iter().flatten()) {
        assert!((a - b).abs() < 1e-12 * (1.0 + a.abs()), "{a} vs {b}");
    }
    // θ = 0.5 stays close.
    let (approx, _) = barnes_hut_gradient(&sparse, &y, 0.5);
    let err: f64 =
        exact.iter().flatten().zip(approx.iter().flatten()).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt();
    let size: f64 = exact.iter().flatten().map(|a| a * a).sum::<f64>().sqrt();
    assert!(err < 0.05 * size, "{err} vs {size}");
}

/// Share of points whose nearest embedded cluster centre is their own cluster.
pub(crate) fn separation(y: &[Vec<f64>], labels: &[usize], k: usize) -> f64 {
    let d = y[0].len();
    let mut centres = vec![vec![0.0; d]; k];
    let mut counts = vec![0.0; k];
    for (p, &l) in y.iter().zip(labels) {
        counts[l] += 1.0;
        for a in 0..d {
            centres[l][a] += p[a];
        }
    }
    for (c, n) in centres.iter_mut().zip(&counts) {
        c.iter_mut().for_each(|v| *v /= n);
    }
    let right = y
        .iter()
        .zip(labels)
        .filter(|(p, l)| {
            let near = (0..k).min_by(|&a, &b| sq_dist(p, &centres[a]).total_cmp(&sq_dist(p, &centres[b]))).unwrap();
            near == **l
        })
        .count();
    right as f64 / y.len() as f64
}

#[test]
fn exact_embedding_separates_the_clusters() {
    let (x, _) = data();
    let labels: Vec<usize> = (0..60).map(|i| i / 20).collect();
    let options = TsneOptions { perplexity: 10.0, ..TsneOptions::new() };
    let t = tsne(&Proximity::Points(&x), &options).unwrap();
    assert_eq!(t.method, TsneMethod::Exact);
    assert!((separation(&t.coordinates, &labels, 3) - 1.0).abs() < 1e-12);
    assert!(trustworthiness(&Proximity::Points(&x), &t.coordinates, 5).unwrap() > 0.9);
    assert!(t.kl_divergence > 0.0 && t.kl_divergence < 1.0, "{}", t.kl_divergence);
    // Reproducible, also from a random start.
    assert_eq!(t, tsne(&Proximity::Points(&x), &options).unwrap());
    let r = TsneOptions { random_start: true, seed: 9, ..options };
    let tr = tsne(&Proximity::Points(&x), &r).unwrap();
    assert!(separation(&tr.coordinates, &labels, 3) > 0.95);
    // A distance matrix gives the same affinities and (up to rounding) the
    // same start; the layout then separates the clusters as well.
    let m = Proximity::Points(&x).matrix();
    let (pp, pd) = (Proximity::Points(&x), Proximity::Distances(&m));
    let (a, b) = (joint_probabilities(&pp, 10.0).unwrap(), joint_probabilities(&pd, 10.0).unwrap());
    assert!(a.iter().flatten().zip(b.iter().flatten()).all(|(u, v)| (u - v).abs() < 1e-14));
    let (sa, sb) = (start_layout(&pp, 2, false, 1).unwrap(), start_layout(&pd, 2, false, 1).unwrap());
    assert!(sa.iter().flatten().zip(sb.iter().flatten()).all(|(u, v)| (u - v).abs() < 1e-12));
    let td = tsne(&pd, &options).unwrap();
    assert!((separation(&td.coordinates, &labels, 3) - 1.0).abs() < 1e-12);
    assert!(tsne(&Proximity::Points(&x), &TsneOptions { perplexity: 59.0, ..options }).is_err());
    assert!(
        tsne(&Proximity::Points(&x), &TsneOptions { dims: 3, method: Some(TsneMethod::BarnesHut), ..options }).is_err()
    );
}

#[test]
fn barnes_hut_embedding_separates_the_clusters() {
    // 400 points in four clusters in 6-D.
    let mut rng = Rng::new(11);
    let mut x = Vec::new();
    let mut labels = Vec::new();
    for i in 0..400 {
        let g = i % 4;
        let mut p: Vec<f64> = (0..6).map(|_| rng.normal()).collect();
        p[g] += 8.0;
        x.push(p);
        labels.push(g);
    }
    let options = TsneOptions { method: Some(TsneMethod::BarnesHut), iterations: 500, ..TsneOptions::new() };
    let t = tsne(&Proximity::Points(&x), &options).unwrap();
    assert_eq!(t.method, TsneMethod::BarnesHut);
    assert!(separation(&t.coordinates, &labels, 4) > 0.99);
    assert!(trustworthiness(&Proximity::Points(&x), &t.coordinates, 10).unwrap() > 0.85);
}
