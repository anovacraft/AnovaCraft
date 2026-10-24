//! Samples for Gaussian mixture clustering and dimension reduction.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{EmbeddingMethod, EmbeddingRequest, GaussianMixtureRequest};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel};

fn round_to(x: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (x * f).round() / f
}

/// Population name, size, CD4 and CD8 means, the lower Cholesky factor of
/// their covariance (`l11`, `l21`, `l22`), and the size mean.
/// One population: name, size, means, Cholesky factor, size mean.
type Population = (&'static str, usize, [f64; 2], [f64; 3], f64);

const POPULATIONS: [Population; 3] = [
    // Covariance [[0.64, 0.48], [0.48, 0.52]]: elongated along the diagonal.
    ("Helper T", 180, [6.0, 1.8], [0.8, 0.6, 0.4], 52.0),
    // Covariance [[0.36, −0.30], [−0.30, 1.25]]: tall and tilted the other way.
    ("Cytotoxic T", 120, [1.6, 6.0], [0.6, -0.5, 1.0], 60.0),
    // Covariance 0.09 I: small and round.
    ("Double negative", 60, [1.0, 1.0], [0.3, 0.0, 0.3], 44.0),
];

/// **Simulated** flow-cytometry-like measurements of 360 cells from three
/// populations of unequal size and shape: CD4 and CD8 marker intensities
/// (log scale) drawn from bivariate normals with the means and covariance
/// factors in `POPULATIONS` (one elongated with positive correlation, one
/// tall with negative correlation, one small and round), plus a cell size
/// N(mean, 5²). Drawn with the SplitMix64 generator seeded 0xCE11; values
/// rounded to 0.001 (size to 0.1). The true population is kept in a
/// column to compare with the clusters found.
#[must_use]
pub fn cell_populations() -> DataTable {
    let mut rng = Rng::new(0xCE11);
    let (mut pop, mut cd4, mut cd8, mut size) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (name, n, mean, l, s) in POPULATIONS {
        for _ in 0..n {
            let (z1, z2) = (rng.normal(), rng.normal());
            pop.push(name.to_owned());
            cd4.push(round_to(mean[0] + l[0] * z1, 3));
            cd8.push(round_to(mean[1] + l[1] * z1 + l[2] * z2, 3));
            size.push(round_to(s + 5.0 * rng.normal(), 1));
        }
    }
    // Shuffle the rows so populations are interleaved.
    let mut order: Vec<usize> = (0..pop.len()).collect();
    rng.shuffle(&mut order);
    let pick = |v: &[f64]| order.iter().map(|&i| v[i]).collect::<Vec<f64>>();
    let mut cell = Column::numeric("Cell", (1..=pop.len()).map(|i| i as f64).collect());
    cell.level = MeasurementLevel::Nominal;
    table(
        "Cell populations (simulated)",
        vec![
            cell,
            Column::text("Population", order.iter().map(|&i| pop[i].clone()).collect()),
            Column::numeric("CD4 marker", pick(&cd4)),
            Column::numeric("CD8 marker", pick(&cd8)),
            Column::numeric("Size (FSC)", pick(&size)),
        ],
    )
}

pub(crate) fn cell_populations_sample() -> Sample {
    let t = cell_populations();
    let markers = vec![col(&t, "CD4 marker"), col(&t, "CD8 marker")];
    let reports = vec![
        SavedReport::GaussianMixture { request: GaussianMixtureRequest::new(markers.clone()) },
        SavedReport::Cluster {
            request: crate::ClusterRequest {
                distance: Default::default(),
                columns: markers,
                method: crate::ClusterMethod::KMeans { k: 3, seed: 1 },
                standardize: false,
                choose: None,
                centers: None,
            },
        },
    ];
    Sample { table: t, reports }
}

/// Number of genes in [`expression_profiles`].
pub const GENES: usize = 20;

/// **Simulated** expression of 20 genes in 300 cells of four types (75
/// each). Cell type `k` raises genes `5k + 1 … 5k + 5` by 2.5 (two of its
/// genes by 1.5 more), every cell has a "cycle" score `u ~ U(0, 1)` adding
/// `1.5u` to genes 1, 8 and 15, and each value has N(0, 0.6²) noise; rounded
/// to 0.001. Drawn with the SplitMix64 generator seeded 0x6E7E. Four groups
/// in twenty dimensions, with a continuous gradient across them — the kind
/// of data t-SNE and UMAP are used to map.
#[must_use]
pub fn expression_profiles() -> DataTable {
    let mut rng = Rng::new(0x6E7E);
    let types = ["Basal", "Luminal", "Stromal", "Immune"];
    let mut kind = Vec::new();
    let mut genes: Vec<Vec<f64>> = vec![Vec::new(); GENES];
    for i in 0..300 {
        let k = i % 4;
        kind.push(types[k].to_owned());
        let u = rng.next_f64();
        for (g, values) in genes.iter_mut().enumerate() {
            let mut m = if g / 5 == k { 2.5 } else { 0.0 };
            if g / 5 == k && g % 5 < 2 {
                m += 1.5;
            }
            if matches!(g, 0 | 7 | 14) {
                m += 1.5 * u;
            }
            values.push(round_to(m + 0.6 * rng.normal(), 3));
        }
    }
    let mut cell = Column::numeric("Cell", (1..=300).map(f64::from).collect());
    cell.level = MeasurementLevel::Nominal;
    let mut columns = vec![cell, Column::text("Cell type", kind)];
    for (g, values) in genes.into_iter().enumerate() {
        columns.push(Column::numeric(format!("Gene {:02}", g + 1), values));
    }
    table("Expression profiles (simulated)", columns)
}

pub(crate) fn expression_profiles_sample() -> Sample {
    let t = expression_profiles();
    let genes: Vec<usize> = (1..=GENES).map(|g| col(&t, &format!("Gene {g:02}"))).collect();
    let color = Some(col(&t, "Cell type"));
    let reports = [EmbeddingMethod::Tsne, EmbeddingMethod::Umap, EmbeddingMethod::ClassicalScaling]
        .into_iter()
        .map(|m| SavedReport::Embedding {
            request: EmbeddingRequest { color, ..EmbeddingRequest::new(m, genes.clone()) },
        })
        .collect();
    Sample { table: t, reports }
}

/// Town names and invented map positions (km) of [`town_distances`].
const TOWNS: [(&str, f64, f64); 12] = [
    ("Ashford", 0.0, 0.0),
    ("Brookley", 34.0, 12.0),
    ("Calder", 61.0, -8.0),
    ("Dunmore", 18.0, 47.0),
    ("Elstow", 52.0, 39.0),
    ("Fernby", 88.0, 25.0),
    ("Glenrith", -22.0, 30.0),
    ("Holloway", 40.0, 80.0),
    ("Ingleton", 75.0, 66.0),
    ("Jarrow Cross", -10.0, -35.0),
    ("Kestle", 25.0, -40.0),
    ("Larkhill", 100.0, -20.0),
];

/// **Invented** road distances (km) between 12 fictional towns: the
/// straight-line distance between invented map positions times a winding
/// factor 1.15–1.35 per pair (seeded 0x70AD), rounded to whole km, so the
/// matrix is symmetric but not exactly Euclidean. One column per town; row
/// `i` is the same town as column `i`.
#[must_use]
pub fn town_distances() -> DataTable {
    let mut rng = Rng::new(0x70AD);
    let n = TOWNS.len();
    let mut d = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..i {
            let (dx, dy) = (TOWNS[i].1 - TOWNS[j].1, TOWNS[i].2 - TOWNS[j].2);
            let v = ((dx * dx + dy * dy).sqrt() * (1.15 + 0.2 * rng.next_f64())).round();
            d[i][j] = v;
            d[j][i] = v;
        }
    }
    let mut columns = vec![Column::text("Town", TOWNS.iter().map(|t| t.0.to_owned()).collect())];
    for (j, town) in TOWNS.iter().enumerate() {
        columns.push(Column::numeric(town.0, (0..n).map(|i| d[i][j]).collect()));
    }
    table("Town distances (invented)", columns)
}

pub(crate) fn town_distances_sample() -> Sample {
    let t = town_distances();
    let towns: Vec<usize> = TOWNS.iter().map(|town| col(&t, town.0)).collect();
    let mut reports: Vec<SavedReport> = [EmbeddingMethod::ClassicalScaling, EmbeddingMethod::MetricScaling]
        .into_iter()
        .map(|m| SavedReport::Embedding {
            request: EmbeddingRequest { distances: true, ..EmbeddingRequest::new(m, towns.clone()) },
        })
        .collect();
    // Road distances are not Euclidean: principal coordinates show the
    // negative eigenvalues that classical scaling drops.
    reports.push(SavedReport::PrincipalCoordinates {
        request: crate::PcoaRequest { input: crate::PcoaInput::Matrix(towns), ..crate::PcoaRequest::new(Vec::new()) },
    });
    Sample { table: t, reports }
}
