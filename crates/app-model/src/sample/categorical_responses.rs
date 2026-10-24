//! Samples for ordinal and nominal logistic regression. Both datasets are
//! **invented** (simulated), not survey or study data.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{CategoricalLogisticRequest, CategoricalModel};
use stat_data::{Column, DataTable, MeasurementLevel};

/// The rating scale of [`service_ratings`], lowest first.
pub const SERVICE_RATING_SCALE: [&str; 5] = ["Poor", "Fair", "Good", "Very good", "Excellent"];

/// **Simulated** satisfaction ratings of 150 customer contacts: the wait in
/// minutes (uniform on 1–30, to 0.1), the channel (phone, chat or branch,
/// equally likely) and a five-point rating drawn from a proportional-odds
/// model with cut-points −3.2, −1.6, 0.4, 2.2, a wait slope of −0.11 per
/// minute and channel effects branch +0.9, chat +0.3 (against phone).
/// Drawn with Python's `random.seed(20261016)`.
#[must_use]
pub fn service_ratings() -> DataTable {
    let wait = vec![
        4.9, 20.8, 6.1, 25.6, 2.5, 29.6, 19.7, 22.6, 17.3, 19.4, 4.5, 2.8, 21.5, 15.8, 8.3, 23.8, 3.3, 5.6, 19.2, 18.6,
        10.4, 16.9, 20.9, 4.7, 6.4, 10.4, 22.3, 10.5, 27.1, 2.9, 27.3, 21.0, 27.6, 28.4, 19.7, 6.7, 25.4, 12.8, 7.9,
        22.1, 20.2, 19.9, 21.7, 21.4, 27.5, 1.7, 25.0, 11.8, 13.3, 29.8, 5.7, 27.7, 6.1, 12.7, 13.8, 8.1, 16.3, 27.6,
        17.8, 16.7, 8.6, 15.7, 18.1, 3.8, 28.0, 19.9, 19.9, 11.4, 24.2, 3.9, 11.1, 12.4, 3.7, 12.5, 13.4, 18.5, 28.0,
        24.7, 30.0, 5.4, 20.0, 18.7, 23.3, 24.1, 5.2, 19.5, 2.9, 12.1, 22.3, 11.2, 4.9, 22.1, 11.2, 14.4, 6.4, 4.9,
        1.9, 5.9, 6.4, 28.6, 1.1, 13.1, 11.3, 27.7, 2.6, 7.0, 7.0, 9.2, 5.9, 8.2, 2.8, 23.7, 13.2, 8.4, 4.8, 22.7, 7.6,
        26.9, 18.0, 16.8, 19.9, 25.9, 13.0, 29.8, 24.3, 5.9, 25.5, 9.9, 26.4, 4.9, 19.1, 11.6, 22.0, 28.8, 27.0, 18.6,
        26.6, 19.3, 25.4, 17.5, 6.0, 2.2, 13.2, 21.1, 21.5, 3.0, 23.2, 7.9, 2.9, 9.9,
    ];
    let channel = [
        "Branch", "Branch", "Branch", "Phone", "Branch", "Branch", "Phone", "Branch", "Branch", "Branch", "Branch",
        "Branch", "Chat", "Phone", "Branch", "Chat", "Phone", "Chat", "Chat", "Phone", "Phone", "Phone", "Phone",
        "Branch", "Branch", "Phone", "Branch", "Chat", "Chat", "Chat", "Branch", "Branch", "Chat", "Chat", "Chat",
        "Phone", "Chat", "Phone", "Phone", "Branch", "Phone", "Phone", "Branch", "Branch", "Phone", "Branch", "Branch",
        "Branch", "Phone", "Branch", "Branch", "Phone", "Phone", "Branch", "Branch", "Chat", "Chat", "Chat", "Chat",
        "Chat", "Branch", "Chat", "Chat", "Phone", "Branch", "Phone", "Phone", "Branch", "Phone", "Chat", "Chat",
        "Phone", "Branch", "Phone", "Chat", "Phone", "Chat", "Branch", "Chat", "Chat", "Phone", "Chat", "Branch",
        "Branch", "Branch", "Chat", "Phone", "Chat", "Branch", "Chat", "Phone", "Phone", "Branch", "Phone", "Branch",
        "Chat", "Branch", "Chat", "Chat", "Chat", "Chat", "Branch", "Chat", "Chat", "Chat", "Phone", "Chat", "Branch",
        "Chat", "Chat", "Phone", "Chat", "Branch", "Phone", "Branch", "Chat", "Branch", "Chat", "Chat", "Branch",
        "Chat", "Branch", "Branch", "Branch", "Phone", "Branch", "Branch", "Phone", "Chat", "Chat", "Branch", "Phone",
        "Branch", "Phone", "Chat", "Branch", "Phone", "Phone", "Chat", "Chat", "Phone", "Chat", "Branch", "Branch",
        "Chat", "Chat", "Branch", "Branch", "Branch", "Phone",
    ];
    let rating = [
        "Very good",
        "Fair",
        "Very good",
        "Fair",
        "Very good",
        "Good",
        "Good",
        "Fair",
        "Good",
        "Fair",
        "Good",
        "Good",
        "Poor",
        "Good",
        "Good",
        "Fair",
        "Very good",
        "Good",
        "Fair",
        "Good",
        "Fair",
        "Fair",
        "Good",
        "Fair",
        "Good",
        "Fair",
        "Fair",
        "Very good",
        "Fair",
        "Fair",
        "Excellent",
        "Excellent",
        "Fair",
        "Very good",
        "Good",
        "Fair",
        "Poor",
        "Good",
        "Good",
        "Fair",
        "Fair",
        "Poor",
        "Fair",
        "Good",
        "Fair",
        "Excellent",
        "Good",
        "Fair",
        "Good",
        "Good",
        "Fair",
        "Fair",
        "Very good",
        "Very good",
        "Good",
        "Good",
        "Good",
        "Fair",
        "Very good",
        "Very good",
        "Fair",
        "Fair",
        "Fair",
        "Good",
        "Good",
        "Poor",
        "Good",
        "Good",
        "Poor",
        "Good",
        "Good",
        "Fair",
        "Good",
        "Fair",
        "Fair",
        "Fair",
        "Poor",
        "Good",
        "Fair",
        "Fair",
        "Poor",
        "Fair",
        "Fair",
        "Good",
        "Very good",
        "Good",
        "Very good",
        "Fair",
        "Good",
        "Good",
        "Good",
        "Poor",
        "Good",
        "Good",
        "Poor",
        "Good",
        "Good",
        "Very good",
        "Good",
        "Fair",
        "Excellent",
        "Good",
        "Excellent",
        "Poor",
        "Good",
        "Good",
        "Excellent",
        "Very good",
        "Good",
        "Good",
        "Good",
        "Poor",
        "Good",
        "Good",
        "Good",
        "Poor",
        "Excellent",
        "Poor",
        "Good",
        "Good",
        "Good",
        "Poor",
        "Poor",
        "Poor",
        "Good",
        "Very good",
        "Poor",
        "Good",
        "Good",
        "Good",
        "Fair",
        "Fair",
        "Good",
        "Fair",
        "Poor",
        "Good",
        "Poor",
        "Fair",
        "Fair",
        "Poor",
        "Good",
        "Good",
        "Poor",
        "Very good",
        "Poor",
        "Fair",
        "Good",
        "Fair",
        "Good",
        "Very good",
    ];
    let mut rating = Column::text("Rating", rating.iter().map(|s| (*s).to_owned()).collect());
    rating.level = MeasurementLevel::Ordinal;
    table(
        "Service ratings (simulated)",
        vec![
            Column::numeric("Wait (min)", wait),
            Column::text("Channel", channel.iter().map(|s| (*s).to_owned()).collect()),
            rating,
        ],
    )
}

