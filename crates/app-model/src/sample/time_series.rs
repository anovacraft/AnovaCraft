//! Samples for the Time Series menu: a seasonal series, an S-shaped adoption
//! curve and a pair of series related at a lag.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::series_explore::{
    CrossSettings, DecomposeMethod, DecomposeSettings, ExploreMethod, Prewhiten, SeriesExploreRequest, TrendChoice,
    TrendSettings,
};
use crate::time_series_data::SeriesInput;
use stat_core::rng::Rng;
use stat_data::temporal::days_from_civil;
use stat_data::{Column, DataTable, TemporalDisplay, TemporalFormat};

/// Midnight UTC of `year-month-day`, in seconds since 1970.
fn date(year: i64, month: u32, day: u32) -> f64 {
    days_from_civil(year, month, day) as f64 * 86_400.0
}

/// The first day of `n` consecutive months from `year-month`.
fn months(year: i64, month: u32, n: usize) -> Vec<f64> {
    (0..n)
        .map(|k| {
            let total = year * 12 + i64::from(month) - 1 + k as i64;
            date(total.div_euclid(12), u32::try_from(total.rem_euclid(12)).unwrap_or(0) + 1, 1)
        })
        .collect()
}

fn date_column(name: &str, values: Vec<f64>) -> Column {
    Column::date_time(name, values, TemporalDisplay::new(TemporalFormat::Date))
}

/// **Simulated** monthly electricity demand of a regional grid, January 2015
/// to December 2024 (120 months). Demand is `T_t · S_m · exp(e_t)` in GWh:
/// a trend `T_t = 820 + 2.1 t` (t = 0, 1, …, growth of about 25 GWh a year),
/// a seasonal factor peaking in January (heating) with a smaller peak in
/// July (cooling), `S_m = 1 + 0.11 cos(2π m/12) + 0.07 cos(4π m/12)` for month
/// `m = 0` (January) … 11, and AR(1) noise `e_t = 0.5 e_{t−1} + ε_t`,
/// `ε_t ~ N(0, 0.015²)`. Rounded to 0.1 GWh; drawn with
/// `stat_core::rng::Rng` (seed `0x000E_1EC7`).
#[must_use]
pub fn electricity_demand() -> DataTable {
    let mut rng = Rng::new(0x000E_1EC7);
    let n = 120;
    let mut e = 0.0;
    let demand: Vec<f64> = (0..n)
        .map(|t| {
            e = 0.5 * e + 0.015 * rng.normal();
            let m = (t % 12) as f64;
            let season = 1.0
                + 0.11 * (2.0 * std::f64::consts::PI * m / 12.0).cos()
                + 0.07 * (4.0 * std::f64::consts::PI * m / 12.0).cos();
            ((820.0 + 2.1 * t as f64) * season * e.exp() * 10.0).round() / 10.0
        })
        .collect();
    table(
        "Monthly electricity demand (simulated)",
        vec![date_column("Month", months(2015, 1, n)), Column::numeric("Demand (GWh)", demand)],
    )
}

pub(super) fn electricity_demand_sample() -> Sample {
    let t = electricity_demand();
    let series = SeriesInput { column: col(&t, "Demand (GWh)"), time: Some(col(&t, "Month")), period: None };
    Sample {
        reports: vec![
            SavedReport::SeriesExplore {
                request: SeriesExploreRequest {
                    series,
                    method: ExploreMethod::Describe(crate::series_explore::DescribeSettings::default()),
                },
            },
            SavedReport::SeriesExplore {
                request: SeriesExploreRequest {
                    series,
                    method: ExploreMethod::Decompose(DecomposeSettings {
                        method: DecomposeMethod::ClassicalMultiplicative,
                        ..DecomposeSettings::default()
                    }),
                },
            },
        ],
        table: t,
    }
}

