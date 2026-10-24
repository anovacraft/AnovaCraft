//! Samples for accelerated testing: a published accelerated life test
//! (Device-A), published laser degradation paths, and a simulated
//! destructive degradation test of adhesive bonds.

use super::{Sample, col, table, text};
use crate::degradation::{DegradationLayout, DegradationRequest};
use crate::life_distribution::LifeDataLayout;
use crate::life_stress::{LifeStressRequest, StressColumn};
use crate::project::SavedReport;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};
use stat_reliability::degradation::{FailureDirection, PathModel, ScaleTransform};
use stat_reliability::life_regression::RegressionFamily;
use stat_reliability::life_stress::{StressTransform, TemperatureUnit};

/// Device-A accelerated life test (Meeker & Escobar 1998, *Statistical
/// Methods for Reliability Data*, Table C.10, from Hooper & Amster 1990):
/// hours to failure of devices run at 10, 40, 60 and 80 °C, with the
/// survivors removed at 5000 hours. Each row counts `Units` devices.
#[must_use]
pub fn device_a() -> DataTable {
    let mut hours = Vec::new();
    let mut censored = Vec::new();
    let mut temp = Vec::new();
    let mut units = Vec::new();
    let mut push = |h: f64, c: f64, t: f64, n: f64| {
        hours.push(h);
        censored.push(c);
        temp.push(t);
        units.push(n);
    };
    push(5000.0, 1.0, 10.0, 30.0);
    for h in [1298.0, 1390.0, 3187.0, 3241.0, 3261.0, 3313.0, 4501.0, 4568.0, 4841.0, 4982.0] {
        push(h, 0.0, 40.0, 1.0);
    }
    push(5000.0, 1.0, 40.0, 90.0);
    for h in [581.0, 925.0, 1432.0, 1586.0, 2452.0, 2734.0, 2772.0, 4106.0, 4674.0] {
        push(h, 0.0, 60.0, 1.0);
    }
    push(5000.0, 1.0, 60.0, 11.0);
    for h in [283.0, 361.0, 515.0, 638.0, 854.0, 1024.0, 1030.0, 1045.0, 1767.0, 1777.0, 1856.0, 1951.0, 1964.0, 2884.0]
    {
        push(h, 0.0, 80.0, 1.0);
    }
    push(5000.0, 1.0, 80.0, 1.0);
    table(
        "Device-A accelerated life test",
        vec![
            Column::numeric("Hours", hours),
            Column::numeric("Censored", censored),
            Column::numeric("Temperature (°C)", temp),
            Column::numeric("Units", units),
        ],
    )
}

/// The Device-A sample: a lognormal–Arrhenius model used at 10 °C.
pub(super) fn device_a_sample() -> Sample {
    let t = device_a();
    let request = LifeStressRequest {
        frequency: Some(col(&t, "Units")),
        family: RegressionFamily::Lognormal,
        probabilities: vec![0.01, 0.1, 0.5],
        times: vec![50_000.0, 100_000.0],
        ..LifeStressRequest::new(
            LifeDataLayout::Times {
                time: col(&t, "Hours"),
                censor: Some(col(&t, "Censored")),
                coding: crate::StatusCoding::NonzeroIsCensored,
            },
            col(&t, "Temperature (°C)"),
            10.0,
        )
    };
    Sample { table: t, reports: vec![SavedReport::LifeStress { request }] }
}

/// Percent increase in operating current of 15 GaAs lasers tested at 80 °C,
/// measured every 250 hours to 4000 hours (Meeker & Escobar 1998, Table
/// C.17). A laser is taken to fail when its current has risen by 10%.
pub const LASER_CURRENT: [[f64; 16]; 15] = [
    [
        0.4741, 0.9255, 2.1147, 2.7168, 3.511, 4.3415, 4.9076, 5.4782, 5.9925, 6.7224, 7.1303, 8.0006, 8.9193, 9.494,
        9.8675, 10.9446,
    ],
    [
        0.7078, 1.2175, 1.8955, 2.2954, 2.865, 3.7454, 4.4192, 4.9894, 5.5148, 6.0668, 6.6385, 7.1615, 7.7778, 8.4242,
        8.9092, 9.2834,
    ],
    [
        0.7074, 1.1651, 1.7253, 1.9888, 2.5325, 2.9695, 3.2977, 3.9354, 4.1613, 4.4459, 4.889, 5.2696, 5.6913, 6.0216,
        6.4485, 6.8849,
    ],
    [
        0.3634, 0.6161, 1.3597, 1.9525, 2.2995, 2.9468, 3.3871, 3.7863, 4.1116, 4.5013, 4.7209, 4.9766, 5.2812, 5.615,
        5.9503, 6.1438,
    ],
    [
        0.2707, 0.6144, 1.1093, 1.773, 2.0625, 2.5835, 2.9863, 3.3765, 4.0491, 4.626, 5.237, 5.6191, 6.0449, 6.3167,
        7.0952, 7.5941,
    ],
    [
        0.3584, 1.3922, 1.9541, 2.8617, 3.4552, 3.8117, 4.533, 5.3541, 5.9217, 6.7078, 7.7027, 8.6053, 9.1477, 9.951,
        10.4857, 11.0096,
    ],
    [
        0.3637, 0.9237, 1.2098, 1.4591, 1.9323, 2.3862, 2.6799, 2.9397, 3.4183, 4.0857, 4.5843, 4.8402, 5.1145, 5.5691,
        6.114, 7.1723,
    ],
    [
        0.458, 1.0678, 1.4163, 1.7659, 2.1061, 2.3969, 2.78, 3.0187, 3.289, 3.7542, 4.1595, 4.7575, 5.1597, 5.4591,
        5.8085, 6.245,
    ],
    [
        0.5098, 0.9308, 1.5713, 1.9576, 2.5871, 3.2892, 3.6075, 4.1148, 4.5964, 4.913, 5.3408, 5.8418, 6.3961, 6.8441,
        7.2037, 7.8848,
    ],
    [0.4136, 1.488, 2.381, 2.995, 3.835, 4.501, 5.251, 6.256, 7.051, 7.803, 8.321, 8.93, 9.554, 10.45, 11.28, 12.21],
    [
        0.4435, 0.9978, 1.5738, 1.9584, 2.5093, 2.8436, 3.4682, 4.007, 4.5124, 4.8018, 5.2042, 5.6556, 6.1951, 6.5405,
        6.9626, 7.4238,
    ],
    [
        0.3864, 0.7988, 1.3521, 1.741, 2.979, 3.5871, 4.0315, 4.4393, 4.7906, 5.2205, 5.4848, 5.9639, 6.2276, 6.9877,
        7.3671, 7.8841,
    ],
    [
        0.3034, 0.7448, 1.5225, 1.8548, 2.3897, 2.9473, 3.5104, 3.9207, 5.0316, 5.4708, 5.8362, 6.4962, 6.9372, 7.3883,
        7.8472, 8.087,
    ],
    [
        0.438, 0.6979, 1.0515, 1.3526, 1.7998, 2.5514, 2.8274, 3.3908, 3.7159, 4.0906, 4.8296, 5.4145, 5.759, 6.1419,
        6.5086, 6.8823,
    ],
    [
        0.5056, 0.8349, 1.2864, 1.5191, 1.9148, 2.2697, 2.7763, 3.4153, 3.7814, 4.1053, 4.3754, 4.6295, 5.3831, 5.8391,
        6.1618, 6.6247,
    ],
];

