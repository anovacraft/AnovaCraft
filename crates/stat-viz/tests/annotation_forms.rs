//! The annotation layers and the flow, cluster and estimation chart forms
//! from table to file: every one builds, draws finite marks and exports to
//! each figure format.

use stat_data::{Column, DataTable, RowStateManager};
use stat_viz::export::{Figure, VectorFormat, export_graph, panel_views, summary_table};
use stat_viz::{
    BracketElement, ClusterMapSettings, EncodingPlan, FlowSettings, GraphForm, GraphSpec, HullElement, JitterKind,
    NetworkSettings, PointsElement, SharedRanges,
};

fn table() -> DataTable {
    let n = 48;
    let text = |f: &dyn Fn(usize) -> &'static str| (0..n).map(|i| f(i).to_owned()).collect::<Vec<_>>();
    DataTable::new(
        "t",
        vec![
            Column::text("group", text(&|i| ["Control", "Low", "High"][i % 3])),
            Column::numeric(
                "score",
                (0..n).map(|i| 20.0 + (i % 3) as f64 * 2.0 + ((i * 7) % 11) as f64 * 0.4).collect(),
            ),
            Column::numeric("other", (0..n).map(|i| ((i * 5) % 13) as f64).collect()),
            Column::text("origin", text(&|i| ["North", "South", "East", "West"][i % 4])),
            Column::text("hub", text(&|i| ["Hub A", "Hub B"][(i / 3) % 2])),
            Column::text("destination", text(&|i| ["North", "South", "East", "West"][(i * 3 + 1) % 4])),
            Column::numeric("parcels", (0..n).map(|i| 1.0 + (i % 5) as f64).collect()),
        ],
    )
    .unwrap()
}

fn check(spec: &GraphSpec) {
    let t = table();
    let mut state = RowStateManager::new(t.n_rows());
    state.select([0, 5], stat_data::SelectionMode::Replace);
    let frame = spec.frame(&t, &state).unwrap_or_else(|e| panic!("{:?}: {e}", spec.form));
    let layers = spec.layers(&frame, &state);
    assert!(layers.iter().flatten().all(stat_viz::Layer::all_finite), "{:?}", spec.form);
    assert!(layers.iter().flatten().any(|l| !l.marks.is_empty()), "{:?} draws something", spec.form);
    let ranges = SharedRanges::of(&frame, &layers);
    let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
    for format in VectorFormat::ALL {
        let bytes =
            export_graph(spec, &frame, &layers, &views, Figure { title: "T", format, width: 540.0, height: 400.0 });
        assert!(bytes.len() > 200, "{:?} {format:?}", spec.form);
        if format == VectorFormat::Svg {
            let svg = String::from_utf8(bytes).unwrap();
            assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
            assert!(svg.trim_end().ends_with("</svg>"));
        }
    }
}

