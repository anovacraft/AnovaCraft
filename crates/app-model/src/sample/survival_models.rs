//! Samples for proportional hazards and parametric survival regression.

use super::{Sample, col, table, text};
use crate::life_distribution::LifeDataLayout;
use crate::project::SavedReport;
use crate::surface_profiler::ProfilerOptions;
use crate::{ProportionalHazardsRequest, StatusCoding, SurvivalRegressionRequest};
use stat_data::{Column, DataTable};

/// Freireich et al. (1963, *Blood* 21:699): weeks in remission for 42
/// leukemia patients on 6-mercaptopurine or placebo, with each patient's
/// log₁₀ white blood cell count at diagnosis and sex as tabulated by
/// Kleinbaum & Klein (*Survival Analysis: A Self-Learning Text*, 3rd ed.,
/// 2012, the "anderson" data; female = 0, male = 1 there). `Relapse` is 1
/// for a relapse and 0 for a patient still in remission when observation
/// ended.
#[must_use]
pub fn leukemia_covariates() -> DataTable {
    let weeks = [
        35, 34, 32, 32, 25, 23, 22, 20, 19, 17, 16, 13, 11, 10, 10, 9, 7, 6, 6, 6, 6, 23, 22, 17, 15, 12, 12, 11, 11,
        8, 8, 8, 8, 5, 5, 4, 4, 3, 2, 2, 1, 1,
    ];
    let relapse = [
        0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1,
    ];
    let male = [
        1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1,
        1, 1, 1, 1, 1,
    ];
    let log_wbc = [
        1.45, 1.47, 2.2, 2.53, 1.78, 2.57, 2.32, 2.01, 2.05, 2.16, 3.6, 2.88, 2.6, 2.7, 2.96, 2.8, 4.43, 3.2, 2.31,
        4.06, 3.28, 1.97, 2.73, 2.95, 2.3, 1.5, 3.06, 3.49, 2.12, 3.52, 3.05, 2.32, 3.26, 3.49, 3.97, 4.36, 2.42, 4.01,
        4.91, 4.48, 2.8, 5.0,
    ];
    table(
        "Leukemia remission with covariates (Freireich 1963)",
        vec![
            text("Treatment", (0..42).map(|i| if i < 21 { "6-MP" } else { "Placebo" }.to_owned()).collect()),
            Column::numeric("Weeks", weeks.iter().map(|&w| f64::from(w)).collect()),
            Column::numeric("Relapse", relapse.iter().map(|&r| f64::from(r)).collect()),
            Column::numeric("log WBC", log_wbc.to_vec()),
            text("Sex", male.iter().map(|&m| if m == 1 { "Male" } else { "Female" }.to_owned()).collect()),
        ],
    )
}

pub(super) fn leukemia_covariates_sample() -> Sample {
    let t = leukemia_covariates();
    let request = ProportionalHazardsRequest {
        coding: StatusCoding::NonzeroIsEvent,
        curve_by: Some(col(&t, "Treatment")),
        times: vec![10.0, 20.0],
        ..ProportionalHazardsRequest::new(
            col(&t, "Weeks"),
            Some(col(&t, "Relapse")),
            vec![col(&t, "Treatment"), col(&t, "log WBC"), col(&t, "Sex")],
        )
    };
    Sample {
        table: t,
        reports: vec![SavedReport::ProportionalHazards { request, profilers: ProfilerOptions::default() }],
    }
}

/// **Simulated** accelerated life test of electrolytic capacitors: 20 units
/// from each of two lots at 85, 105 and 125 °C, with Weibull lives (shape 2;
/// log characteristic life `8.3 − 0.045·(T − 85) + 0.25·[lot B]` hours),
/// inspected at 168, 336, 504, 672, 1000, 1500 and 2000 hours. Each row
/// counts the units found failed between two inspections (`Last pass`
/// empty: failed before the first one) or still working at 2000 hours
/// (`Found failed` empty). Drawn with NumPy's `default_rng(20261016)`
/// (`crates/stat-reliability/tests/data/survival_regression_reference.py`).
#[must_use]
pub fn capacitor_life_test() -> DataTable {
    let temp = [
        85, 85, 85, 85, 85, 85, 105, 105, 105, 105, 105, 105, 105, 105, 105, 105, 105, 105, 105, 125, 125, 125, 125,
        125, 125, 125, 125, 125, 125, 125, 125,
    ];
    let lot = "AAAABBAAAAAAABBBBBBAAAAAABBBBBB";
    let start = [
        336, 1000, 1500, 2000, 1000, 2000, 168, 336, 504, 672, 1000, 1500, 2000, 168, 336, 672, 1000, 1500, 2000, 0,
        168, 336, 504, 672, 1000, 168, 336, 504, 672, 1000, 1500,
    ];
    let end = [
        504, 1500, 2000, 0, 1500, 0, 336, 504, 672, 1000, 1500, 2000, 0, 336, 504, 1000, 1500, 2000, 0, 168, 336, 504,
        672, 1000, 1500, 336, 504, 672, 1000, 1500, 2000,
    ];
    let units = [1, 1, 2, 16, 2, 18, 4, 1, 4, 1, 4, 2, 4, 1, 1, 2, 7, 5, 4, 1, 2, 4, 6, 5, 2, 5, 2, 1, 7, 4, 1];
    let missing_if_zero = |v: i32| if v == 0 { f64::NAN } else { f64::from(v) };
    table(
        "Capacitor life test (simulated)",
        vec![
            Column::numeric("Temperature (°C)", temp.iter().map(|&t| f64::from(t)).collect()),
            text("Lot", lot.chars().map(|c| c.to_string()).collect()),
            Column::numeric("Last pass (h)", start.iter().map(|&v| missing_if_zero(v)).collect()),
            Column::numeric("Found failed (h)", end.iter().map(|&v| missing_if_zero(v)).collect()),
            Column::numeric("Units", units.iter().map(|&u| f64::from(u)).collect()),
        ],
    )
}

pub(super) fn capacitor_life_test_sample() -> Sample {
    let t = capacitor_life_test();
    let request = SurvivalRegressionRequest {
        frequency: Some(col(&t, "Units")),
        curve_by: Some(col(&t, "Temperature (°C)")),
        probabilities: vec![0.01, 0.1, 0.5],
        times: vec![1000.0, 8760.0],
        ..SurvivalRegressionRequest::new(
            LifeDataLayout::Intervals { start: col(&t, "Last pass (h)"), end: col(&t, "Found failed (h)") },
            vec![col(&t, "Temperature (°C)"), col(&t, "Lot")],
        )
    };
    Sample {
        table: t,
        reports: vec![SavedReport::SurvivalRegression { request, profilers: ProfilerOptions::default() }],
    }
}
