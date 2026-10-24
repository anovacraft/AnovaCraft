//! Samples for the Time Series menu's forecasting models.

use super::{Sample, col, table};
use crate::arima_analysis::{ArimaRequest, TransferRole};
use crate::forecasting::SeriesInput;
use crate::project::SavedReport;
use crate::smoothing_analysis::SmoothingRequest;
use stat_core::rng::Rng;
use stat_data::temporal::seconds_from_civil;
use stat_data::{Column, DataTable, TemporalDisplay, TemporalFormat};

/// **Simulated** monthly occupancy of a 220-room hotel, January 2016 to
/// December 2024 (108 months): the average number of rooms occupied per
/// night, `Occupied rooms`, drawn from the multiplicative Holt–Winters state
/// space model (Hyndman et al. 2008), so its level, growth and seasonal
/// pattern drift over time: start level 95 rooms, growth 0.55 rooms a month,
/// seasonal factors from a January low of about 0.7 to a summer peak of about
/// 1.3 (averaging 1), relative errors `ε ~ N(0, 0.03²)` and smoothing
/// parameters α = 0.35, β = 0.02, γ = 0.15:
/// `y = (ℓ + b)·s·(1 + ε)`, `ℓ ← (ℓ + b)(1 + αε)`, `b ← b + β(ℓ + b)ε`,
/// `s ← s(1 + γε)`. Rounded to 0.1 room; drawn with `stat_core::rng::Rng`
/// (seed `0x40_7E_11`).
#[must_use]
pub fn hotel_occupancy() -> DataTable {
    let mut rng = Rng::new(0x40_7E_11);
    const FACTORS: [f64; 12] = [0.70, 0.76, 0.88, 0.98, 1.08, 1.22, 1.30, 1.28, 1.10, 0.96, 0.84, 0.90];
    let mean = FACTORS.iter().sum::<f64>() / 12.0;
    let (alpha, beta, gamma) = (0.35, 0.02, 0.15);
    let mut seasons: Vec<f64> = FACTORS.iter().map(|f| f / mean).collect();
    let (mut level, mut slope) = (95.0_f64, 0.55_f64);
    let (mut months, mut rooms) = (Vec::new(), Vec::new());
    for t in 0..108_u32 {
        let (year, month) = (2016 + i64::from(t / 12), t % 12 + 1);
        months.push(seconds_from_civil(year, month, 1, 0, 0, 0.0));
        let e = 0.03 * rng.normal();
        let s = &mut seasons[(t % 12) as usize];
        let base = level + slope;
        let y = base * *s * (1.0 + e);
        level = base * (1.0 + alpha * e);
        slope += beta * base * e;
        *s *= 1.0 + gamma * e;
        rooms.push((y * 10.0).round() / 10.0);
    }
    table(
        "Hotel occupancy (simulated)",
        vec![
            Column::date_time("Month", months, TemporalDisplay::new(TemporalFormat::Date)),
            Column::numeric("Occupied rooms", rooms),
        ],
    )
}

/// **Simulated** hourly temperature (°C) of a chemical reactor over ten days
/// (240 hours from 3 March 2025 00:00): an ARIMA(1,1,1) process — hourly
/// changes `w_t = 0.5 w_{t−1} + e_t + 0.3 e_{t−1}`, `e_t ~ N(0, 0.15²)` —
/// starting at 352 °C, so the temperature wanders without a fixed mean.
/// Rounded to 0.01 °C; drawn with `stat_core::rng::Rng` (seed 3).
#[must_use]
pub fn reactor_temperature() -> DataTable {
    let mut rng = Rng::new(3);
    let start = seconds_from_civil(2025, 3, 3, 0, 0, 0.0);
    let (mut hours, mut temp) = (Vec::new(), Vec::new());
    let (mut level, mut w, mut e_prev) = (352.0, 0.0, 0.0);
    for h in 0..240_u32 {
        let e = 0.15 * rng.normal();
        w = 0.5 * w + e + 0.3 * e_prev;
        e_prev = e;
        level += w;
        hours.push(start + f64::from(h) * 3600.0);
        temp.push((level * 100.0_f64).round() / 100.0);
    }
    table(
        "Reactor temperature (simulated)",
        vec![
            Column::date_time("Hour", hours, TemporalDisplay::new(TemporalFormat::DateTime)),
            Column::numeric("Temperature (°C)", temp),
        ],
    )
}

