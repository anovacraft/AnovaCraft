//! Constant-amplitude fatigue tests with run-outs for the S–N curve report.

use super::{Sample, col, table};
use crate::FatigueRequest;
use crate::project::SavedReport;
use stat_data::{Column, DataTable};
use stat_reliability::fatigue::SnForm;

/// Stress amplitude (MPa), cycles and whether the specimen ran out.
#[rustfmt::skip]
const SPECIMENS: [(f64, f64, bool); 32] = [
    (200.0, 8_990_000.0, false), (200.0, 10_000_000.0, true), (200.0, 7_950_000.0, false), (200.0, 6_020_000.0, false),
    (200.0, 7_330_000.0, false), (200.0, 5_750_000.0, false), (200.0, 9_240_000.0, false), (200.0, 10_000_000.0, true),
    (250.0, 992_000.0, false), (250.0, 937_000.0, false), (250.0, 1_540_000.0, false), (250.0, 1_450_000.0, false),
    (250.0, 1_300_000.0, false), (250.0, 815_000.0, false),
    (300.0, 383_000.0, false), (300.0, 531_000.0, false), (300.0, 212_000.0, false), (300.0, 316_000.0, false),
    (300.0, 165_000.0, false), (300.0, 217_000.0, false),
    (350.0, 74_500.0, false), (350.0, 153_000.0, false), (350.0, 96_400.0, false), (350.0, 193_000.0, false),
    (350.0, 183_000.0, false), (350.0, 157_000.0, false),
    (400.0, 29_000.0, false), (400.0, 70_700.0, false), (400.0, 88_200.0, false), (400.0, 94_800.0, false),
    (400.0, 45_300.0, false), (400.0, 72_700.0, false),
];

/// **Invented** rotating-bending fatigue tests of 32 steel specimens at
/// five stress amplitudes (200–400 MPa); two specimens at 200 MPa were
/// stopped unbroken at 10⁷ cycles (run-outs). The same data are the
/// reference case of `stat-reliability`'s fatigue tests.
#[must_use]
pub fn steel_fatigue() -> DataTable {
    table(
        "Steel fatigue",
        vec![
            Column::text("Specimen", (1..=SPECIMENS.len()).map(|i| format!("F{i:02}")).collect()),
            Column::numeric("Stress (MPa)", SPECIMENS.iter().map(|s| s.0).collect()),
            Column::numeric("Cycles", SPECIMENS.iter().map(|s| s.1).collect()),
            Column::numeric("Run-out", SPECIMENS.iter().map(|s| f64::from(u8::from(s.2))).collect()),
        ],
    )
}

/// The fatigue sample: a lognormal Basquin curve and an endurance-limit curve.
pub(super) fn steel_fatigue_sample() -> Sample {
    let t = steel_fatigue();
    let basquin = FatigueRequest::new(col(&t, "Cycles"), Some(col(&t, "Run-out")), col(&t, "Stress (MPa)"));
    let endurance = FatigueRequest { form: SnForm::EnduranceLimit, ..basquin.clone() };
    Sample {
        table: t,
        reports: vec![SavedReport::Fatigue { request: basquin }, SavedReport::Fatigue { request: endurance }],
    }
}