pub(super) fn service_ratings_sample() -> Sample {
    let t = service_ratings();
    let request = CategoricalLogisticRequest {
        order: SERVICE_RATING_SCALE.iter().map(|s| (*s).to_owned()).collect(),
        ..CategoricalLogisticRequest::new(
            CategoricalModel::Ordinal,
            col(&t, "Rating"),
            vec![col(&t, "Wait (min)"), col(&t, "Channel")],
        )
    };
    Sample { reports: vec![SavedReport::CategoricalLogistic { request }], table: t }
}

/// **Simulated** commuting choices of 200 people: the distance in km
/// (log-normal, median about 3.7 km, to 0.1), age in years (uniform 18–70)
/// and the usual mode, drawn from a baseline-category logit model with car
/// as the baseline: walk `2.5 − 1.6 d`, bicycle `1.2 − 0.35 d − 0.05 (a − 40)`,
/// bus `0.4 − 0.05 d + 0.02 (a − 40)`. Drawn with Python's
/// `random.seed(20261016)` after [`service_ratings`].
#[must_use]
pub fn commute_modes() -> DataTable {
    let distance = vec![
        5.4, 4.0, 0.8, 5.0, 4.2, 4.2, 3.9, 5.1, 0.9, 3.2, 1.7, 4.3, 1.7, 2.8, 6.6, 1.9, 3.4, 4.0, 5.6, 4.6, 8.1, 4.5,
        16.6, 18.8, 5.7, 7.3, 3.0, 5.2, 2.7, 7.3, 6.2, 13.3, 4.1, 0.7, 3.0, 3.0, 0.8, 1.6, 5.3, 6.7, 3.9, 4.0, 2.0,
        3.6, 2.0, 13.4, 2.2, 4.5, 7.4, 2.7, 1.4, 7.2, 5.4, 4.6, 9.2, 1.7, 1.4, 5.4, 1.4, 1.5, 2.9, 11.1, 0.8, 3.3, 1.2,
        0.8, 4.3, 10.9, 4.0, 1.8, 3.6, 7.5, 3.0, 3.7, 3.0, 7.2, 5.1, 4.0, 10.7, 4.1, 4.0, 9.5, 15.9, 2.2, 4.0, 2.9,
        1.0, 2.0, 0.9, 1.4, 3.0, 1.6, 6.7, 4.6, 13.5, 0.9, 9.1, 7.6, 1.8, 0.8, 6.7, 1.2, 2.5, 2.3, 2.7, 4.8, 1.3, 2.5,
        3.5, 2.9, 5.2, 4.3, 5.1, 7.4, 4.9, 2.0, 2.9, 3.9, 3.1, 1.0, 1.8, 2.6, 4.1, 1.6, 6.8, 4.9, 9.6, 6.1, 0.6, 16.4,
        5.0, 3.2, 4.4, 4.0, 4.8, 18.9, 1.7, 2.1, 2.2, 3.6, 4.7, 2.5, 2.8, 5.9, 1.9, 4.2, 5.8, 4.3, 1.4, 1.9, 1.3, 1.9,
        7.8, 8.4, 1.2, 6.3, 1.1, 7.4, 5.6, 7.9, 1.3, 10.1, 9.6, 2.3, 4.2, 1.9, 6.7, 4.5, 1.4, 5.1, 5.3, 4.4, 4.9, 4.6,
        2.6, 4.8, 5.1, 1.3, 11.5, 2.2, 6.4, 5.5, 15.7, 2.6, 6.0, 3.5, 3.1, 3.9, 5.5, 9.1, 3.8, 1.9, 1.2, 6.8, 3.1, 2.8,
        20.1, 2.7, 18.3, 1.5,
    ];
    let age = vec![
        70.0, 59.0, 58.0, 61.0, 36.0, 70.0, 46.0, 68.0, 24.0, 62.0, 40.0, 43.0, 34.0, 19.0, 24.0, 65.0, 67.0, 40.0,
        65.0, 24.0, 55.0, 62.0, 26.0, 62.0, 23.0, 51.0, 44.0, 58.0, 47.0, 21.0, 63.0, 55.0, 20.0, 20.0, 67.0, 27.0,
        57.0, 26.0, 50.0, 24.0, 38.0, 48.0, 29.0, 28.0, 51.0, 47.0, 67.0, 23.0, 66.0, 60.0, 19.0, 33.0, 55.0, 39.0,
        70.0, 63.0, 50.0, 62.0, 28.0, 52.0, 33.0, 67.0, 58.0, 57.0, 65.0, 25.0, 54.0, 45.0, 32.0, 26.0, 69.0, 47.0,
        23.0, 48.0, 58.0, 38.0, 45.0, 58.0, 66.0, 55.0, 24.0, 51.0, 59.0, 22.0, 35.0, 69.0, 56.0, 65.0, 41.0, 28.0,
        18.0, 69.0, 56.0, 55.0, 65.0, 55.0, 43.0, 67.0, 49.0, 34.0, 63.0, 19.0, 43.0, 21.0, 61.0, 69.0, 30.0, 20.0,
        64.0, 39.0, 21.0, 44.0, 57.0, 63.0, 47.0, 69.0, 68.0, 22.0, 47.0, 47.0, 42.0, 54.0, 35.0, 66.0, 57.0, 18.0,
        25.0, 43.0, 30.0, 34.0, 24.0, 38.0, 47.0, 61.0, 31.0, 69.0, 40.0, 30.0, 58.0, 44.0, 49.0, 61.0, 39.0, 20.0,
        63.0, 65.0, 65.0, 18.0, 19.0, 60.0, 24.0, 51.0, 37.0, 30.0, 19.0, 61.0, 30.0, 23.0, 40.0, 59.0, 66.0, 64.0,
        49.0, 31.0, 52.0, 23.0, 30.0, 61.0, 54.0, 45.0, 32.0, 24.0, 64.0, 18.0, 24.0, 69.0, 50.0, 25.0, 45.0, 43.0,
        32.0, 64.0, 30.0, 31.0, 33.0, 33.0, 43.0, 34.0, 39.0, 48.0, 26.0, 41.0, 42.0, 27.0, 69.0, 66.0, 36.0, 27.0,
        68.0, 59.0,
    ];
    let mode = [
        "Car", "Car", "Bus", "Car", "Bicycle", "Bus", "Bus", "Car", "Walk", "Bicycle", "Bicycle", "Car", "Car",
        "Bicycle", "Car", "Car", "Bus", "Bicycle", "Bus", "Bus", "Bus", "Car", "Bus", "Bus", "Bus", "Car", "Bicycle",
        "Bus", "Bus", "Car", "Bicycle", "Bus", "Bicycle", "Bus", "Bicycle", "Bus", "Walk", "Bicycle", "Bus", "Car",
        "Car", "Car", "Bicycle", "Bicycle", "Bus", "Bus", "Bus", "Bicycle", "Bus", "Bus", "Car", "Car", "Car", "Bus",
        "Bus", "Walk", "Car", "Bus", "Walk", "Bicycle", "Bus", "Car", "Walk", "Car", "Bus", "Bicycle", "Bus", "Bus",
        "Car", "Bus", "Bus", "Bus", "Bus", "Car", "Car", "Car", "Bicycle", "Car", "Bus", "Car", "Bicycle", "Bus",
        "Bus", "Bicycle", "Bicycle", "Bus", "Bus", "Bus", "Bicycle", "Car", "Bus", "Walk", "Bus", "Car", "Car", "Bus",
        "Car", "Bus", "Bus", "Bicycle", "Car", "Walk", "Car", "Car", "Bus", "Bicycle", "Bicycle", "Bicycle", "Bicycle",
        "Bus", "Bicycle", "Bus", "Bus", "Bus", "Bicycle", "Car", "Car", "Car", "Bus", "Car", "Bicycle", "Bus", "Bus",
        "Bus", "Bicycle", "Bicycle", "Bus", "Bus", "Bicycle", "Car", "Bus", "Bicycle", "Bus", "Bus", "Bus", "Car",
        "Bus", "Bus", "Bus", "Bus", "Bicycle", "Bus", "Bus", "Bus", "Car", "Bus", "Car", "Bicycle", "Bicycle", "Bus",
        "Bicycle", "Bicycle", "Car", "Car", "Walk", "Car", "Car", "Car", "Bus", "Car", "Walk", "Car", "Car", "Bus",
        "Car", "Bus", "Bus", "Bicycle", "Walk", "Bus", "Car", "Bicycle", "Bus", "Bus", "Bicycle", "Bus", "Bus", "Car",
        "Car", "Bicycle", "Bus", "Car", "Bus", "Bicycle", "Bicycle", "Bicycle", "Bus", "Bicycle", "Car", "Car", "Bus",
        "Walk", "Bicycle", "Bus", "Bus", "Car", "Car", "Car", "Bus", "Bicycle",
    ];
    let mut mode = Column::text("Mode", mode.iter().map(|s| (*s).to_owned()).collect());
    mode.level = MeasurementLevel::Nominal;
    table(
        "Commute modes (simulated)",
        vec![Column::numeric("Distance (km)", distance), Column::numeric("Age", age), mode],
    )
}

pub(super) fn commute_modes_sample() -> Sample {
    let t = commute_modes();
    let request = CategoricalLogisticRequest {
        reference: Some("Car".to_owned()),
        ..CategoricalLogisticRequest::new(
            CategoricalModel::Nominal,
            col(&t, "Mode"),
            vec![col(&t, "Distance (km)"), col(&t, "Age")],
        )
    };
    Sample { reports: vec![SavedReport::CategoricalLogistic { request }], table: t }
}
