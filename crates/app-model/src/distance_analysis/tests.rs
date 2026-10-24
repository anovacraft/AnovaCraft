use super::*;
use crate::report::SectionKind;
use stat_data::{Column, parse_json};

const REFERENCE: &str = include_str!("../../../stat-multivariate/tests/data/distance_reference.json");

fn reference() -> JsonValue {
    parse_json(REFERENCE).unwrap()
}

fn numbers(v: &JsonValue) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()
}

/// The species counts of the reference file as a table, one column per species.
fn counts_table() -> DataTable {
    let r = reference();
    let rows: Vec<Vec<f64>> = r.get("counts").unwrap().as_array().unwrap().iter().map(numbers).collect();
    let cols = (0..rows[0].len())
        .map(|j| Column::numeric(format!("Species {}", j + 1), rows.iter().map(|row| row[j]).collect()))
        .collect();
    DataTable::new("counts", cols).unwrap()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

/// Distances through the report layer match SciPy's `pdist` (reference file).
#[test]
fn distance_table_matches_reference() {
    let t = counts_table();
    let states = RowStateManager::new(t.n_rows());
    let r = reference();
    for (metric, key) in [
        (DistanceMetric::Euclidean, "euclidean"),
        (DistanceMetric::Manhattan, "manhattan"),
        (DistanceMetric::BrayCurtis, "bray-curtis"),
    ] {
        let spec = DistanceSpec { columns: (0..t.n_cols()).collect(), metric, standardize: false };
        let (d, rows) = distance_table(&t, &states, &spec, None, "d").unwrap();
        assert_eq!(rows.len(), t.n_rows());
        assert_eq!(d.n_cols(), t.n_rows() + 1);
        let want: Vec<Vec<f64>> =
            r.get("pdist").unwrap().get(key).unwrap().as_array().unwrap().iter().map(numbers).collect();
        for (i, row) in want.iter().enumerate() {
            for (j, w) in row.iter().enumerate() {
                let got = d.column(j + 1).unwrap().as_numeric().unwrap()[i];
                assert!(close(got, *w, 1e-12), "{key} [{i},{j}]: {got} vs {w}");
            }
        }
    }
}

/// Excluded rows drop out of the matrix, rows and columns alike.
#[test]
fn excluded_rows_leave_the_matrix() {
    let t = counts_table();
    let mut states = RowStateManager::new(t.n_rows());
    states.set_excluded([2], true);
    let spec =
        DistanceSpec { columns: (0..t.n_cols()).collect(), metric: DistanceMetric::Euclidean, standardize: false };
    let (d, rows) = distance_table(&t, &states, &spec, None, "d").unwrap();
    assert_eq!(rows.len(), t.n_rows() - 1);
    assert!(!rows.contains(&2));
    assert_eq!(d.n_rows(), t.n_rows() - 1);
    // The matrix table feeds principal coordinates as a matrix.
    let cols: Vec<usize> = (1..d.n_cols()).collect();
    let m = matrix_from_columns(&d, &RowStateManager::new(d.n_rows()), &cols).unwrap();
    assert_eq!(m.matrix.n(), rows.len());
}

/// Principal coordinates of Bray–Curtis distances match R's `cmdscale` and
/// `ape::pcoa`, with and without corrections.
#[test]
fn pcoa_matches_reference() {
    let t = counts_table();
    let states = RowStateManager::new(t.n_rows());
    let r = reference();
    let refs = r.get("pcoa_braycurtis").unwrap();
    let spec =
        DistanceSpec { columns: (0..t.n_cols()).collect(), metric: DistanceMetric::BrayCurtis, standardize: false };
    for correction in [NegativeCorrection::None, NegativeCorrection::Lingoes, NegativeCorrection::Cailliez] {
        let request = PcoaRequest { input: PcoaInput::Variables(spec.clone()), correction, color: None };
        let a = PcoaAnalysis::run(&t, &states, &request).unwrap();
        let (want, constant) = match correction {
            NegativeCorrection::None => (numbers(refs.get("eigenvalues").unwrap()), None),
            c => {
                let block = refs.get(c.key()).unwrap();
                (numbers(block.get("eigenvalues").unwrap()), block.get("constant").and_then(JsonValue::as_f64))
            }
        };
        let got = &a.coordinates().eigenvalues;
        let scale = want[0];
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() <= 1e-9 * scale, "{correction:?}: {g} vs {w}");
        }
        if let Some(c) = constant {
            assert!(close(a.coordinates().correction_constant.unwrap(), c, 1e-9));
        }
        let kinds: Vec<SectionKind> = a.sections().iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::PcoaSummary,
                SectionKind::PcoaEigenvalues,
                SectionKind::PcoaPlot,
                SectionKind::PcoaCorrelations
            ]
        );
        assert!(!a.saveable_columns().is_empty());
    }
}

