//! Invented samples for the graph canvas's estimation plots, significance
//! brackets, clustered heat maps, convex hulls and flow charts.

use super::{Sample, round_to, table, text};
use crate::project::SavedReport;
use stat_core::rng::Rng;
use stat_data::{Column, DataTable};
use stat_viz::{
    BracketElement, ClusterMapSettings, EncodingPlan, EstimationSettings, FlowSettings, GraphForm, GraphSpec,
    HullElement, JitterKind, NetworkSettings, PointsElement,
};

fn graph(spec: GraphSpec) -> SavedReport {
    SavedReport::Graph { spec: Box::new(spec) }
}

/// Swarm markers wide enough for a group's points to sit side by side.
fn swarm() -> PointsElement {
    PointsElement { jitter: JitterKind::Swarm, jitter_width: 0.45, ..PointsElement::default() }
}

/// **Invented** reading-test scores of 80 pupils, 20 in each arm of a
/// 2 × 2 trial: phonics lessons (no/yes) crossed with small-group tutoring
/// (no/yes). Scores are normal with standard deviation 8 around 50
/// (control), 54 (phonics), 58 (tutoring) and 61 (both), drawn with the
/// SplitMix64 generator seeded 2026 and rounded to whole points.
#[must_use]
pub fn reading_trial() -> DataTable {
    let arms = [
        ("Control", "No", "No", 50.0),
        ("Phonics", "Yes", "No", 54.0),
        ("Tutoring", "No", "Yes", 58.0),
        ("Both", "Yes", "Yes", 61.0),
    ];
    let mut rng = Rng::new(2026);
    let (mut programme, mut phonics, mut tutored, mut score) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (arm, p, t, mean) in arms {
        for _ in 0..20 {
            programme.push(arm.to_owned());
            phonics.push(p.to_owned());
            tutored.push(t.to_owned());
            score.push(round_to(mean + 8.0 * rng.normal(), 0));
        }
    }
    table(
        "Reading trial (invented)",
        vec![
            Column::numeric("Pupil", (1..=80).map(f64::from).collect()),
            text("Programme", programme),
            text("Phonics", phonics),
            text("Tutored", tutored),
            Column::numeric("Score", score),
        ],
    )
}

pub(super) fn reading_trial_sample() -> Sample {
    let by = |x: &str| EncodingPlan { x: vec![x.to_owned()], y: vec!["Score".to_owned()], ..Default::default() };
    let estimation = |x: &str, reference: &str| GraphSpec {
        encoding: by(x),
        points: Some(swarm()),
        form: GraphForm::Estimation,
        estimation: EstimationSettings { reference: Some(reference.to_owned()), ..EstimationSettings::default() },
        ..GraphSpec::default()
    };
    Sample {
        reports: vec![
            graph(estimation("Tutored", "No")),
            graph(estimation("Programme", "Control")),
            graph(GraphSpec {
                encoding: by("Programme"),
                points: Some(swarm()),
                brackets: Some(BracketElement::default()),
                ..GraphSpec::default()
            }),
        ],
        table: reading_trial(),
    }
}

/// **Invented** parcel flows: 96 consignments from four regions through two
/// sorting hubs to four regions, with the number of parcels in each
/// (1 to 40). Hub A takes most of the north and west, hub B most of the
/// south and east; consignments mostly go to another region. Drawn with the
/// SplitMix64 generator seeded 7.
#[must_use]
pub fn parcel_flows() -> DataTable {
    let regions = ["North", "South", "East", "West"];
    let mut rng = Rng::new(7);
    let (mut origin, mut hub, mut destination, mut parcels) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..96 {
        let o = i % 4;
        let a_share = [0.8, 0.25, 0.3, 0.75][o];
        let h = if rng.next_f64() < a_share { "Hub A" } else { "Hub B" };
        let d = if rng.next_f64() < 0.15 { o } else { (o + 1 + rng.below(3)) % 4 };
        origin.push(regions[o].to_owned());
        hub.push(h.to_owned());
        destination.push(regions[d].to_owned());
        parcels.push((1 + rng.below(40)) as f64);
    }
    table(
        "Parcel flows (invented)",
        vec![
            text("Origin", origin),
            text("Hub", hub),
            text("Destination", destination),
            Column::numeric("Parcels", parcels),
        ],
    )
}

