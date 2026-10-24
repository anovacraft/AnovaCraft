//! Gaussian mixture tests. Reference values come from
//! `tests/data/mixture_reference.py`: scikit-learn's `GaussianMixture`
//! (VII, VVI, EEE, VVV) and a direct NumPy EM (EII, EEI), each iterated to
//! convergence from the M-step of the same starting partition. The
//! log-likelihood agrees to 1e-9; parameters to 1e-5 (the likelihood is flat
//! enough near the optimum that both iterations stop that far apart).

use super::*;

const DATA: &str = include_str!("../../tests/data/mixture_data.csv");

fn data() -> (Vec<Vec<f64>>, Vec<usize>) {
    let mut points = Vec::new();
    let mut labels = Vec::new();
    for line in DATA.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        points.push(vec![f[0].parse().unwrap(), f[1].parse().unwrap()]);
        labels.push(f[2].parse().unwrap());
    }
    (points, labels)
}

struct Reference {
    structure: CovarianceStructure,
    ll: f64,
    bic: f64,
    weights: [f64; 3],
    means: [[f64; 2]; 3],
    cov: [[[f64; 2]; 2]; 3],
}

#[allow(clippy::unreadable_literal)]
fn references() -> Vec<Reference> {
    use CovarianceStructure as C;
    vec![
        Reference {
            structure: C::Eii,
            ll: -683.897364515444,
            bic: 1412.8904466777542,
            weights: [0.37128244622077045, 0.35465583591620436, 0.27406171786302497],
            means: [
                [0.24741590568512586, 0.42335644193764177],
                [6.7827497259684995, 0.9765219615345695],
                [1.9694983815668612, 8.03376611570267],
            ],
            cov: [[[1.9528673164680177, 0.0], [0.0, 1.9528673164680177]]; 3],
        },
        Reference {
            structure: C::Eei,
            ll: -681.1201118679453,
            bic: 1412.3465766768531,
            weights: [0.37149015522080975, 0.3515730525626899, 0.2769367922165001],
            means: [
                [0.2504006136938825, 0.4189986788814567],
                [6.818325496866985, 0.9635197015702042],
                [1.9752023226131095, 7.9832668887673295],
            ],
            cov: [[[1.5569040753821985, 0.0], [0.0, 2.354165837364226]]; 3],
        },
        Reference {
            structure: C::Vii,
            ll: -673.1000705501913,
            bic: 1401.3171293354415,
            weights: [0.43095101343230785, 0.3190272621792999, 0.25002172438839226],
            means: [
                [0.716700273301692, 0.8901789709299368],
                [7.0401643445995346, 0.7540674815084394],
                [1.9290323761842936, 8.32355733019612],
            ],
            cov: [
                [[3.892585694166914, 0.0], [0.0, 3.892585694166914]],
                [[0.9585681729864235, 0.0], [0.0, 0.9585681729864235]],
                [[1.250609961629406, 0.0], [0.0, 1.250609961629406]],
            ],
        },
        Reference {
            structure: C::Vvi,
            ll: -665.6312378806551,
            bic: 1401.4113698786578,
            weights: [0.39942759068993466, 0.33129742637301784, 0.2692749829370476],
            means: [
                [0.5047085213805694, 0.621623862337068],
                [6.978825984736832, 0.8434527853556103],
                [1.9441296183438637, 8.086656577112032],
            ],
            cov: [
                [[3.1657565662310487, 0.0], [0.0, 3.240854393172519]],
                [[0.9705390193750318, 0.0], [0.0, 1.2621914230146594]],
                [[0.6093087153760979, 0.0], [0.0, 2.5401858965936412]],
            ],
        },
        Reference {
            structure: C::Eee,
            ll: -678.5788222659155,
            bic: 1412.27463276689,
            weights: [0.3881183361967548, 0.3424748368320728, 0.26940682697117235],
            means: [
                [0.3814010580054335, 0.5471717257235691],
                [6.904025010960668, 0.9126497814153444],
                [1.9475508066364402, 8.09309411442739],
            ],
            cov: [[[1.6025606109391175, 0.49576795106449634], [0.49576795106449634, 2.3485489302168965]]; 3],
        },
        Reference {
            structure: C::Vvv,
            ll: -625.9194183220759,
            bic: 1337.019636643788,
            weights: [0.4039093963954177, 0.33136470609771207, 0.2647258975068703],
            means: [
                [0.5455680778045402, 0.6868406844124335],
                [6.9675770296169, 0.8125616577413244],
                [1.9189579995932584, 8.154042085373053],
            ],
            cov: [
                [[3.2977444250108605, 2.5955101654012807], [2.5955101654012807, 3.497107230640413]],
                [[1.0142843917949367, -0.7035580873509696], [-0.7035580873509696, 1.091839662488549]],
                [[0.5861936530748082, -0.014572191837393224], [-0.014572191837393228, 2.3073639943635516]],
            ],
        },
    ]
}

fn close(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol * (1.0 + b.abs()), "{what}: {a} vs {b}");
}

fn tight(structure: CovarianceStructure) -> MixtureOptions {
    MixtureOptions { tolerance: 1e-14, max_iterations: 100_000, ..MixtureOptions::new(3, structure) }
}

