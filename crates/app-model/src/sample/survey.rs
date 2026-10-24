//! Sample for the 3-D scatter plot and the contour and surface plots of data.

use super::{Sample, col, table, text};
use crate::project::SavedReport;
use crate::{Camera, DataSurfaceOptions, DataSurfaceRequest, Scatter3dOptions, Scatter3dRequest, SurfaceFit};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};

/// An **invented** survey of a 400 m × 300 m hillside: 140 points at
/// irregular positions (whole metres, drawn with the workspace's
/// integer-based generator, seed 316) with the ground elevation, the soil
/// depth and the soil type. The ground is a rounded hill with a saddle
/// towards a lower knoll in the south-east, on a slope falling to the west;
/// soil is deeper on low ground, and its type follows elevation and
/// position. Values are rounded (elevation to 0.1 m, depth to 1 cm), so the
/// table is the same on every platform.
#[must_use]
pub fn hillside_survey() -> DataTable {
    let mut rng = Rng::new(316);
    let n = 140;
    let (mut east, mut north, mut elevation, mut depth, mut soil) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let e = rng.below(401) as f64;
        let nn = rng.below(301) as f64;
        let hill = 38.0 * (-((e - 150.0).powi(2) + (nn - 190.0).powi(2)) / (2.0 * 85.0 * 85.0)).exp();
        let knoll = 17.0 * (-((e - 320.0).powi(2) + (nn - 80.0).powi(2)) / (2.0 * 55.0 * 55.0)).exp();
        let slope = 0.035 * e;
        let bumps = 1.2 * (e / 37.0).sin() * (nn / 29.0).cos();
        let z = 112.0 + hill + knoll + slope + bumps;
        let z = (z * 10.0).round() / 10.0;
        let d = 165.0 - 2.2 * (z - 112.0) + (rng.below(21) as f64 - 10.0);
        east.push(e);
        north.push(nn);
        elevation.push(z);
        depth.push(d.round().max(10.0));
        soil.push(
            if z > 140.0 {
                "Sandy loam"
            } else if e > 260.0 && nn < 150.0 {
                "Silt loam"
            } else if z < 122.0 {
                "Clay"
            } else {
                "Loam"
            }
            .to_owned(),
        );
    }
    table(
        "Hillside survey (invented)",
        vec![
            Column::numeric("Easting (m)", east),
            Column::numeric("Northing (m)", north),
            Column::numeric("Elevation (m)", elevation),
            Column::numeric("Soil depth (cm)", depth),
            text("Soil type", soil),
        ],
    )
}

pub(super) fn hillside_survey_sample() -> Sample {
    let t = hillside_survey();
    let (e, n, z) = (col(&t, "Easting (m)"), col(&t, "Northing (m)"), col(&t, "Elevation (m)"));
    let scatter = Scatter3dRequest {
        color: Some(col(&t, "Soil type")),
        size: Some(col(&t, "Soil depth (cm)")),
        ..Scatter3dRequest::new(e, n, z)
    };
    let depth = DataSurfaceRequest {
        method: SurfaceFit::SmoothingSpline,
        grid: 30,
        ..DataSurfaceRequest::new(e, n, col(&t, "Soil depth (cm)"))
    };
    Sample {
        reports: vec![
            SavedReport::DataSurface {
                request: DataSurfaceRequest::new(e, n, z),
                options: DataSurfaceOptions { levels: 12, ..DataSurfaceOptions::default() },
            },
            SavedReport::Scatter3d {
                request: scatter,
                options: Scatter3dOptions { drop_lines: true, camera: Camera { azimuth: -30.0, elevation: 25.0 } },
            },
            SavedReport::DataSurface {
                request: depth,
                options: DataSurfaceOptions { surface: false, ..DataSurfaceOptions::default() },
            },
        ],
        table: t,
    }
}