pub(super) fn parcel_flows_sample() -> Sample {
    let cols = |c: &[&str]| EncodingPlan {
        x: c.iter().map(|s| (*s).to_owned()).collect(),
        y: vec!["Parcels".to_owned()],
        ..Default::default()
    };
    let by_parcels = FlowSettings { size_by_sum: true, ..FlowSettings::default() };
    Sample {
        reports: vec![
            graph(GraphSpec {
                encoding: cols(&["Origin", "Hub", "Destination"]),
                form: GraphForm::Sankey(by_parcels),
                ..GraphSpec::default()
            }),
            graph(GraphSpec {
                encoding: cols(&["Origin", "Destination"]),
                form: GraphForm::Chord(by_parcels),
                ..GraphSpec::default()
            }),
            graph(GraphSpec {
                encoding: cols(&["Origin", "Hub"]),
                form: GraphForm::Network(NetworkSettings::default()),
                ..GraphSpec::default()
            }),
        ],
        table: parcel_flows(),
    }
}

/// **Invented** climate normals of 24 towns, six in each of four regions:
/// January and July mean temperature (°C), annual rainfall (mm), sunshine
/// (hours a year), mean wind speed (m/s) and frost days. Each region has its
/// own typical values (a mild wet coast, a cold upland, a dry warm plain and
/// a sunny south) with normal scatter, drawn with the SplitMix64 generator
/// seeded 31.
#[must_use]
pub fn regional_climate() -> DataTable {
    // January, July, rainfall, sunshine, wind, frost days.
    let regions = [
        ("Coast", [6.0, 17.0, 1300.0, 1500.0, 6.5, 15.0]),
        ("Upland", [-1.0, 15.0, 1700.0, 1200.0, 5.5, 85.0]),
        ("Plain", [2.0, 21.0, 600.0, 1800.0, 3.5, 55.0]),
        ("South", [8.0, 25.0, 700.0, 2500.0, 3.0, 8.0]),
    ];
    let spread = [1.2, 1.0, 150.0, 120.0, 0.6, 8.0];
    let digits = [1, 1, 0, 0, 1, 0];
    let mut rng = Rng::new(31);
    let (mut town, mut region) = (Vec::new(), Vec::new());
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); 6];
    for (r, (name, centre)) in regions.iter().enumerate() {
        for k in 0..6 {
            town.push(format!(
                "{}{}",
                ["Ash", "Brook", "Cliff", "Dale", "Elm", "Ford"][k],
                ["mouth", "moor", "field", "haven"][r]
            ));
            region.push((*name).to_owned());
            for j in 0..6 {
                let v = (centre[j] + spread[j] * rng.normal()).max(if j >= 2 { 0.0 } else { f64::NEG_INFINITY });
                values[j].push(round_to(v, digits[j]));
            }
        }
    }
    let names = ["January (°C)", "July (°C)", "Rainfall (mm)", "Sunshine (h)", "Wind (m/s)", "Frost days"];
    let mut columns = vec![text("Town", town), text("Region", region)];
    columns.extend(names.iter().zip(values).map(|(n, v)| Column::numeric(*n, v)));
    table("Regional climate (invented)", columns)
}

pub(super) fn regional_climate_sample() -> Sample {
    let measures = ["January (°C)", "July (°C)", "Rainfall (mm)", "Sunshine (h)", "Wind (m/s)", "Frost days"];
    Sample {
        reports: vec![
            graph(GraphSpec {
                encoding: EncodingPlan {
                    x: measures.map(String::from).to_vec(),
                    label: Some("Town".to_owned()),
                    ..Default::default()
                },
                form: GraphForm::ClusterMap(ClusterMapSettings::default()),
                ..GraphSpec::default()
            }),
            graph(GraphSpec {
                encoding: EncodingPlan {
                    x: vec!["Rainfall (mm)".to_owned()],
                    y: vec!["Sunshine (h)".to_owned()],
                    color: Some("Region".to_owned()),
                    ..Default::default()
                },
                points: Some(PointsElement::default()),
                hull: Some(HullElement::default()),
                ..GraphSpec::default()
            }),
        ],
        table: regional_climate(),
    }
}

#[cfg(test)]
mod tests {
    use stat_data::RowStateManager;

    #[test]
    fn graph_samples_draw_their_new_forms() {
        for s in [super::reading_trial_sample(), super::parcel_flows_sample(), super::regional_climate_sample()] {
            let rows = RowStateManager::new(s.table.n_rows());
            for r in &s.reports {
                let crate::project::SavedReport::Graph { spec } = r else { panic!() };
                let frame = spec.frame(&s.table, &rows).unwrap();
                let layers = spec.layers(&frame, &rows);
                assert!(layers.iter().flatten().any(|l| !l.marks.is_empty()), "{:?}", spec.form);
                assert!(
                    layers.iter().flatten().all(|l| l.notes.iter().all(|n| !n.contains("cannot"))),
                    "{:?}",
                    spec.form
                );
            }
        }
    }
}
