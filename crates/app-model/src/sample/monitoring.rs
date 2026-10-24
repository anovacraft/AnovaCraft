//! Simulated samples for the principal-components control chart and the
//! environmental monitoring limits. Each is drawn from a stated model with a
//! fixed seed.

use super::{Sample, col, round_to, table, text};
use crate::attribute_chart::SubgroupSize;
use crate::monitoring_limits::{LevelMethod, LimitsBy, MonitoringRequest};
use crate::pca_chart::PcaChartRequest;
use crate::project::SavedReport;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel, TemporalDisplay, TemporalFormat};
use stat_quality::ReadingKind;

/// The eight sensors of [`sensor_batches`]: name, mean, loading on the
/// production-rate factor, loading on the ambient factor, noise SD, decimals.
const SENSORS: [(&str, f64, f64, f64, f64, i32); 8] = [
    ("Reactor temperature (°C)", 85.0, 1.2, 0.4, 0.25, 2),
    ("Jacket temperature (°C)", 62.0, -1.0, 0.9, 0.3, 2),
    ("Coolant temperature (°C)", 18.0, -0.6, 1.1, 0.2, 2),
    ("Pressure (bar)", 4.2, 0.12, 0.02, 0.03, 3),
    ("Feed flow (L/min)", 120.0, 6.0, -0.5, 1.2, 1),
    ("Agitator power (kW)", 15.0, 0.8, 0.1, 0.25, 2),
    ("pH", 6.8, -0.05, 0.03, 0.02, 3),
    ("Off-gas CO₂ (%)", 3.5, 0.3, -0.05, 0.06, 3),
];

/// **Simulated** readings of eight sensors on a batch reactor: 100
/// observations, ten per batch. Each reading is
/// `mean + a·f₁ + b·f₂ + noise`, where `f₁` (production rate) and `f₂`
/// (ambient conditions) are independent N(0, 1) factors and `a`, `b` are
/// the sensor's loadings, so the eight sensors move together in two
/// patterns. The first 60 observations (batches 1–6) are normal operation.
/// From observation 71 the coolant-temperature sensor drifts upwards by
/// 0.25 °C per observation without the other sensors following (a broken
/// correlation, seen by the SPE chart); from observation 86 the production
/// rate runs 2.5 SD high (a shift along the model, seen by the T² chart).
#[must_use]
pub fn sensor_batches() -> DataTable {
    let mut rng = Rng::new(0x5E_4502);
    let n = 100;
    let mut columns: Vec<Vec<f64>> = vec![Vec::with_capacity(n); SENSORS.len()];
    let (mut obs, mut batch) = (Vec::new(), Vec::new());
    for i in 0..n {
        let f1 = rng.normal() + if i >= 85 { 2.5 } else { 0.0 };
        let f2 = rng.normal();
        for (j, &(name, mean, a, b, sd, d)) in SENSORS.iter().enumerate() {
            let mut v = mean + a * f1 + b * f2 + sd * rng.normal();
            if name.starts_with("Coolant") && i >= 70 {
                v += 0.25 * (i - 69) as f64;
            }
            columns[j].push(round_to(v, d));
        }
        obs.push((i + 1) as f64);
        batch.push(format!("B{:02}", i / 10 + 1));
    }
    let mut observation = Column::numeric("Observation", obs);
    observation.level = MeasurementLevel::Ordinal;
    let mut cols = vec![observation, text("Batch", batch)];
    cols.extend(SENSORS.iter().zip(columns).map(|(s, v)| Column::numeric(s.0, v)));
    table("Reactor sensors (simulated)", cols)
}

pub(super) fn sensor_batches_sample() -> Sample {
    let t = sensor_batches();
    let sensors: Vec<usize> = SENSORS.iter().map(|s| col(&t, s.0)).collect();
    let reports = vec![SavedReport::PcaChart {
        request: PcaChartRequest {
            reference: Some(60),
            label: Some(col(&t, "Batch")),
            ..PcaChartRequest::new(sensors)
        },
    }];
    Sample { table: t, reports }
}

/// A Poisson draw by inversion (small means only).
fn poisson(rng: &mut Rng, mean: f64) -> f64 {
    let (mut k, mut p) = (0.0, (-mean).exp());
    let mut total = p;
    let u = rng.next_f64();
    while u > total && k < 1000.0 {
        k += 1.0;
        p *= mean / k;
        total += p;
    }
    k
}

/// A negative binomial draw with mean `mean` and size `size` (a gamma
/// mixture of Poissons).
fn negative_binomial(rng: &mut Rng, mean: f64, size: f64) -> f64 {
    let rate = rng.gamma(size) * mean / size;
    poisson(rng, rate)
}

