//! Sample for missing-value imputation.

use super::{Noise, Sample, col, round_to, table};
use crate::project::SavedReport;
use crate::{ImputationMethod, ImputationRequest};
use stat_data::{Column, DataTable, MeasurementLevel};

/// Measured columns of the air-quality sample.
pub const AIR_QUALITY_COLUMNS: [&str; 5] =
    ["Temperature (°C)", "Humidity (%)", "NO₂ (µg/m³)", "Ozone (µg/m³)", "PM2.5 (µg/m³)"];

/// **Simulated** daily means from one air-quality monitoring station over
/// 120 days. A seasonal temperature `T = 18 + 8 sin(2πd/120) + 2ε`, humidity
/// `65 − 1.2(T − 18) + 6ε`, and a pollution level `P` following an AR(1)
/// process (`Pₜ = 0.7 Pₜ₋₁ + ε`) drive `NO₂ = 25 + 8P + 3ε`,
/// `ozone = 40 + 1.5(T − 18) − 3P + 4ε` and `PM2.5 = 12 + 5P + 0.08(H − 65) + 2ε`
/// (ε independent standard normal). Values are missing as monitors fail:
/// the PM2.5 sensor is out for days 31 to 38, the ozone analyser drops out on
/// very humid days (humidity above 78 %, so missing at random given
/// humidity) and about 4 % of other readings are lost at random.
#[must_use]
pub fn air_quality() -> DataTable {
    let mut noise = Noise(0xA1E0_0A11_7700_2026);
    let n = 120;
    let mut values: Vec<Vec<f64>> = (0..5).map(|_| Vec::with_capacity(n)).collect();
    let mut p = 0.0;
    for d in 0..n {
        let t = 18.0 + 8.0 * (std::f64::consts::TAU * d as f64 / n as f64).sin() + 2.0 * noise.normal();
        let h = (65.0 - 1.2 * (t - 18.0) + 6.0 * noise.normal()).clamp(20.0, 100.0);
        p = 0.7 * p + noise.normal();
        let no2 = (25.0 + 8.0 * p + 3.0 * noise.normal()).max(1.0);
        let ozone = (40.0 + 1.5 * (t - 18.0) - 3.0 * p + 4.0 * noise.normal()).max(1.0);
        let pm = (12.0 + 5.0 * p + 0.08 * (h - 65.0) + 2.0 * noise.normal()).max(0.5);
        for (j, v) in [t, h, no2, ozone, pm].into_iter().enumerate() {
            values[j].push(round_to(v, 1));
        }
    }
    for d in 0..n {
        if (30..38).contains(&d) {
            values[4][d] = f64::NAN;
        }
        if values[1][d] > 78.0 {
            values[3][d] = f64::NAN;
        }
        for j in [0, 1, 2, 3, 4] {
            if noise.uniform() < 0.04 {
                values[j][d] = f64::NAN;
            }
        }
    }
    let mut day = Column::numeric("Day", (1..=n).map(|d| d as f64).collect());
    day.level = MeasurementLevel::Ordinal;
    let mut columns = vec![day];
    columns.extend(AIR_QUALITY_COLUMNS.iter().zip(values).map(|(name, v)| Column::numeric(*name, v)));
    table("Air quality (simulated)", columns)
}

pub(super) fn air_quality_sample() -> Sample {
    let t = air_quality();
    let columns: Vec<usize> = AIR_QUALITY_COLUMNS.iter().map(|n| col(&t, n)).collect();
    let reports = vec![
        SavedReport::Imputation { request: ImputationRequest::new(columns.clone()) },
        SavedReport::Imputation {
            request: ImputationRequest { method: ImputationMethod::LowRank, ..ImputationRequest::new(columns) },
        },
    ];
    Sample { table: t, reports }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_quality_has_the_described_gaps() {
        let t = air_quality();
        let missing =
            |name: &str| t.column(col(&t, name)).unwrap().as_numeric().unwrap().iter().filter(|v| v.is_nan()).count();
        assert!(missing("PM2.5 (µg/m³)") >= 8);
        assert!(missing("Ozone (µg/m³)") > missing("Temperature (°C)"));
        let total: usize = AIR_QUALITY_COLUMNS.iter().map(|n| missing(n)).sum();
        assert!((40..120).contains(&total), "{total}");
    }
}