/// The shrub sample mixes numeric, ordinal and nominal columns with
/// missing values: Gower handles them, Euclidean refuses the text columns.
#[test]
fn gower_on_mixed_columns() {
    let t = crate::sample::shrub_traits();
    let states = RowStateManager::new(t.n_rows());
    let columns: Vec<usize> = (1..t.n_cols()).collect();
    let spec = DistanceSpec { columns: columns.clone(), metric: DistanceMetric::Gower, standardize: false };
    let m = measure(&t, &states, &spec).unwrap();
    assert_eq!(m.matrix.n(), 16);
    for i in 0..16 {
        for j in 0..16 {
            let d = m.matrix.get(i, j);
            assert!((0.0..=1.0).contains(&d));
            assert!((d - m.matrix.get(j, i)).abs() < 1e-15);
        }
    }
    // Shrubs 1 and 2: leaf |15.29 − 13.58| / range, seed |0.726 − 0.27| / range,
    // shade ranks 2 and 4 of 1–4, growth form missing for shrub 1, thorns equal.
    let leaf_range = 16.72 - 2.93;
    let seed_range = 3.017 - 0.243;
    let want = ((15.29 - 13.58) / leaf_range + (0.726 - 0.27) / seed_range + 2.0 / 3.0 + 0.0) / 4.0;
    assert!(close(m.matrix.get(0, 1), want, 1e-12), "{} vs {want}", m.matrix.get(0, 1));
    let euclid = DistanceSpec { metric: DistanceMetric::Euclidean, ..spec };
    assert!(measure(&t, &states, &euclid).is_err());
}

#[test]
fn json_round_trip() {
    let t = crate::sample::shrub_traits();
    let mut request = PcoaRequest::new(vec![1, 2, 3, 4, 5]);
    request.color = Some(4);
    request.correction = NegativeCorrection::Cailliez;
    let matrix = PcoaRequest { input: PcoaInput::Matrix(vec![1, 2, 3]), ..PcoaRequest::new(Vec::new()) };
    let standardized = PcoaRequest {
        input: PcoaInput::Variables(DistanceSpec {
            columns: vec![1, 2],
            metric: DistanceMetric::Manhattan,
            standardize: true,
        }),
        ..PcoaRequest::new(Vec::new())
    };
    for request in [request, matrix, standardized] {
        let saved = crate::SavedReport::PrincipalCoordinates { request };
        let json = crate::project::report_to_json(&saved, &t);
        assert_eq!(crate::project::report_from_json(&json, &t).unwrap(), saved);
    }
}

/// Hierarchical clustering on Gower distances and on a stored matrix agree;
/// k-means refuses other distances.
#[test]
fn clustering_on_distances() {
    use crate::multivariate::{ClusterAnalysis, ClusterDistance, ClusterMethod, ClusterRequest};
    let t = crate::sample::shrub_traits();
    let states = RowStateManager::new(t.n_rows());
    let columns: Vec<usize> = (1..t.n_cols()).collect();
    let method = ClusterMethod::Hierarchical { linkage: stat_multivariate::Linkage::Average, k: 3 };
    let gower = ClusterRequest {
        columns: columns.clone(),
        method,
        standardize: false,
        choose: None,
        centers: None,
        distance: ClusterDistance::Metric(DistanceMetric::Gower),
    };
    let a = ClusterAnalysis::run(&t, &states, &gower).unwrap();
    // The same distances as a table operation, clustered as a matrix.
    let spec = DistanceSpec { columns, metric: DistanceMetric::Gower, standardize: false };
    let (d, _) = distance_table(&t, &states, &spec, Some(0), "d").unwrap();
    assert_eq!(d.column(0).unwrap().level_text(0), "Shrub 01");
    let matrix = ClusterRequest {
        columns: (1..d.n_cols()).collect(),
        method,
        standardize: false,
        choose: None,
        centers: None,
        distance: ClusterDistance::Matrix,
    };
    let b = ClusterAnalysis::run(&d, &RowStateManager::new(d.n_rows()), &matrix).unwrap();
    assert_eq!(a.cluster_column(), b.cluster_column());
    let kmeans = ClusterRequest { method: ClusterMethod::KMeans { k: 3, seed: 1 }, ..gower };
    assert!(ClusterAnalysis::run(&t, &states, &kmeans).is_err());
}