/// The monitored locations of [`clean_room`]: name, room grade, mean colony
/// count, mean particle count (per m³).
const LOCATIONS: [(&str, &str, f64, f64); 10] = [
    ("A-01 Filling needle", "A", 0.08, 400.0),
    ("A-02 Stopper bowl", "A", 0.08, 450.0),
    ("B-01 Filling room door", "B", 1.2, 9_000.0),
    ("B-02 Operator station", "B", 1.6, 11_000.0),
    ("B-03 Return grille", "B", 1.0, 8_000.0),
    ("C-01 Wash bay", "C", 9.0, 160_000.0),
    ("C-02 Gowning bench", "C", 11.0, 180_000.0),
    ("C-03 Material airlock", "C", 8.0, 150_000.0),
    ("D-01 Corridor", "D", 25.0, 900_000.0),
    ("D-02 Store", "", 22.0, 850_000.0),
];

/// **Simulated** environmental monitoring of a clean-room suite: ten
/// locations in rooms of grades A–D (the store's grade was never entered, so
/// it is missing), sampled every working day for 40 days. Air colony counts
/// are negative binomial with size 1.5 and the location's mean (zero-inflated
/// in grade A: 85% of plates are clean); contact plates: 5 per visit in grades
/// A and B, 10 in C and D, each positive with probability 0.04 (A, B) or 0.15
/// (C, D), with a beta-distributed day effect; non-viable particles ≥ 0.5 µm
/// per m³ are lognormal with SD 0.35 on the log scale. Three things happen:
/// the operator station (B-02) has two contaminated days (16 and 23 colonies
/// on days 31 and 33); the material airlock (C-03) counts rise steadily over the
/// last six days; and the corridor (D-01) particle counts jump on day 37.
#[must_use]
pub fn clean_room() -> DataTable {
    let mut rng = Rng::new(0xC1EA_400D);
    let start = 1_798_761_600.0; // 2027-01-01
    let (mut date, mut location, mut grade) = (Vec::new(), Vec::new(), Vec::new());
    let (mut cfu, mut taken, mut positive, mut particles) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for day in 0..40 {
        let p_day = rng.gamma(6.0);
        let p_day = p_day / (p_day + rng.gamma(6.0)) * 2.0;
        for &(name, g, mean, part) in &LOCATIONS {
            date.push(start + 86_400.0 * f64::from(day + day / 5 * 2));
            location.push(name.to_owned());
            grade.push(g.to_owned());
            let mut count = if g == "A" {
                if rng.next_f64() < 0.85 { 0.0 } else { negative_binomial(&mut rng, mean / 0.15, 1.5) }
            } else {
                negative_binomial(&mut rng, mean, 1.5)
            };
            if name.starts_with("B-02") && (day == 30 || day == 32) {
                count = if day == 30 { 16.0 } else { 23.0 };
            }
            if name.starts_with("C-03") && day >= 34 {
                count = (mean * (1.0 + 0.45 * f64::from(day - 33))).round() + f64::from(day - 34);
            }
            cfu.push(count);
            let (n, p) = if matches!(g, "A" | "B") { (5.0, 0.04) } else { (10.0, 0.15) };
            let p = (p * p_day).min(0.9);
            taken.push(n);
            positive.push((0..n as usize).filter(|_| rng.next_f64() < p).count() as f64);
            let jump = if name.starts_with("D-01") && day >= 36 { 3.0 } else { 1.0 };
            particles.push((part * jump * (0.35 * rng.normal()).exp()).round());
        }
    }
    let mut grade = text("Room grade", grade);
    grade.level = MeasurementLevel::Ordinal;
    table(
        "Clean-room monitoring (simulated)",
        vec![
            Column::date_time("Date", date, TemporalDisplay::new(TemporalFormat::Date)),
            text("Location", location),
            grade,
            Column::numeric("Air colonies (CFU)", cfu),
            Column::numeric("Contact plates taken", taken),
            Column::numeric("Contact plates positive", positive),
            Column::numeric("Particles ≥0.5 µm per m³", particles),
        ],
    )
}

pub(super) fn clean_room_sample() -> Sample {
    let t = clean_room();
    let base = |value: &str| MonitoringRequest {
        order: Some(col(&t, "Date")),
        ..MonitoringRequest::new(col(&t, value), Some(col(&t, "Location")))
    };
    let reports = vec![
        SavedReport::MonitoringLimits {
            request: MonitoringRequest {
                limits_by: LimitsBy::Column(col(&t, "Room grade")),
                signals_only: true,
                ..base("Air colonies (CFU)")
            },
        },
        SavedReport::MonitoringLimits {
            request: MonitoringRequest {
                kind: ReadingKind::Positives,
                trials: SubgroupSize::Column(col(&t, "Contact plates taken")),
                limits_by: LimitsBy::Column(col(&t, "Room grade")),
                signals_only: true,
                ..base("Contact plates positive")
            },
        },
        SavedReport::MonitoringLimits {
            request: MonitoringRequest {
                kind: ReadingKind::Continuous,
                method: LevelMethod::Sigma { alert: 2.0, action: 3.0 },
                signals_only: true,
                ..base("Particles ≥0.5 µm per m³")
            },
        },
    ];
    Sample { table: t, reports }
}
