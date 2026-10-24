//! Samples for the ordinal mixed model and for shelf life with random
//! batches.

use super::{Sample, col, table, text};
use crate::project::SavedReport;
use crate::{MixedFamily, MixedRequest, RandomEffectSpec, ShelfLifeRequest};
use stat_data::{Column, DataTable, MeasurementLevel};

/// Randall's (1989) wine bitterness experiment ("The analysis of sensory
/// data by generalized linear model", *Biometrical Journal* 31, 781–793):
/// nine judges each rated eight wines on a five-point bitterness scale (1 =
/// least bitter), the wines made at two temperatures (cold, warm) with or
/// without skin contact, two bottles of each.
#[must_use]
pub fn wine_bitterness() -> DataTable {
    let rating = [
        2, 3, 3, 4, 4, 4, 5, 5, 1, 2, 1, 3, 2, 3, 5, 4, 2, 3, 3, 2, 5, 5, 4, 4, 3, 2, 3, 2, 3, 2, 5, 3, 2, 3, 4, 3, 3,
        3, 3, 3, 3, 2, 3, 2, 2, 4, 5, 4, 1, 1, 2, 2, 2, 3, 2, 3, 2, 2, 2, 3, 3, 3, 3, 4, 1, 2, 3, 2, 3, 2, 4, 4,
    ];
    let mut bitterness = Column::numeric("Bitterness", rating.iter().map(|&r| f64::from(r)).collect());
    bitterness.level = MeasurementLevel::Ordinal;
    let temperature: Vec<String> = (0..72).map(|i| if (i / 4) % 2 == 0 { "cold" } else { "warm" }.to_owned()).collect();
    let contact: Vec<String> = (0..72).map(|i| if (i / 2) % 2 == 0 { "no" } else { "yes" }.to_owned()).collect();
    let mut judge = Column::numeric("Judge", (0..72).map(|i| (i / 8 + 1) as f64).collect());
    judge.level = MeasurementLevel::Nominal;
    table(
        "Wine bitterness (Randall 1989)",
        vec![bitterness, text("Temperature", temperature), text("Skin contact", contact), judge],
    )
}

pub(super) fn wine_bitterness_sample() -> Sample {
    let t = wine_bitterness();
    let request = MixedRequest {
        response: col(&t, "Bitterness"),
        predictors: vec![col(&t, "Temperature"), col(&t, "Skin contact")],
        effects: Vec::new(),
        random: vec![RandomEffectSpec::intercept(col(&t, "Judge"))],
        family: MixedFamily::Ordinal,
        trials: None,
        offset: None,
        reml: true,
        quadrature_points: 1,
        slices: None,
        slopes: None,
        frequency: None,
        weight: None,
        robust: false,
        residual_groups: None,
        df_method: stat_mixed::DfMethod::Satterthwaite,
        order: Vec::new(),
    };
    let reports = vec![SavedReport::Mixed { request, profilers: crate::surface_profiler::ProfilerOptions::default() }];
    Sample { reports, table: t }
}

/// **Simulated** capsule assay (% of label claim, to 0.1) of six lots at 0,
/// 3, 6, 9, 12, 18 and 24 months: `100.5 + b₀ + (−0.12 + b₁)t + ε` with lot
/// effects `b₀ ~ N(0, 0.6²)`, `b₁ ~ N(0, 0.03²)` and `ε ~ N(0, 0.35²)`,
/// drawn in R with `set.seed(51)` (the shelf-life reference case of
/// `crates/stat-mixed/tests/data/extensions_reference.R`).
#[must_use]
pub fn capsule_stability() -> DataTable {
    let months = [0.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0];
    let assay = vec![
        101.6, 100.0, 99.7, 99.8, 99.6, 98.2, 97.1, 100.3, 99.3, 98.6, 98.3, 97.8, 96.6, 96.1, 100.6, 99.4, 99.1, 98.5,
        98.2, 97.5, 96.1, 100.7, 100.9, 100.1, 100.1, 100.1, 99.6, 97.8, 101.8, 100.8, 101.0, 100.0, 99.3, 99.1, 98.2,
        100.6, 100.2, 100.1, 99.9, 99.7, 99.3, 98.4,
    ];
    let lot: Vec<String> = (0..42).map(|i| format!("Lot {}", i / 7 + 1)).collect();
    table(
        "Capsule stability, six lots (simulated)",
        vec![
            text("Lot", lot),
            Column::numeric("Month", (0..6).flat_map(|_| months).collect()),
            Column::numeric("Assay (% label claim)", assay),
        ],
    )
}

pub(super) fn capsule_stability_sample() -> Sample {
    let t = capsule_stability();
    let fixed = ShelfLifeRequest::lower(col(&t, "Month"), col(&t, "Assay (% label claim)"), Some(col(&t, "Lot")), 95.0);
    let random = ShelfLifeRequest { random_batches: true, horizon: Some(60.0), ..fixed.clone() };
    Sample {
        reports: vec![SavedReport::ShelfLife { request: random }, SavedReport::ShelfLife { request: fixed }],
        table: t,
    }
}
