//! Sample for bootstrap and permutation tests.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{BootstrapRequest, DistributionOptions, DistributionRequest, PermutationDesign, PermutationRequest};
use stat_data::{Column, DataTable, MeasurementLevel};
use stat_inference::permutation::{CorrelationStatistic, GroupStatistic};

/// **Invented** repair times (hours) of 24 jobs, 12 by each of two crews,
/// with the number of parts replaced: log-normal times (median 2.2 h for
/// crew A and 3.4 h for crew B, ×e^{0.25} per part replaced, log standard
/// deviations 0.55 and 0.6) and Poisson(2) parts, rounded to 0.1 h. Drawn
/// with NumPy's `default_rng(20261016)`. Skewed, with one long repair — the
/// kind of small sample where bootstrap intervals and permutation tests
/// avoid a normal assumption.
#[must_use]
pub fn repair_times() -> DataTable {
    let crew = (0..24).map(|i| if i < 12 { "A" } else { "B" }.to_owned()).collect();
    let parts = vec![
        2.0, 2.0, 1.0, 2.0, 1.0, 1.0, 4.0, 3.0, 3.0, 1.0, 3.0, 3.0, 2.0, 0.0, 3.0, 0.0, 3.0, 0.0, 1.0, 0.0, 1.0, 0.0,
        4.0, 4.0,
    ];
    let mut parts = Column::numeric("Parts replaced", parts);
    parts.level = MeasurementLevel::Ordinal;
    table(
        "Repair times (invented)",
        vec![
            Column::numeric("Job", (1..=24).map(f64::from).collect()),
            Column::text("Crew", crew),
            parts,
            Column::numeric(
                "Repair time (h)",
                vec![
                    2.5, 4.0, 0.9, 3.5, 4.5, 1.7, 3.1, 14.1, 6.7, 2.8, 3.7, 8.4, 8.2, 4.0, 4.8, 2.8, 4.9, 4.5, 1.7,
                    4.6, 5.4, 2.3, 7.7, 9.5,
                ],
            ),
        ],
    )
}

pub(super) fn repair_times_sample() -> Sample {
    let t = repair_times();
    let time = col(&t, "Repair time (h)");
    let distribution = SavedReport::Distribution {
        request: DistributionRequest { column: time, quantile_method: Default::default(), censoring: None },
        options: DistributionOptions::default(),
    };
    let bootstrap = BootstrapRequest {
        rows: vec!["Mean".to_owned(), "Standard deviation".to_owned(), "Coefficient of variation (%)".to_owned()],
        replicates: 2000,
        seed: 2026,
        ..BootstrapRequest::new(distribution.clone(), "Summary Statistics", vec!["Value".to_owned()])
    };
    let by_crew = PermutationRequest::new(PermutationDesign::TwoGroups {
        response: time,
        group: col(&t, "Crew"),
        statistic: GroupStatistic::MedianDifference,
    });
    let with_parts = PermutationRequest::new(PermutationDesign::Correlation {
        x: col(&t, "Parts replaced"),
        y: time,
        statistic: CorrelationStatistic::Spearman,
    });
    Sample {
        reports: vec![
            distribution,
            SavedReport::Bootstrap { request: bootstrap },
            SavedReport::PermutationTest { request: by_crew },
            SavedReport::PermutationTest { request: with_parts },
        ],
        table: t,
    }
}