#[test]
fn every_structure_matches_the_reference_optimum() {
    let (points, labels) = data();
    for r in references() {
        let z0 = one_hot(&labels, 3);
        let fit = fit_gaussian_mixture_from(&points, z0, &tight(r.structure)).unwrap();
        let what = r.structure.key();
        assert!(fit.converged, "{what}");
        close(fit.log_likelihood, r.ll, 1e-9, what);
        close(fit.bic(), r.bic, 1e-9, what);
        for c in 0..3 {
            close(fit.weights[c], r.weights[c], 1e-5, what);
            for a in 0..2 {
                close(fit.means[c][a], r.means[c][a], 1e-5, what);
                for b in 0..2 {
                    close(fit.covariances[c][a][b], r.cov[c][a][b], 1e-5, what);
                }
            }
        }
        // Membership probabilities sum to one.
        assert!(fit.probabilities.iter().all(|z| (z.iter().sum::<f64>() - 1.0).abs() < 1e-12));
    }
}

#[test]
fn starts_reach_the_optimum_and_bic_prefers_full_varying() {
    let (points, labels) = data();
    let r = references().pop().unwrap();
    // From k-means or Ward starts, VVV reaches the same optimum (up to labels).
    for start in [MixtureStart::KMeans, MixtureStart::Hierarchical] {
        let options = MixtureOptions { start, tolerance: 1e-12, ..MixtureOptions::new(3, CovarianceStructure::Vvv) };
        let fit = fit_gaussian_mixture(&points, &options).unwrap();
        close(fit.log_likelihood, r.ll, 1e-8, start.key());
        assert_eq!(fit.classification()[0], 0, "components numbered by first appearance");
        // Each fitted mean is near one reference mean.
        for m in &fit.means {
            assert!(r.means.iter().any(|q| (q[0] - m[0]).abs() < 1e-4 && (q[1] - m[1]).abs() < 1e-4));
        }
    }
    let sel = select_gaussian_mixture(
        &points,
        &[1, 2, 3, 4],
        &CovarianceStructure::ALL,
        &MixtureOptions::new(1, CovarianceStructure::Vvv),
    )
    .unwrap();
    assert_eq!(sel.candidates.len(), 24);
    let best = &sel.candidates[sel.best];
    assert_eq!((best.structure, best.k), (CovarianceStructure::Vvv, 3));
    // The chosen model classifies most points as generated.
    let class = sel.chosen().classification();
    let mut agree = 0;
    for (c, l) in class.iter().zip(&labels) {
        agree += usize::from(*c == *l);
    }
    assert!(agree >= 140, "{agree} of 150");
    assert!(sel.chosen().uncertainty().iter().all(|u| (0.0..1.0).contains(u)));
}

#[test]
fn one_component_is_the_normal_maximum_likelihood_fit() {
    let (points, _) = data();
    let fit = fit_gaussian_mixture(&points, &MixtureOptions::new(1, CovarianceStructure::Vvv)).unwrap();
    let n = points.len() as f64;
    let mean: Vec<f64> = (0..2).map(|j| points.iter().map(|x| x[j]).sum::<f64>() / n).collect();
    let mut s = [[0.0; 2]; 2];
    for x in &points {
        for a in 0..2 {
            for b in 0..2 {
                s[a][b] += (x[a] - mean[a]) * (x[b] - mean[b]) / n;
            }
        }
    }
    let det = s[0][0] * s[1][1] - s[0][1] * s[1][0];
    // ℓ = −n/2 (p ln 2π + ln|S| + p) at the MLE.
    let ll = -n / 2.0 * (2.0 * (2.0 * std::f64::consts::PI).ln() + det.ln() + 2.0);
    close(fit.log_likelihood, ll, 1e-12, "K = 1");
    assert_eq!(fit.n_parameters(), 5);
    close(fit.covariances[0][0][1], s[0][1], 1e-12, "covariance");
}

#[test]
fn parameter_counts_prediction_and_failures() {
    use CovarianceStructure as C;
    let counts: Vec<usize> = C::ALL.iter().map(|s| s.parameters(3, 4)).collect();
    // 2 + 12 + (1, 3, 4, 12, 10, 30).
    assert_eq!(counts, [15, 17, 18, 26, 24, 44]);
    for s in C::ALL {
        assert_eq!(C::from_key(&s.key().to_lowercase()), Some(s));
    }
    let (points, labels) = data();
    let fit = fit_gaussian_mixture_from(&points, one_hot(&labels, 3), &tight(C::Vvv)).unwrap();
    let again = fit.predict(&points).unwrap();
    for (a, b) in again.iter().zip(&fit.probabilities) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-6);
        }
    }
    assert!(fit_gaussian_mixture(&points, &MixtureOptions::new(0, C::Vvv)).is_err());
    assert!(fit_gaussian_mixture(&points[..2], &MixtureOptions::new(3, C::Vvv)).is_err());
    assert!(fit_gaussian_mixture(&[vec![f64::NAN, 1.0]], &MixtureOptions::new(1, C::Eii)).is_err());
    // Three identical points cannot support a full covariance; a ridge fixes it.
    let flat = vec![vec![1.0, 2.0]; 3];
    assert!(fit_gaussian_mixture(&flat, &MixtureOptions::new(1, C::Vvv)).is_err());
    let ridged =
        fit_gaussian_mixture(&flat, &MixtureOptions { ridge: 1e-3, ..MixtureOptions::new(1, C::Vvv) }).unwrap();
    close(ridged.covariances[0][0][0], 1e-3, 1e-12, "ridge");
    // Deterministic.
    let o = MixtureOptions::new(3, C::Vvi);
    assert_eq!(fit_gaussian_mixture(&points, &o).unwrap(), fit_gaussian_mixture(&points, &o).unwrap());
}