/// The laser degradation data in long form: `Laser`, `Hours`, `Current increase (%)`.
#[must_use]
pub fn laser_degradation() -> DataTable {
    let (mut unit, mut hours, mut value) = (Vec::new(), Vec::new(), Vec::new());
    for (i, path) in LASER_CURRENT.iter().enumerate() {
        for (k, &v) in path.iter().enumerate() {
            unit.push(format!("L{:02}", i + 1));
            hours.push(250.0 * (k + 1) as f64);
            value.push(v);
        }
    }
    table(
        "Laser degradation",
        vec![text("Laser", unit), Column::numeric("Hours", hours), Column::numeric("Current increase (%)", value)],
    )
}

/// The laser sample: linear paths, pseudo failure times at a 10% increase,
/// and a Weibull life distribution.
pub(super) fn laser_sample() -> Sample {
    let t = laser_degradation();
    let request = DegradationRequest {
        times: vec![4000.0, 8000.0],
        ..DegradationRequest::new(
            DegradationLayout::RepeatedMeasures {
                unit: col(&t, "Laser"),
                time: col(&t, "Hours"),
                response: col(&t, "Current increase (%)"),
            },
            10.0,
            FailureDirection::Rising,
        )
    };
    Sample { table: t, reports: vec![SavedReport::Degradation { request }] }
}

/// **Simulated** destructive degradation test of adhesive bonds: three bonds
/// broken at each of 0, 2, 4, 8, 16 and 32 weeks at each of 50, 60 and
/// 70 °C (54 bonds). Strength (newtons) is `90·exp(r·√weeks + 0.04·Z)` with
/// `Z` standard normal and rate `r = −0.025·exp(−0.7·(x − x₄₀))`, `x =
/// 1/(k·T)` in eV⁻¹: an Arrhenius rate with activation energy 0.7 eV,
/// −0.025 per √week at 40 °C. Seed `0xB04D`.
#[must_use]
pub fn adhesive_bonds() -> DataTable {
    let x = |c: f64| stat_reliability::life_stress::INVERSE_BOLTZMANN / (c + 273.15);
    let mut rng = Rng::new(0xB0_4D);
    let (mut weeks, mut temp, mut strength) = (Vec::new(), Vec::new(), Vec::new());
    for c in [50.0, 60.0, 70.0] {
        let rate = -0.025 * (-0.7 * (x(c) - x(40.0))).exp();
        for w in [0.0, 2.0, 4.0, 8.0, 16.0, 32.0f64] {
            for _ in 0..3 {
                let v = 90.0 * (rate * w.sqrt() + 0.04 * rng.normal()).exp();
                weeks.push(w);
                temp.push(c);
                strength.push((v * 100.0).round() / 100.0);
            }
        }
    }
    table(
        "Adhesive bond strength (simulated)",
        vec![
            Column::numeric("Weeks", weeks),
            Column::numeric("Temperature (°C)", temp),
            Column::numeric("Strength (N)", strength),
        ],
    )
}

/// The adhesive-bond sample: log strength linear in √weeks with an
/// Arrhenius rate, failure below 50 N, used at 40 °C.
pub(super) fn adhesive_bond_sample() -> Sample {
    let t = adhesive_bonds();
    let request = DegradationRequest {
        path: PathModel::Linear { response: ScaleTransform::Log, time: ScaleTransform::Sqrt },
        stress: Some(StressColumn {
            column: col(&t, "Temperature (°C)"),
            transform: StressTransform::Arrhenius,
            unit: TemperatureUnit::Celsius,
        }),
        use_stress: Some(40.0),
        probabilities: vec![0.01, 0.1, 0.5],
        times: vec![260.0, 520.0],
        ..DegradationRequest::new(
            DegradationLayout::Destructive { time: col(&t, "Weeks"), response: col(&t, "Strength (N)") },
            50.0,
            FailureDirection::Falling,
        )
    };
    Sample { table: t, reports: vec![SavedReport::Degradation { request }] }
}
