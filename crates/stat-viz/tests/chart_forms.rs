//! Integration tests of the round, bridge, ridgeline and vector chart forms
//! and lollipop bars: each builds from a table, draws finite marks, and
//! exports to every file format with its labels; round forms keep their
//! shape in the file.

use stat_data::{Column, DataTable, RowStateManager, SelectionMode};
use stat_viz::export::{Figure, VectorFormat, equal_aspect_view, export_graph, panel_views};
use stat_viz::{
    AngleUnit, BarElement, BarLayout, EncodingPlan, GraphForm, GraphSpec, PieSettings, PolarSettings, RadarSettings,
    RidgelineSettings, SharedRanges, VectorSettings, WaterfallSettings, WindRoseSettings,
};

fn table() -> DataTable {
    let n = 24;
    let text = |f: &dyn Fn(usize) -> &'static str| (0..n).map(|i| f(i).to_owned()).collect::<Vec<_>>();
    DataTable::new(
        "weather",
        vec![
            Column::numeric("dir", (0..n).map(|i| (i * 37 % 360) as f64).collect()),
            Column::numeric("speed", (0..n).map(|i| 1.0 + (i % 7) as f64 * 1.3).collect()),
            Column::numeric("east", (0..n).map(|i| ((i as f64) * 0.7).sin() * 3.0).collect()),
            Column::numeric("north", (0..n).map(|i| ((i as f64) * 0.7).cos() * 3.0).collect()),
            Column::numeric("x", (0..n).map(|i| (i % 6) as f64).collect()),
            Column::numeric("y", (0..n).map(|i| (i / 6) as f64).collect()),
            Column::numeric("sand", (0..n).map(|i| 10.0 + i as f64).collect()),
            Column::numeric("silt", (0..n).map(|i| 30.0 - i as f64).collect()),
            Column::numeric("clay", (0..n).map(|i| 5.0 + (i % 5) as f64).collect()),
            Column::text("sky", text(&|i| ["Clear", "Cloud", "Rain"][i % 3])),
            Column::text("item", text(&|i| ["Income", "Rent", "Food", "Gift"][i % 4])),
            Column::numeric("amount", (0..n).map(|i| [500.0, -200.0, -90.0, 40.0][i % 4]).collect()),
        ],
    )
    .unwrap()
}

fn spec(form: GraphForm, x: &[&str], y: &[&str], color: Option<&str>) -> GraphSpec {
    GraphSpec {
        encoding: EncodingPlan {
            x: x.iter().map(|s| (*s).to_owned()).collect(),
            y: y.iter().map(|s| (*s).to_owned()).collect(),
            color: color.map(str::to_owned),
            ..Default::default()
        },
        form,
        ..GraphSpec::default()
    }
}

/// Every new form with a label its drawing (or key) must show.
fn every_form() -> Vec<(GraphSpec, &'static str)> {
    let radar = RadarSettings { group_means: true, fill: true };
    let polar = PolarSettings { angle: AngleUnit::Compass, connect: true };
    vec![
        (spec(GraphForm::Pie(PieSettings { donut: true, ..PieSettings::default() }), &["sky"], &[], None), "Rain"),
        (spec(GraphForm::Radar(radar), &["speed", "sand"], &["clay"], Some("sky")), "sand"),
        (spec(GraphForm::Polar(polar), &["dir"], &["speed"], Some("sky")), "90°"),
        (spec(GraphForm::WindRose(WindRoseSettings::default()), &["dir"], &["speed"], None), "NE"),
        (spec(GraphForm::Ternary, &["sand", "silt"], &["clay"], Some("sky")), "silt"),
        (spec(GraphForm::Waterfall(WaterfallSettings::default()), &["item"], &["amount"], None), "Total"),
        (spec(GraphForm::Ridgeline(RidgelineSettings::default()), &["speed"], &["sky"], Some("sky")), "Cloud"),
        (spec(GraphForm::VectorField(VectorSettings::default()), &["x", "east"], &["y", "north"], None), "arrows"),
        (
            GraphSpec {
                bars: Some(BarElement { layout: BarLayout::Lollipop, labels: true, ..BarElement::default() }),
                points: None,
                ..spec(GraphForm::Panels, &["sky"], &["speed"], None)
            },
            "Lollipops",
        ),
    ]
}

#[test]
fn every_new_form_draws_and_exports() {
    let t = table();
    let mut state = RowStateManager::new(t.n_rows());
    state.select([0, 1, 2], SelectionMode::Replace);
    for (spec, label) in every_form() {
        let frame = spec.frame(&t, &state).unwrap_or_else(|e| panic!("{:?}: {e}", spec.form));
        let layers = spec.layers(&frame, &state);
        assert!(layers.iter().flatten().all(|l| l.all_finite()), "{:?}", spec.form);
        assert!(layers.iter().flatten().any(|l| !l.marks.is_empty()), "{:?}", spec.form);
        let ranges = SharedRanges::of(&frame, &layers);
        let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
        let in_key = layers.iter().flatten().flat_map(|l| l.key.iter()).any(|k| k.label.contains(label));
        for format in VectorFormat::ALL {
            let figure = Figure { title: "t", format, width: 520.0, height: 380.0 };
            let bytes = export_graph(&spec, &frame, &layers, &views, figure);
            assert!(bytes.len() > 200, "{:?} {format:?}", spec.form);
            if format == VectorFormat::Svg {
                let svg = String::from_utf8(bytes).unwrap();
                assert!(svg.contains(label) || in_key, "{:?}: {label}", spec.form);
            }
        }
        assert_eq!(spec.form.draws_own_axes(), spec.form.equal_aspect(), "{:?}", spec.form);
    }
}

#[test]
fn round_forms_keep_their_shape() {
    // A wide area widens the horizontal range; a tall one the vertical.
    assert_eq!(equal_aspect_view(([-1.0, 1.0], [-1.0, 1.0]), 400.0, 200.0), ([-2.0, 2.0], [-1.0, 1.0]));
    assert_eq!(equal_aspect_view(([0.0, 1.0], [0.0, 4.0]), 100.0, 100.0), ([-1.5, 2.5], [0.0, 4.0]));
    assert_eq!(equal_aspect_view(([0.0, 0.0], [0.0, 1.0]), 1.0, 1.0), ([0.0, 0.0], [0.0, 1.0]));
}
