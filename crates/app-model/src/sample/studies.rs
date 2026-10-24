//! Simulated samples for the gauge accuracy studies (bias and linearity,
//! single-gauge repeatability) and for robust design with orthogonal arrays.
//! Each is drawn from a stated model with a fixed seed.

use super::{Sample, col, round_to, table, text};
use crate::project::SavedReport;
use crate::{
    ArrayFactor, DesignDeclarationRequest, LinearityRequest, RepeatabilityRequest, RobustDesignRequest,
    SignalToNoiseRequest, create_robust_design,
};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};
use stat_doe::{SignalToNoise, StandardArray};

/// **Simulated** gauge linearity study: five reference parts (2, 4, 6, 8 and
/// 10 mm) each measured 12 times. The gauge's bias falls linearly from about
/// +0.5 mm at 2 mm to −0.6 mm at 10 mm (bias = 0.75 − 0.135 · reference),
/// with measurement error N(0, 0.15²); readings rounded to 0.01 mm.
#[must_use]
pub fn gauge_linearity() -> DataTable {
    let mut rng = Rng::new(0x11AE);
    let (mut part, mut reference, mut measurement) = (Vec::new(), Vec::new(), Vec::new());
    for (p, r) in [2.0, 4.0, 6.0, 8.0, 10.0].into_iter().enumerate() {
        for _ in 0..12 {
            part.push(format!("R{}", p + 1));
            reference.push(r);
            measurement.push(round_to(r + 0.75 - 0.135 * r + 0.15 * rng.normal(), 2));
        }
    }
    table(
        "Gauge linearity (simulated)",
        vec![text("Part", part), Column::numeric("Reference", reference), Column::numeric("Measurement", measurement)],
    )
}

pub(super) fn gauge_linearity_sample() -> Sample {
    let t = gauge_linearity();
    let request = LinearityRequest {
        process_variation: Some(6.0),
        ..LinearityRequest::new(col(&t, "Reference"), col(&t, "Measurement"))
    };
    Sample { reports: vec![SavedReport::GaugeLinearity { request }], table: t }
}

/// **Simulated** single-gauge repeatability study: a master part of
/// 25.000 mm measured 50 times in a row on one gauge. The gauge reads
/// 0.001 mm high with repeatability error N(0, 0.002²); readings rounded to
/// 0.001 mm. Against a tolerance of 0.1 mm the gauge is capable
/// (Cg ≈ 1.7, Cgk ≈ 1.5 by the model). The part was brought to the gauge
/// alternately from below and from above (the Approach column), and
/// readings approached from above are 0.0015 mm higher (hysteresis); the
/// gauge also creeps up by 0.00004 mm per reading (drift).
#[must_use]
pub fn gauge_repeatability() -> DataTable {
    let mut rng = Rng::new(0xC6C6);
    let values: Vec<f64> = (0..50)
        .map(|i| {
            let above = if i % 2 == 1 { 0.0015 } else { 0.0 };
            round_to(25.000_25 + above + 0.000_04 * f64::from(i) + 0.002 * rng.normal(), 3)
        })
        .collect();
    let approach = (0..50).map(|i| if i % 2 == 1 { "above" } else { "below" }.to_owned()).collect();
    table(
        "Gauge repeatability (simulated)",
        vec![
            Column::numeric("Order", (1..=50).map(f64::from).collect()),
            text("Approach", approach),
            Column::numeric("Measurement", values),
        ],
    )
}

pub(super) fn gauge_repeatability_sample() -> Sample {
    let t = gauge_repeatability();
    let request = RepeatabilityRequest {
        direction: Some(col(&t, "Approach")),
        order: Some(col(&t, "Order")),
        ..RepeatabilityRequest::new(col(&t, "Measurement"), 25.0, 0.1)
    };
    Sample { reports: vec![SavedReport::GaugeRepeatability { request }], table: t }
}

