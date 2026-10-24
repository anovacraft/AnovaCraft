//! Sample for nonlinear regression.

use super::{Sample, col, table};
use crate::NonlinearRequest;
use crate::project::SavedReport;
use stat_data::{Column, DataTable};
use stat_linear::CurveForm;

/// **Simulated** heights (cm) of two seedlings measured every three days for
/// 45 days, from the logistic curve `48 / (1 + exp(−0.18·(Day − 20)))` with
/// normal errors whose standard deviation grows with the height
/// (`0.3 + 0.04 × mean`), rounded to 0.1 cm. Drawn with NumPy's
/// `default_rng(20261016)`.
#[must_use]
pub fn seedling_growth() -> DataTable {
    let days: Vec<f64> = (0..32).map(|i| f64::from(i / 2 * 3)).collect();
    let plant = (0..32).map(|i| if i % 2 == 0 { "A" } else { "B" }.to_owned()).collect();
    table(
        "Seedling growth (simulated)",
        vec![
            Column::numeric("Day", days),
            Column::text("Seedling", plant),
            Column::numeric(
                "Height (cm)",
                vec![
                    0.8, 1.6, 2.2, 1.4, 3.0, 3.5, 5.4, 5.3, 8.6, 8.3, 13.1, 15.8, 19.9, 19.3, 24.9, 24.2, 27.7, 31.8,
                    36.4, 41.3, 41.3, 39.3, 42.0, 47.7, 44.1, 45.2, 45.8, 47.6, 46.4, 48.7, 45.1, 49.5,
                ],
            ),
        ],
    )
}

pub(super) fn seedling_growth_sample() -> Sample {
    let t = seedling_growth();
    let request = NonlinearRequest::library(col(&t, "Height (cm)"), col(&t, "Day"), CurveForm::Logistic3);
    Sample {
        reports: vec![SavedReport::Nonlinear { request, profilers: crate::NonlinearAnalysis::default_profilers() }],
        table: t,
    }
}
