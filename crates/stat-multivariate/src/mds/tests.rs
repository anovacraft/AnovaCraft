//! Multidimensional scaling tests. Reference values from
//! `tests/data/mds_reference.py` (numpy eigen-decomposition for classical
//! scaling; scikit-learn's `smacof` for metric scaling from the classical start).

use super::*;
use crate::embedding::Proximity;

#[allow(clippy::unreadable_literal)]
fn dissimilarities() -> Vec<Vec<f64>> {
    vec![
        vec![0.000, 4.900, 2.478, 2.878, 3.016, 5.552, 10.452, 4.387, 3.681, 2.702],
        vec![4.900, 0.000, 6.941, 4.048, 4.902, 7.071, 5.552, 4.855, 5.203, 2.973],
        vec![2.478, 6.941, 0.000, 4.592, 5.140, 7.309, 12.493, 6.864, 5.369, 5.179],
        vec![2.878, 4.048, 4.592, 0.000, 5.404, 8.430, 9.600, 4.756, 6.559, 1.566],
        vec![3.016, 4.902, 5.140, 5.404, 0.000, 3.026, 8.551, 1.996, 4.129, 3.839],
        vec![5.552, 7.071, 7.309, 8.430, 3.026, 0.000, 9.283, 3.674, 5.989, 6.864],
        vec![10.452, 5.552, 12.493, 9.600, 8.551, 9.283, 0.000, 7.156, 9.054, 8.525],
        vec![4.387, 4.855, 6.864, 4.756, 1.996, 3.674, 7.156, 0.000, 6.125, 3.507],
        vec![3.681, 5.203, 5.369, 6.559, 4.129, 5.989, 9.054, 6.125, 0.000, 4.993],
        vec![2.702, 2.973, 5.179, 1.566, 3.839, 6.864, 8.525, 3.507, 4.993, 0.000],
    ]
}

#[allow(clippy::unreadable_literal)]
const EIGENVALUES: [f64; 10] = [
    98.41561731472855,
    49.27871162242674,
    22.927862803842505,
    4.34125922961831,
    1.3158162212130804,
    0.291567043192012,
    -2.5006423768339623e-15,
    -0.9478696441395246,
    -4.813480295259014,
    -4.991056195622646,
];
#[allow(clippy::unreadable_literal)]
const CLASSICAL_DISTANCES: [f64; 45] = [
    5.150558158662465,
    2.066801110361243,
    7.03355876688764,
    3.468719584392161,
    3.757541922735634,
    4.458541758422767,
    3.1266677756462924,
    4.7180708413011585,
    4.957737123553714,
    5.438555162926182,
    5.802926095734768,
    7.128850615703392,
    7.254725387358588,
    8.389673163900104,
    2.952143685593301,
    10.443609789027592,
    5.826199557519445,
    12.486167947618252,
    9.56950532993669,
    8.526264796371146,
    9.250954482085898,
    4.105439347172745,
    3.3925836717040148,
    6.140244524390664,
    5.278152782570419,
    1.7346452341119942,
    3.736925405614497,
    6.812187901342065,
    2.212694282880961,
    4.1006321248034885,
    4.205054944250992,
    4.335129565196514,
    1.116673952092218,
    4.064370973150464,
    8.62329393022077,
    1.9515453891594354,
    2.8100389864061848,
    2.5808725764390745,
    4.512085284946283,
    1.7649410477935126,
    3.8467770609375567,
    6.765670379229048,
    8.319179793718865,
    3.5160963550152844,
    2.8015086023502356,
];
#[allow(clippy::unreadable_literal)]
const SMACOF_STRESS: f64 = 21.659844886102363;
#[allow(clippy::unreadable_literal)]
const SMACOF_DISTANCES: [f64; 45] = [
    4.847013660201677,
    2.573690933397278,
    7.113742653672114,
    3.0966618265722246,
    3.5590385543326235,
    4.294950447257625,
    3.2282708236451705,
    4.631643213812203,
    5.583767966854676,
    5.295371382721304,
    6.159536140518705,
    7.14515755293164,
    8.179720400970437,
    8.400004421181604,
    3.1119561380208083,
    10.094729887673246,
    5.796241250722867,
    12.602645538625062,
    9.353129479391047,
    8.2016589595383,
    8.783197312647376,
    4.39844715869689,
    3.6088925051503042,
    6.943134200382976,
    5.500630910617366,
    1.799368002695826,
    3.543252027171093,
    6.403833732880416,
    3.064428705645933,
    6.9958381307326425,
    4.072630547793696,
    6.137025479121986,
    2.96691516565071,
    4.427288260998228,
    11.16409200273865,
    4.7658578902974185,
    2.4812927857191496,
    2.5278497476029664,
    4.589816033983494,
    1.66425585824884,
    3.7619085025166683,
    6.828290356720956,
    8.187522010681555,
    3.8363750527692755,
    5.172573413432318,
];

