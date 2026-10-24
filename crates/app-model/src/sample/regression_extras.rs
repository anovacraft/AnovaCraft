//! Samples for probit (dose–response) analysis, method comparison and
//! shelf-life estimation.

use super::{Sample, col, table, text};
use crate::project::SavedReport;
use crate::{DoseResponseRequest, MethodComparisonRequest, ShelfLifeRequest};
use stat_data::{Column, DataTable, MeasurementLevel};

/// Martin's (1942) rotenone sprays on the chrysanthemum aphid
/// *Macrosiphoniella sanborni*, the worked example of Finney's *Probit
/// Analysis* (3rd ed., 1971): concentration of rotenone (mg/l), aphids
/// sprayed and aphids killed, with the unsprayed control (0 of 49 killed).
#[must_use]
pub fn rotenone() -> DataTable {
    table(
        "Rotenone (Martin 1942)",
        vec![
            Column::numeric("Rotenone (mg/l)", vec![10.2, 7.7, 5.1, 3.8, 2.6, 0.0]),
            Column::numeric("Aphids", vec![50.0, 49.0, 46.0, 48.0, 50.0, 49.0]),
            Column::numeric("Killed", vec![44.0, 42.0, 24.0, 16.0, 6.0, 0.0]),
        ],
    )
}

pub(super) fn rotenone_sample() -> Sample {
    let t = rotenone();
    let request = DoseResponseRequest::probit(col(&t, "Rotenone (mg/l)"), col(&t, "Killed"), Some(col(&t, "Aphids")));
    Sample { reports: vec![SavedReport::DoseResponse { request }], table: t }
}

/// **Simulated** glucose (mg/dL) of 40 specimens by a laboratory analyzer
/// and a point-of-care meter: true values uniform on 60–380, the analyzer
/// adding N(0, 4²) and the meter reading `3 + 1.04 × true + N(0, 5²)`, to
/// 0.1 mg/dL; specimen 18's meter reading is 45 mg/dL too high (a gross
/// error). Drawn with Python's `random.seed(20261015)`.
#[must_use]
pub fn glucose_methods() -> DataTable {
    let analyzer = vec![
        73.4, 76.0, 71.3, 85.0, 97.2, 118.7, 128.8, 133.3, 140.5, 143.9, 184.7, 183.2, 180.3, 201.3, 195.5, 211.9,
        218.6, 220.0, 224.3, 231.7, 255.4, 267.5, 260.8, 270.8, 268.7, 272.1, 312.4, 312.7, 321.2, 327.6, 318.8, 333.7,
        332.3, 350.7, 352.9, 347.9, 350.4, 370.1, 386.7, 372.2,
    ];
    let meter = vec![
        87.5, 81.7, 83.1, 86.7, 99.4, 129.2, 134.5, 145.5, 151.0, 158.6, 181.0, 196.7, 199.5, 205.0, 209.0, 225.3,
        234.8, 264.9, 242.6, 249.6, 266.9, 279.7, 282.7, 280.8, 289.3, 281.2, 323.1, 328.2, 336.7, 342.2, 334.8, 353.0,
        340.7, 356.9, 376.8, 375.4, 379.9, 390.6, 398.7, 387.4,
    ];
    let mut specimen = Column::numeric("Specimen", (1..=40).map(f64::from).collect());
    specimen.level = MeasurementLevel::Nominal;
    table(
        "Glucose methods (simulated)",
        vec![specimen, Column::numeric("Analyzer (mg/dL)", analyzer), Column::numeric("Meter (mg/dL)", meter)],
    )
}

pub(super) fn glucose_methods_sample() -> Sample {
    let t = glucose_methods();
    let request = MethodComparisonRequest::new(col(&t, "Analyzer (mg/dL)"), col(&t, "Meter (mg/dL)"));
    Sample { reports: vec![SavedReport::MethodComparison { request }], table: t }
}

/// **Simulated** tablet assay (% of label claim, to 0.1) of three batches at
/// 0, 3, 6, 9, 12, 18 and 24 months: batch A `100.8 − 0.115t`, B
/// `101.6 − 0.120t`, C `100.2 − 0.112t`, each plus N(0, 0.35²). Drawn with
/// Python's `random.seed(7)`.
#[must_use]
pub fn tablet_stability() -> DataTable {
    let months = [0.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0];
    let assay = vec![
        100.7, 100.6, 100.0, 99.7, 99.1, 98.7, 98.4, 101.7, 101.6, 101.0, 100.7, 100.2, 98.9, 99.0, 100.4, 100.0, 98.9,
        98.6, 98.5, 98.0, 97.6,
    ];
    let batch: Vec<String> = ["A", "B", "C"].iter().flat_map(|b| std::iter::repeat_n((*b).to_owned(), 7)).collect();
    table(
        "Tablet stability (simulated)",
        vec![
            text("Batch", batch),
            Column::numeric("Month", (0..3).flat_map(|_| months).collect()),
            Column::numeric("Assay (% label claim)", assay),
        ],
    )
}

pub(super) fn tablet_stability_sample() -> Sample {
    let t = tablet_stability();
    let request =
        ShelfLifeRequest::lower(col(&t, "Month"), col(&t, "Assay (% label claim)"), Some(col(&t, "Batch")), 95.0);
    Sample { reports: vec![SavedReport::ShelfLife { request }], table: t }
}