/// **Simulated** hourly operation of a lime kiln over 12.5 days (300 hours
/// from 2 June 2025 00:00) followed by 24 hours of planned fuel feed:
/// `Fuel feed (t/h)` is a set point moved every 12–36 hours to a new value
/// drawn from `N(6, 0.5²)` (kept within 5–7 t/h) plus an AR(1) wander
/// (`φ = 0.8`, innovations `N(0, 0.05²)`); the
/// `Burning-zone temperature (°C)` responds two hours later through a
/// first-order lag, `vₜ = 0.8 v_{t−1} + 4 x̃_{t−2}` (steady-state gain
/// 20 °C per t/h, reached over about half a day), around 1150 °C, plus AR(1) noise
/// (`φ = 0.5`, innovations `N(0, 1)`). The last 24 rows have the planned
/// feed and no temperature, so forecasts use them. Feed rounded to 0.01 t/h,
/// temperature to 0.1 °C; drawn with `stat_core::rng::Rng` (seed 0x4B11).
#[must_use]
pub fn kiln_fuel_feed() -> DataTable {
    let mut rng = Rng::new(0x4B11);
    let start = seconds_from_civil(2025, 6, 2, 0, 0, 0.0);
    let (observed, planned) = (300_usize, 24_usize);
    let total = observed + planned;
    let mut set_point = 6.0_f64;
    let mut next_step = 20_usize;
    let mut wander = 0.0_f64;
    let mut feed = Vec::with_capacity(total);
    for t in 0..total {
        if t == next_step {
            set_point = (6.0 + 0.5 * rng.normal()).clamp(5.0, 7.0);
            next_step += 12 + rng.below(25);
        }
        wander = 0.8 * wander + 0.05 * rng.normal();
        feed.push(((set_point + wander) * 100.0).round() / 100.0);
    }
    let (mut v, mut noise) = (vec![0.0; total], 0.0_f64);
    let mut temp = Vec::with_capacity(total);
    for t in 0..total {
        let x_lag = if t >= 2 { feed[t - 2] - 6.0 } else { 0.0 };
        v[t] = if t >= 1 { 0.8 * v[t - 1] } else { 0.0 } + 4.0 * x_lag;
        noise = 0.5 * noise + 1.0 * rng.normal();
        temp.push(if t < observed { ((1150.0 + v[t] + noise) * 10.0).round() / 10.0 } else { f64::NAN });
    }
    let hours = (0..total).map(|h| start + h as f64 * 3600.0).collect();
    table(
        "Kiln fuel feed (simulated)",
        vec![
            Column::date_time("Hour", hours, TemporalDisplay::new(TemporalFormat::DateTime)),
            Column::numeric("Fuel feed (t/h)", feed),
            Column::numeric("Burning-zone temperature (°C)", temp),
        ],
    )
}

/// Kiln fuel feed opening an ARIMA report with the feed as a transfer-function
/// input (delay from the data, first-order denominator) and the noise model
/// chosen by the automatic search, forecasting the planned 24 hours.
pub(super) fn kiln_fuel_feed_sample() -> Sample {
    let t = kiln_fuel_feed();
    let mut request = ArimaRequest::new(col(&t, "Burning-zone temperature (°C)"));
    request.series.time = Some(col(&t, "Hour"));
    request.series.period = Some(1);
    request.forecast.horizon = 24;
    request.inputs = vec![TransferRole::new(col(&t, "Fuel feed (t/h)"))];
    Sample { table: t, reports: vec![SavedReport::Arima { request }] }
}

/// Hotel occupancy opening an automatic exponential smoothing report.
pub(super) fn hotel_occupancy_sample() -> Sample {
    let t = hotel_occupancy();
    let request = SmoothingRequest {
        series: SeriesInput { column: col(&t, "Occupied rooms"), time: Some(col(&t, "Month")), period: None },
        ..SmoothingRequest::new(0)
    };
    Sample { table: t, reports: vec![SavedReport::ExponentialSmoothing { request }] }
}

/// Reactor temperature opening an automatic ARIMA report forecasting a day ahead.
pub(super) fn reactor_temperature_sample() -> Sample {
    let t = reactor_temperature();
    let mut request = ArimaRequest::new(col(&t, "Temperature (°C)"));
    request.series.time = Some(col(&t, "Hour"));
    request.series.period = Some(1);
    request.forecast.horizon = 24;
    Sample { table: t, reports: vec![SavedReport::Arima { request }] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_complete_and_positive() {
        let h = hotel_occupancy();
        assert_eq!(h.n_rows(), 108);
        assert!(h.column(1).unwrap().as_numeric().unwrap().iter().all(|v| *v > 0.0));
        let k = kiln_fuel_feed();
        assert_eq!(k.n_rows(), 324);
        let temp = k.column(2).unwrap().as_numeric().unwrap();
        assert!(temp[..300].iter().all(|v| v.is_finite()) && temp[300..].iter().all(|v| v.is_nan()));
        let r = reactor_temperature();
        assert_eq!(r.n_rows(), 240);
        assert!(r.column(1).unwrap().as_numeric().unwrap().iter().all(|v| v.is_finite()));
    }
}
