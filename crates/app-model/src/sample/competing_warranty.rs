//! Samples for competing risks and warranty returns.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{CompetingRisksRequest, WarrantyLayout, WarrantyRequest};
use stat_core::distribution_fit::FitFamily;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};

/// A Weibull draw with shape `beta` and scale `eta`.
fn weibull(rng: &mut Rng, beta: f64, eta: f64) -> f64 {
    eta * (-(1.0 - rng.next_f64()).ln()).powf(1.0 / beta)
}

/// **Simulated** field data: 120 water pumps at two sites, followed for up to
/// 5000 hours (each pump's follow-up ends between 2000 and 5000 hours as it
/// joined the study). Each pump fails from the first of three independent
/// modes, or is still `Running` at the end of its follow-up:
///
/// * *Seal leak* — Weibull, shape 0.9, scale 14 000 h (early, random);
/// * *Bearing wear* — Weibull, shape 3, scale `6000·exp(−0.12·(load − 10))` h:
///   wear-out that comes sooner under a higher `Load (kN)` (uniform 6–14);
/// * *Impeller crack* — Weibull, shape 1.5, scale 12 000 h at the North site
///   and 6000 h at the harsher South site.
///
/// Times are rounded to whole hours; drawn with `stat_core::rng::Rng`
/// (seed `0xC0_4E_75`).
#[must_use]
pub fn pump_seal_failures() -> DataTable {
    let mut rng = Rng::new(0xC0_4E_75);
    let (mut id, mut site, mut load, mut hours, mut mode) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for k in 0..120 {
        let south = k % 2 == 1;
        let kn = ((6.0 + 8.0 * rng.next_f64()) * 10.0).round() / 10.0;
        let leak = weibull(&mut rng, 0.9, 14_000.0);
        let wear = weibull(&mut rng, 3.0, 6000.0 * (-0.12 * (kn - 10.0)).exp());
        let crack = weibull(&mut rng, 1.5, if south { 6000.0 } else { 12_000.0 });
        let follow = 2000.0 + 3000.0 * rng.next_f64();
        let first = leak.min(wear).min(crack);
        let (t, m) = if follow < first {
            (follow, "Running")
        } else if first == leak {
            (leak, "Seal leak")
        } else if first == wear {
            (wear, "Bearing wear")
        } else {
            (crack, "Impeller crack")
        };
        id.push(format!("WP{:03}", k + 1));
        site.push(if south { "South" } else { "North" }.to_owned());
        load.push(kn);
        hours.push(t.round().max(1.0));
        mode.push(m.to_owned());
    }
    table(
        "Pump failure modes (simulated)",
        vec![
            Column::text("Pump", id),
            Column::text("Site", site),
            Column::numeric("Load (kN)", load),
            Column::numeric("Hours", hours),
            Column::text("Failure mode", mode),
        ],
    )
}

pub(super) fn pump_failure_modes_sample() -> Sample {
    let t = pump_seal_failures();
    let request = CompetingRisksRequest {
        censor_value: Some("Running".into()),
        group: Some(col(&t, "Site")),
        covariates: vec![col(&t, "Load (kN)"), col(&t, "Site")],
        mode_family: Some(FitFamily::Weibull),
        ..CompetingRisksRequest::new(col(&t, "Hours"), col(&t, "Failure mode"))
    };
    Sample { table: t, reports: vec![SavedReport::CompetingRisks { request }] }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// **Simulated** warranty data for one year of a consumer product: units
/// shipped each month (`Shipped`) and, for each month of shipment, the units
/// returned in each calendar month of the year (`Returned Jan` …
/// `Returned Dec`; empty before the shipment month). Every unit's age at
/// failure is drawn from a Weibull distribution with shape 1.6 and scale 60
/// months (about 2.7% fail in the first year of service), units entering
/// service at the start of their shipment month; a return is counted in the
/// month of the failure. Drawn with `stat_core::rng::Rng` (seed `0x3A_44_17`).
#[must_use]
pub fn monthly_warranty_returns() -> DataTable {
    let mut rng = Rng::new(0x3A_44_17);
    let shipped: [f64; 12] =
        [800.0, 950.0, 1100.0, 1200.0, 1150.0, 1300.0, 1400.0, 1350.0, 1500.0, 1450.0, 1600.0, 1550.0];
    let mut returns = vec![vec![f64::NAN; 12]; 12];
    for (s, &n) in shipped.iter().enumerate() {
        for slot in returns[s].iter_mut().skip(s) {
            *slot = 0.0;
        }
        for _ in 0..n as usize {
            let age = weibull(&mut rng, 1.6, 60.0);
            let month = s + age.floor() as usize;
            if month < 12 {
                returns[s][month] += 1.0;
            }
        }
    }
    let mut columns = vec![
        Column::text("Ship month", MONTHS.iter().map(|m| format!("{m} 2025")).collect()),
        Column::numeric("Shipped", shipped.to_vec()),
    ];
    for (j, m) in MONTHS.iter().enumerate() {
        columns.push(Column::numeric(format!("Returned {m}"), (0..12).map(|s| returns[s][j]).collect()));
    }
    table("Monthly warranty returns (simulated)", columns)
}

pub(super) fn warranty_returns_sample() -> Sample {
    let t = monthly_warranty_returns();
    let returns = MONTHS.iter().map(|m| col(&t, &format!("Returned {m}"))).collect();
    let request = WarrantyRequest {
        planned: vec![1600.0; 6],
        ..WarrantyRequest::new(WarrantyLayout::Triangular {
            shipped: col(&t, "Shipped"),
            returns,
            label: Some(col(&t, "Ship month")),
        })
    };
    Sample { table: t, reports: vec![SavedReport::Warranty { request }] }
}
