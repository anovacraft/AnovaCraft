//! Samples for many two-group tests, for adjusting p-values and for the
//! area under a curve.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{AreaRequest, ManyTestsRequest, PValueRequest};
use stat_data::{Column, DataTable};

/// Number of genes in [`gene_screen`].
pub const SCREEN_GENES: usize = 30;

/// **Simulated** expression (log₂ units) of 30 genes in 16 tissue samples,
/// 8 untreated and 8 treated. Each gene has its own baseline (8 + N(0, 1.5²))
/// and noise N(0, 0.5²); genes G01–G06 rise by 0.6–1.6 under treatment and
/// G07 falls by 1.0; the other 23 do not change. Drawn with the workspace's
/// seeded generator (seed `0x5C2E`), rounded to 0.01.
#[must_use]
pub fn gene_screen() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0x5C2E);
    let shifts = [1.6, 1.4, 1.2, 1.0, 0.8, 0.6, -1.0];
    let mut columns = vec![
        Column::text("Sample", (1..=16).map(|i| format!("T{i:02}")).collect()),
        Column::text("Treatment", (0..16).map(|i| if i < 8 { "Untreated" } else { "Treated" }.to_owned()).collect()),
    ];
    for g in 0..SCREEN_GENES {
        let base = 8.0 + 1.5 * rng.normal();
        let shift = shifts.get(g).copied().unwrap_or(0.0);
        let values = (0..16)
            .map(|i| {
                let y = base + if i < 8 { 0.0 } else { shift } + 0.5 * rng.normal();
                (y * 100.0).round() / 100.0
            })
            .collect();
        columns.push(Column::numeric(format!("G{:02}", g + 1), values));
    }
    table("Gene screen (simulated)", columns)
}

pub(super) fn gene_screen_sample() -> Sample {
    let t = gene_screen();
    let genes: Vec<usize> = (0..SCREEN_GENES).map(|g| col(&t, &format!("G{:02}", g + 1))).collect();
    let reports = vec![SavedReport::ManyTests {
        request: ManyTestsRequest {
            order: crate::ManyTestsOrder::PValue,
            ..ManyTestsRequest::new(genes, col(&t, "Treatment"))
        },
    }];
    Sample { table: t, reports }
}

/// **Invented** p-values of twelve secondary endpoints of a clinical trial,
/// each from its own two-sided test, with the number of patients measured
/// on each endpoint (for a weighted combined test).
#[must_use]
pub fn trial_endpoints() -> DataTable {
    let names = [
        "Systolic pressure",
        "Diastolic pressure",
        "Heart rate",
        "LDL cholesterol",
        "HDL cholesterol",
        "Triglycerides",
        "Fasting glucose",
        "HbA1c",
        "Body weight",
        "Waist circumference",
        "CRP",
        "Quality of life score",
    ];
    table(
        "Trial endpoints (invented)",
        vec![
            Column::text("Endpoint", names.iter().map(|&n| n.to_owned()).collect()),
            Column::numeric(
                "p-value",
                vec![0.0021, 0.0130, 0.4410, 0.0008, 0.2730, 0.0390, 0.0460, 0.0115, 0.0870, 0.0310, 0.6220, 0.1480],
            ),
            Column::numeric(
                "Patients",
                vec![412.0, 412.0, 409.0, 398.0, 398.0, 398.0, 401.0, 377.0, 415.0, 380.0, 366.0, 352.0],
            ),
        ],
    )
}

pub(super) fn trial_endpoints_sample() -> Sample {
    let t = trial_endpoints();
    let reports = vec![SavedReport::AdjustPValues {
        request: PValueRequest {
            label: Some(col(&t, "Endpoint")),
            weight: Some(col(&t, "Patients")),
            ..PValueRequest::new(col(&t, "p-value"))
        },
    }];
    Sample { table: t, reports }
}

/// **Simulated** plasma concentration (mg/L) of a drug after one oral dose
/// of two formulations, A and B. Destructive sampling: three animals at each
/// of nine times (0–24 h), each animal measured once. The mean follows a
/// one-compartment curve `C (e^{−0.2 t} − e^{−k t})` (A: `C = 12`, `k = 1.2`;
/// B: `C = 10`, `k = 0.7`) with multiplicative noise of about 12%, drawn with
/// the workspace's seeded generator (seed `0xA0C`), rounded to 0.01.
#[must_use]
pub fn plasma_concentration() -> DataTable {
    let mut rng = stat_core::rng::Rng::new(0xA0C);
    let times: [f64; 9] = [0.0, 0.5, 1.0, 2.0, 4.0, 6.0, 8.0, 12.0, 24.0];
    let (mut form, mut time, mut conc, mut animal) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (f, c, k) in [("A", 12.0_f64, 1.2_f64), ("B", 10.0, 0.7)] {
        for &t in &times {
            for _ in 0..3 {
                let mean: f64 = c * ((-0.2 * t).exp() - (-k * t).exp());
                let y = (mean * (0.12 * rng.normal()).exp() * 100.0).round() / 100.0;
                animal.push(format!("{f}{:02}", animal.len() % 27 + 1));
                form.push(f.to_owned());
                time.push(t);
                conc.push(y);
            }
        }
    }
    table(
        "Plasma concentration (simulated)",
        vec![
            Column::text("Animal", animal),
            Column::text("Formulation", form),
            Column::numeric("Time (h)", time),
            Column::numeric("Concentration (mg/L)", conc),
        ],
    )
}

pub(super) fn plasma_concentration_sample() -> Sample {
    let t = plasma_concentration();
    let reports = vec![SavedReport::AreaUnderCurve {
        request: AreaRequest {
            group: Some(col(&t, "Formulation")),
            ..AreaRequest::new(col(&t, "Time (h)"), col(&t, "Concentration (mg/L)"))
        },
    }];
    Sample { table: t, reports }
}