/// **Simulated** robust-design experiment on moulding shrinkage (%): four
/// control factors in an L8 inner array (temperature 150/180 °C on column 1,
/// hold time 10/20 s on column 2, pressure 2/4 bar on column 3, resin A/B
/// on column 4) crossed with an L4 outer array of two noise factors
/// (ambient humidity and material age, coded ±1). Shrinkage is
/// `1.2 + 0.25 T − 0.15 t + 0.05 R + (0.30 − 0.20 P)·humidity + 0.10·age + ε`,
/// ε ~ N(0, 0.05²), with T, t, P, R coded −1/+1 at the low/high level: the
/// higher pressure makes the process insensitive to humidity, which the
/// signal-to-noise ratio picks out. Values rounded to 0.01.
#[must_use]
pub fn moulding_shrinkage() -> DataTable {
    let two =
        |name: &str, levels: [&str; 2]| ArrayFactor { name: name.into(), levels: levels.map(String::from).to_vec() };
    let request = RobustDesignRequest {
        inner: StandardArray::L8,
        control: vec![
            two("Temperature", ["150", "180"]),
            two("Hold time", ["10", "20"]),
            two("Pressure", ["2", "4"]),
            two("Resin", ["A", "B"]),
        ],
        outer: Some(StandardArray::L4),
        noise: vec![two("Humidity", ["low", "high"]), two("Age", ["new", "old"])],
        replicates: 1,
        response: "Shrinkage".into(),
        randomize: None,
    };
    let (mut t, _) = create_robust_design(&request).expect("a valid robust design");
    let inner = StandardArray::L8.array();
    let outer = StandardArray::L4.array();
    let coded = |level: usize| if level == 1 { -1.0 } else { 1.0 };
    let mut rng = Rng::new(0x5A11);
    let first_response = t.n_cols() - outer.len();
    for (o, noise) in outer.iter().enumerate() {
        let values: Vec<f64> = inner
            .iter()
            .map(|r| {
                let (temp, time, press, resin) = (coded(r[0]), coded(r[1]), coded(r[2]), coded(r[3]));
                let (humidity, age) = (coded(noise[0]), coded(noise[1]));
                let y = 1.2 + 0.25 * temp - 0.15 * time
                    + 0.05 * resin
                    + (0.30 - 0.20 * press) * humidity
                    + 0.10 * age
                    + 0.05 * rng.normal();
                round_to(y, 2)
            })
            .collect();
        let name = t.column(first_response + o).expect("response column").name.clone();
        t.replace_column(first_response + o, Column::numeric(name, values)).expect("same length");
    }
    rename(t, "Moulding shrinkage (simulated)")
}

fn rename(t: DataTable, name: &str) -> DataTable {
    DataTable::new(name, t.columns().to_vec()).expect("columns have equal length")
}

pub(super) fn moulding_shrinkage_sample() -> Sample {
    let t = moulding_shrinkage();
    let factors: Vec<usize> = ["Temperature", "Hold time", "Pressure", "Resin"].iter().map(|n| col(&t, n)).collect();
    let responses: Vec<usize> = (t.n_cols() - 4..t.n_cols()).collect();
    let reports = vec![
        SavedReport::SignalToNoise {
            request: SignalToNoiseRequest {
                factors: factors.clone(),
                responses: responses.clone(),
                ratio: SignalToNoise::SmallerTheBetter,
            },
        },
        SavedReport::DeclaredDesign { request: DesignDeclarationRequest::new(&factors, &responses) },
    ];
    Sample { table: t, reports }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stat_data::RowStateManager;

    #[test]
    fn studies_have_the_stated_shapes() {
        let l = gauge_linearity();
        assert_eq!(l.n_rows(), 60);
        let r = gauge_repeatability();
        assert_eq!(r.n_rows(), 50);
        let m = moulding_shrinkage();
        assert_eq!((m.n_rows(), m.n_cols()), (8, 9));
        assert_eq!(m.column(5).unwrap().name, "Shrinkage (Humidity=low, Age=new)");
        assert_eq!(gauge_linearity(), l, "generated identically every time");
    }

    /// The signal-to-noise analysis finds the model's robust setting: high
    /// pressure (which removes the humidity effect) and low temperature.
    #[test]
    fn robust_sample_recovers_the_model() {
        let s = moulding_shrinkage_sample();
        let SavedReport::SignalToNoise { request } = &s.reports[0] else { panic!() };
        let a = crate::SignalToNoiseAnalysis::run(&s.table, &RowStateManager::new(8), request).unwrap();
        let (best, _, _) = a.best_settings();
        assert_eq!(best[0], 0, "low temperature");
        assert_eq!(best[2], 1, "high pressure");
        let SavedReport::DeclaredDesign { request } = &s.reports[1] else { panic!() };
        let d = crate::DeclaredDesign::run(&s.table, &RowStateManager::new(8), request).unwrap();
        assert!(d.model_support()[0].estimable);
        assert_eq!(d.coded_runs()[0].len(), 4, "Resin, a two-level text column, is declared too");
    }
}