fn lower(x: &[Vec<f64>]) -> Vec<f64> {
    let mut out = Vec::new();
    for i in 0..x.len() {
        for j in 0..i {
            out.push(dist(&x[i], &x[j]));
        }
    }
    out
}

#[test]
fn classical_scaling_matches_the_eigen_decomposition() {
    let d = dissimilarities();
    let c = classical_scaling(&Proximity::Distances(&d), 2).unwrap();
    for (a, b) in c.eigenvalues.iter().zip(EIGENVALUES) {
        assert!((a - b).abs() < 1e-10 * 98.0, "{a} vs {b}");
    }
    for (a, b) in lower(&c.coordinates).iter().zip(CLASSICAL_DISTANCES) {
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }
    // Share of the positive eigenvalue total in two dimensions.
    let pos: f64 = EIGENVALUES.iter().filter(|v| **v > 0.0).sum();
    assert!((c.proportion() - (EIGENVALUES[0] + EIGENVALUES[1]) / pos).abs() < 1e-12);
}

#[test]
fn classical_scaling_of_points_is_their_principal_component_scores() {
    // For Euclidean distances, the data route (SVD) and the matrix route
    // (eigen-decomposition of B) give the same configuration.
    let pts: Vec<Vec<f64>> = (0..12)
        .map(|i| {
            let t = f64::from(i);
            vec![t.sin() * 3.0, t.cos() + 0.1 * t, (0.7 * t).sin() * 0.5, 0.05 * t * t]
        })
        .collect();
    let a = classical_scaling(&Proximity::Points(&pts), 3).unwrap();
    let m = Proximity::Points(&pts).matrix();
    let b = classical_scaling(&Proximity::Distances(&m), 3).unwrap();
    for (u, v) in lower(&a.coordinates).iter().zip(lower(&b.coordinates)) {
        assert!((u - v).abs() < 1e-9);
    }
    for k in 0..4 {
        assert!((a.eigenvalues[k] - b.eigenvalues[k]).abs() < 1e-9 * a.eigenvalues[0]);
    }
    assert!(a.eigenvalues[5..].iter().all(|v| *v == 0.0));
    // Two dimensions of four-dimensional data cannot be exact; four are.
    let full = classical_scaling(&Proximity::Points(&pts), 4).unwrap();
    assert!((full.proportion() - 1.0).abs() < 1e-12);
    for (u, v) in lower(&full.coordinates).iter().zip(lower(&pts)) {
        assert!((u - v).abs() < 1e-9);
    }
    assert!(classical_scaling(&Proximity::Points(&pts), 12).is_err());
}

#[test]
fn smacof_matches_scikit_learn_from_the_classical_start() {
    let d = dissimilarities();
    let start = classical_scaling(&Proximity::Distances(&d), 2).unwrap().coordinates;
    let s0 = raw_stress(&d, &start);
    let fit = smacof_from(&d, start, 5000, 0.0).unwrap();
    assert!((fit.raw_stress - SMACOF_STRESS).abs() < 1e-8 * SMACOF_STRESS, "{}", fit.raw_stress);
    assert!(fit.raw_stress < s0);
    for (a, b) in lower(&fit.coordinates).iter().zip(SMACOF_DISTANCES) {
        assert!((a - b).abs() < 1e-5, "{a} vs {b}");
    }
    let total: f64 = (0..10).map(|i| (0..i).map(|j| d[i][j] * d[i][j]).sum::<f64>()).sum();
    assert!((fit.stress1 - (fit.raw_stress / total).sqrt()).abs() < 1e-12);
    // With random starts the stress can only be as low or lower; it is
    // reproducible from the seed.
    let options = SmacofOptions { max_iterations: 5000, tolerance: 1e-12, ..SmacofOptions::new(2) };
    let best = metric_scaling(&Proximity::Distances(&d), &options).unwrap();
    assert!(best.raw_stress <= SMACOF_STRESS * (1.0 + 1e-8));
    assert_eq!(best, metric_scaling(&Proximity::Distances(&d), &options).unwrap());
}

#[test]
fn exact_euclidean_configurations_have_zero_stress() {
    let pts = vec![vec![0.0, 0.0], vec![3.0, 0.0], vec![0.0, 4.0], vec![3.0, 4.0], vec![1.0, 1.0]];
    let fit = metric_scaling(&Proximity::Points(&pts), &SmacofOptions::new(2)).unwrap();
    assert!(fit.stress1 < 1e-6, "{}", fit.stress1);
    assert_eq!(fit.start, 0);
}