/// **Simulated** cumulative customers of a subscription service in its
/// first 48 months (January 2021 to December 2024), following a logistic
/// adoption curve `C(t) = 52 000 / (1 + 80 e^{−0.16 t})` (t = 1 in the first
/// month; midpoint near month 27) with multiplicative noise `exp(0.02 z)`,
/// `z ~ N(0, 1)`, kept non-decreasing (customers are counted cumulatively).
/// Rounded to whole customers; drawn with `stat_core::rng::Rng` (seed
/// `0x000A_D0B7`).
#[must_use]
pub fn product_adoption() -> DataTable {
    let mut rng = Rng::new(0x000A_D0B7);
    let mut last = 0.0f64;
    let customers: Vec<f64> = (1..=48)
        .map(|t| {
            let c = 52_000.0 / (1.0 + 80.0 * (-0.16 * f64::from(t)).exp()) * (0.02 * rng.normal()).exp();
            last = last.max(c.round());
            last
        })
        .collect();
    table(
        "Product adoption (simulated)",
        vec![date_column("Month", months(2021, 1, 48)), Column::numeric("Customers", customers)],
    )
}

pub(super) fn product_adoption_sample() -> Sample {
    let t = product_adoption();
    let request = SeriesExploreRequest {
        series: SeriesInput { column: col(&t, "Customers"), time: Some(col(&t, "Month")), period: Some(1) },
        method: ExploreMethod::Trend(TrendSettings {
            model: TrendChoice::Logistic,
            horizon: 12,
            ..TrendSettings::default()
        }),
    };
    Sample { table: t, reports: vec![SavedReport::SeriesExplore { request }] }
}

/// **Simulated** weekly advertising spend and sales of a retailer over two
/// years (104 weeks from Monday 5 January 2015). Advertising (k$) follows an
/// AR(1) around 20: `a_t = 20 + 0.6 (a_{t−1} − 20) + 3 ε_t`. Sales (k$)
/// respond to advertising two weeks earlier,
/// `s_t = 200 + 4 (a_{t−2} − 20) + u_t`, with AR(1) noise
/// `u_t = 0.5 u_{t−1} + 6 η_t` (`ε, η ~ N(0, 1)` independent). Rounded to
/// 0.1; drawn with `stat_core::rng::Rng` (seed `0xAD5A1E`).
#[must_use]
pub fn advertising_sales() -> DataTable {
    let mut rng = Rng::new(0x00AD_5A1E);
    let n = 104;
    let burn = 20;
    let (mut a, mut u) = (vec![20.0; n + burn], 0.0);
    for t in 1..n + burn {
        a[t] = 20.0 + 0.6 * (a[t - 1] - 20.0) + 3.0 * rng.normal();
    }
    let mut sales = Vec::with_capacity(n);
    for t in burn..n + burn {
        u = 0.5 * u + 6.0 * rng.normal();
        sales.push(((200.0 + 4.0 * (a[t - 2] - 20.0) + u) * 10.0).round() / 10.0);
    }
    let advertising: Vec<f64> = a[burn..].iter().map(|v| (v * 10.0).round() / 10.0).collect();
    let start = date(2015, 1, 5);
    table(
        "Advertising and sales (simulated)",
        vec![
            date_column("Week", (0..n).map(|k| start + k as f64 * 7.0 * 86_400.0).collect()),
            Column::numeric("Advertising (k$)", advertising),
            Column::numeric("Sales (k$)", sales),
        ],
    )
}

pub(super) fn advertising_sales_sample() -> Sample {
    let t = advertising_sales();
    let request = SeriesExploreRequest {
        series: SeriesInput { column: col(&t, "Sales (k$)"), time: Some(col(&t, "Week")), period: None },
        method: ExploreMethod::CrossCorrelation(CrossSettings {
            max_lag: Some(8),
            prewhiten: Prewhiten::Aic,
            ..CrossSettings::new(col(&t, "Advertising (k$)"))
        }),
    };
    Sample { table: t, reports: vec![SavedReport::SeriesExplore { request }] }
}
