//! A sample of concentrations with non-detects (values below a detection
//! limit) for the censored Distribution report.

use super::{Sample, col, table};
use crate::censored_distribution::{CensorDirection, Censoring};
use crate::project::SavedReport;
use crate::{DistributionOptions, DistributionRequest};
use stat_data::{Column, DataTable};

/// Arsenic in 24 wells (invented): the measured concentration in µg/L or,
/// for a non-detect, the detection limit of its laboratory run (1, 2 or
/// 5 µg/L). `Non-detect` is 1 for a value below its limit.
#[must_use]
pub fn well_arsenic() -> DataTable {
    let value = [
        1.0, 1.0, 3.2, 2.0, 4.8, 1.0, 6.1, 2.4, 2.0, 9.7, 1.6, 5.0, 12.5, 3.9, 2.0, 7.3, 1.2, 5.0, 2.9, 18.4, 1.0, 4.1,
        8.8, 2.2,
    ];
    let nd = [
        1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
        0.0, 0.0,
    ];
    let aquifer = ["Shallow", "Deep"];
    table(
        "Well water arsenic",
        vec![
            Column::text("Well", (1..=24).map(|i| format!("W{i:02}")).collect()),
            Column::text("Aquifer", (0..24).map(|i| aquifer[i % 2].to_owned()).collect()),
            Column::numeric("Arsenic (µg/L)", value.to_vec()),
            Column::numeric("Non-detect", nd.to_vec()),
        ],
    )
}

/// The well water sample: the distribution of arsenic with its non-detects.
pub(super) fn well_arsenic_sample() -> Sample {
    let t = well_arsenic();
    let request = DistributionRequest {
        censoring: Some(Censoring::Indicator { column: col(&t, "Non-detect"), direction: CensorDirection::Left }),
        ..DistributionRequest::new(col(&t, "Arsenic (µg/L)"))
    };
    Sample { table: t, reports: vec![SavedReport::Distribution { request, options: DistributionOptions::default() }] }
}
