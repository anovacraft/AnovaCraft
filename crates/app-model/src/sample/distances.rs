//! Mixed-type traits for distance matrices and principal coordinates.

use super::{Sample, col, table};
use crate::PcoaRequest;
use crate::multivariate::{ClusterDistance, ClusterMethod, ClusterRequest};
use crate::project::SavedReport;
use stat_data::{Column, DataTable, MeasurementLevel};
use stat_multivariate::Linkage;
use stat_multivariate::distance::DistanceMetric;

/// Leaf length (cm), seed mass (g), shade tolerance (1–4), growth form and
/// thorns of one shrub; `None` is a missing value.
type Shrub = (Option<f64>, Option<f64>, Option<f64>, Option<&'static str>, Option<&'static str>);

#[rustfmt::skip]
const SHRUBS: [Shrub; 16] = [
    (Some(15.29), Some(0.726), Some(2.0), None, Some("No")),
    (Some(13.58), Some(0.27), Some(4.0), Some("Spreading"), Some("No")),
    (None, Some(2.113), Some(1.0), Some("Upright"), Some("No")),
    (Some(11.37), Some(3.017), Some(4.0), Some("Upright"), Some("No")),
    (Some(16.72), Some(0.965), Some(1.0), Some("Climbing"), Some("Yes")),
    (Some(11.76), None, Some(3.0), Some("Spreading"), Some("Yes")),
    (Some(9.13), Some(0.463), Some(3.0), Some("Spreading"), Some("No")),
    (Some(10.76), Some(1.816), Some(1.0), Some("Upright"), None),
    (Some(8.1), Some(0.325), Some(4.0), Some("Climbing"), Some("No")),
    (None, Some(0.243), Some(2.0), Some("Spreading"), Some("No")),
    (Some(11.03), Some(0.5), Some(1.0), Some("Upright"), Some("Yes")),
    (Some(10.98), Some(2.431), None, Some("Climbing"), Some("No")),
    (Some(9.52), Some(0.892), Some(2.0), Some("Climbing"), Some("No")),
    (Some(2.93), Some(0.761), Some(1.0), Some("Spreading"), Some("Yes")),
    (Some(12.32), Some(0.807), Some(1.0), Some("Upright"), Some("No")),
    (Some(7.36), Some(2.889), Some(4.0), Some("Climbing"), Some("Yes")),
];

/// **Invented** traits of 16 shrubs of mixed type: two measurements, an
/// ordinal shade tolerance (1 = full sun … 4 = deep shade), a nominal
/// growth form and whether the stems carry thorns, with a few values
/// missing. The numbers are the mixed-data case of `stat-multivariate`'s
/// distance reference tests.
#[must_use]
pub fn shrub_traits() -> DataTable {
    let text = |o: Option<&str>| o.unwrap_or_default().to_owned();
    let num = |o: Option<f64>| o.unwrap_or(f64::NAN);
    let mut t = table(
        "Shrub traits",
        vec![
            Column::text("Shrub", (1..=SHRUBS.len()).map(|i| format!("Shrub {i:02}")).collect()),
            Column::numeric("Leaf length (cm)", SHRUBS.iter().map(|s| num(s.0)).collect()),
            Column::numeric("Seed mass (g)", SHRUBS.iter().map(|s| num(s.1)).collect()),
            Column::numeric("Shade tolerance", SHRUBS.iter().map(|s| num(s.2)).collect()),
            Column::text("Growth form", SHRUBS.iter().map(|s| text(s.3)).collect()),
            Column::text("Thorns", SHRUBS.iter().map(|s| text(s.4)).collect()),
        ],
    );
    t.set_level(col(&t, "Shade tolerance"), MeasurementLevel::Ordinal).expect("numeric columns can be ordinal");
    t
}

/// The shrub sample: principal coordinates of Gower distances, coloured by
/// growth form, and average-linkage clustering of the same distances.
pub(super) fn shrub_traits_sample() -> Sample {
    let t = shrub_traits();
    let traits: Vec<usize> = ["Leaf length (cm)", "Seed mass (g)", "Shade tolerance", "Growth form", "Thorns"]
        .iter()
        .map(|c| col(&t, c))
        .collect();
    let pcoa = PcoaRequest { color: Some(col(&t, "Growth form")), ..PcoaRequest::new(traits.clone()) };
    let cluster = ClusterRequest {
        columns: traits,
        method: ClusterMethod::Hierarchical { linkage: Linkage::Average, k: 3 },
        standardize: false,
        choose: None,
        centers: None,
        distance: ClusterDistance::Metric(DistanceMetric::Gower),
    };
    Sample {
        table: t,
        reports: vec![SavedReport::PrincipalCoordinates { request: pcoa }, SavedReport::Cluster { request: cluster }],
    }
}