#[test]
fn every_new_form_and_layer_exports() {
    let groups = EncodingPlan { x: vec!["group".into()], y: vec!["score".into()], ..Default::default() };
    let swarm = PointsElement { jitter: JitterKind::Swarm, ..PointsElement::default() };
    check(&GraphSpec {
        encoding: groups.clone(),
        points: Some(swarm),
        brackets: Some(BracketElement::default()),
        ..GraphSpec::default()
    });
    check(&GraphSpec {
        encoding: EncodingPlan {
            x: vec!["other".into()],
            y: vec!["score".into()],
            color: Some("group".into()),
            ..Default::default()
        },
        hull: Some(HullElement::default()),
        ..GraphSpec::default()
    });
    check(&GraphSpec {
        encoding: groups.clone(),
        points: Some(swarm),
        form: GraphForm::Estimation,
        ..GraphSpec::default()
    });
    let two = GraphSpec {
        encoding: groups.clone(),
        points: Some(swarm),
        form: GraphForm::Estimation,
        options: stat_viz::FrameOptions {
            filter: stat_viz::LocalFilter {
                conditions: vec![stat_viz::FilterCondition::Levels {
                    column: "group".into(),
                    levels: vec!["Control".into(), "High".into()],
                }],
                show_only: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..GraphSpec::default()
    };
    check(&two);
    // The summary table lists the mean difference.
    let t = table();
    let state = RowStateManager::new(t.n_rows());
    let table = summary_table(&two.frame(&t, &state).unwrap());
    assert!(table.iter().any(|r| r[0] == "High − Control"), "{table:?}");
    check(&GraphSpec {
        encoding: EncodingPlan { x: vec!["score".into(), "other".into(), "parcels".into()], ..Default::default() },
        form: GraphForm::ClusterMap(ClusterMapSettings::default()),
        ..GraphSpec::default()
    });
    let flows = EncodingPlan {
        x: vec!["origin".into(), "hub".into(), "destination".into()],
        y: vec!["parcels".into()],
        ..Default::default()
    };
    check(&GraphSpec {
        encoding: flows.clone(),
        form: GraphForm::Sankey(FlowSettings::default()),
        ..GraphSpec::default()
    });
    let ends = EncodingPlan { x: vec!["origin".into(), "destination".into()], ..flows };
    check(&GraphSpec {
        encoding: ends.clone(),
        form: GraphForm::Chord(FlowSettings::default()),
        ..GraphSpec::default()
    });
    check(&GraphSpec { encoding: ends, form: GraphForm::Network(NetworkSettings::default()), ..GraphSpec::default() });
}

/// User annotations and key changes reach exported figures: the text of a
/// text annotation and an arrow's, renamed key entries in place of the
/// generated ones, hidden entries left out; and the drawing order puts the
/// listed layers in the user's order.
#[test]
fn annotations_key_changes_and_layer_order_export() {
    use stat_viz::{Annotation, AnnotationAnchor, AnnotationShape, BoxPlotElement, ElementKind};
    let t = table();
    let state = RowStateManager::new(t.n_rows());
    let mut spec = GraphSpec {
        encoding: EncodingPlan {
            x: vec!["other".into()],
            y: vec!["score".into()],
            color: Some("group".into()),
            ..Default::default()
        },
        points: Some(PointsElement::default()),
        ..GraphSpec::default()
    };
    spec.edit_style(|s| {
        s.annotations.items = vec![
            Annotation {
                shape: AnnotationShape::Text,
                anchor: AnnotationAnchor::Frame,
                panel: 0,
                at: [0.1, 0.9],
                to: [0.1, 0.9],
                text: "Calibrated".into(),
            },
            Annotation {
                shape: AnnotationShape::Arrow,
                anchor: AnnotationAnchor::Data,
                panel: 0,
                at: [2.0, 21.0],
                to: [6.0, 24.0],
                text: "Shift".into(),
            },
        ];
        s.key.rename("color/group/Low", "Low dose", "group Low");
        s.key.set_hidden("color/group/High", true);
    });
    let frame = spec.frame(&t, &state).unwrap();
    let layers = spec.layers(&frame, &state);
    let ids: Vec<String> = spec.key_items(&frame, &layers, None).into_iter().map(|e| e.0).collect();
    assert_eq!(ids, ["color/group/Control", "color/group/High", "color/group/Low"]);
    let ranges = SharedRanges::of(&frame, &layers);
    let views = panel_views(&frame, &ranges, &|_| None, &|_| None);
    let figure = Figure { title: "T", format: VectorFormat::Svg, width: 540.0, height: 400.0 };
    let svg = String::from_utf8(export_graph(&spec, &frame, &layers, &views, figure)).unwrap();
    for shown in ["Calibrated", "Shift", "Low dose", "group Control"] {
        assert!(svg.contains(shown), "{shown} in the figure");
    }
    assert!(!svg.contains("group High") && !svg.contains("group Low"), "hidden and renamed entries");
    // The annotation layer itself: text and an arrow.
    let annotations = spec.annotation_layer(0, &frame.panels[0], views[0]);
    assert!(annotations.marks.iter().any(|m| matches!(m, stat_viz::Mark::Arrow { .. })));

    spec.box_plot = Some(BoxPlotElement::default());
    assert_eq!(spec.layer_kinds(), [ElementKind::BoxPlot, ElementKind::Points]);
    assert!(spec.move_layer(ElementKind::BoxPlot, true));
    assert_eq!(spec.layer_kinds(), [ElementKind::Points, ElementKind::BoxPlot]);
    assert!(!spec.move_layer(ElementKind::BoxPlot, true), "already in front");
    // Turned off and on again, a layer keeps its place in the order.
    spec.box_plot = None;
    spec.fit = Some(stat_viz::FitLineElement::default());
    assert_eq!(spec.layer_kinds(), [ElementKind::FitLine, ElementKind::Points]);
    spec.box_plot = Some(BoxPlotElement::default());
    assert_eq!(spec.layer_kinds(), [ElementKind::Points, ElementKind::FitLine, ElementKind::BoxPlot]);
}
