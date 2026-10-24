//! Graph canvas annotations and chart forms: swarm points, significance
//! brackets, convex hulls, estimation plots, clustered heat maps and flow
//! charts.

use super::*;

/// Sample `name` with its reports replaced by one graph canvas of `spec`,
/// in a tall window on the results view.
fn canvas(name: &str, spec: stat_viz::GraphSpec) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.harness.set_size(egui::vec2(1400.0, 1400.0));
    gui.results();
    gui
}

/// The graph canvas's current specification.
fn spec(gui: &Gui) -> stat_viz::GraphSpec {
    let app = gui.app();
    let app_model::SavedReport::Graph { spec } = app.documents[app.active].reports[0].saved() else {
        panic!("a graph")
    };
    *spec
}

/// Score by programme with points, from the reading trial.
fn scores() -> stat_viz::GraphSpec {
    stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan { x: vec!["Programme".into()], y: vec!["Score".into()], ..Default::default() },
        points: Some(stat_viz::PointsElement::default()),
        ..stat_viz::GraphSpec::default()
    }
}

/// Click once at `at` (screen points).
fn click_at(gui: &mut Gui, at: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(at, true));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(at, false));
    gui.run();
}

/// Where text starting with `label` was last painted (the plot is drawn
/// after the shelves, which name the same columns).
fn text_rect(gui: &Gui, label: &str) -> egui::Rect {
    gui.painted_text_rects()
        .into_iter()
        .rfind(|(t, _, clip)| t.starts_with(label) && clip.width() > 0.0)
        .unwrap_or_else(|| panic!("{label:?} is drawn: {:?}", gui.painted_text()))
        .1
}

/// The points' settings offer the swarm layout, and the Brackets layer
/// writes adjusted p-values with stars above the categories.
#[test]
fn swarm_points_and_significance_brackets() {
    let mut gui = canvas("Reading trial", scores());
    // The first settings button of the layer bar is the points'.
    gui.harness.get_all_by_label("…").next().expect("points settings").simulate_click();
    gui.run();
    gui.click("Swarm (no overlap)");
    assert_eq!(spec(&gui).points.unwrap().jitter, stat_viz::JitterKind::Swarm);
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    gui.click("Brackets");
    let brackets = spec(&gui).brackets.expect("the layer is on");
    assert_eq!(brackets.test, stat_viz::BracketTest::TukeyKramer, "Tukey–Kramer by default");
    let text = gui.painted_text();
    assert!(text.iter().filter(|t| t.starts_with("p = ") || t.starts_with("p < ")).count() >= 3, "{text:?}");
    gui.shot("graph-brackets");
}

/// Choosing the estimation plot from the Chart list lays the points out as
/// a swarm and draws the mean differences from the first group; the Welch
/// interval can be chosen instead of the bootstrap.
#[test]
fn estimation_plot_from_the_chart_list() {
    let mut gui = canvas("Reading trial", scores());
    // The list's entry (selectable, so toggled), pressed through the
    // accessibility tree in case the list scrolled it out of sight.
    let entry = |n: &Node<'_>| {
        n.role() != Role::Label && n.toggled().is_some() && n.label().as_deref() == Some("Estimation plot")
    };
    gui.harness.get_all_by_role(Role::ComboBox).next().expect("the chart list").simulate_click();
    gui.run();
    gui.harness.get_by(entry).click();
    gui.run();
    if gui.harness.query_by(entry).is_some() {
        gui.harness.press_key(egui::Key::Escape);
        gui.run();
    }
    let s = spec(&gui);
    assert_eq!(s.form, stat_viz::GraphForm::Estimation);
    assert_eq!(s.points.unwrap().jitter, stat_viz::JitterKind::Swarm, "an estimation plot uses a swarm");
    assert!(gui.painted_text().iter().any(|t| t.starts_with("Mean difference from")), "{:?}", gui.painted_text());
    gui.click("Welch t");
    assert_eq!(spec(&gui).estimation.interval, stat_viz::EstimationInterval::Welch);
    gui.shot("graph-estimation");
}

/// The Hulls layer outlines each color group of a scatter plot.
#[test]
fn hulls_outline_each_group() {
    let mut gui = canvas(
        "Regional climate",
        stat_viz::GraphSpec {
            encoding: stat_viz::EncodingPlan {
                x: vec!["Rainfall (mm)".into()],
                y: vec!["Sunshine (h)".into()],
                color: Some("Region".into()),
                ..Default::default()
            },
            points: Some(stat_viz::PointsElement::default()),
            ..stat_viz::GraphSpec::default()
        },
    );
    gui.click("Hulls");
    assert!(spec(&gui).hull.is_some_and(|h| h.fill));
    gui.shot("graph-hulls");
}

/// Clicking a cell of the clustered heat map selects that cell's row.
#[test]
fn heat_map_cell_click_selects_its_row() {
    let mut gui = canvas(
        "Regional climate",
        stat_viz::GraphSpec {
            encoding: stat_viz::EncodingPlan {
                x: ["January (°C)", "July (°C)", "Rainfall (mm)", "Sunshine (h)", "Wind (m/s)", "Frost days"]
                    .map(String::from)
                    .to_vec(),
                label: Some("Town".into()),
                ..Default::default()
            },
            form: stat_viz::GraphForm::ClusterMap(stat_viz::ClusterMapSettings::default()),
            ..stat_viz::GraphSpec::default()
        },
    );
    assert!(gui.shows("standardize columns"), "the heat map's settings are shown");
    let (column, town) = (text_rect(&gui, "Rainfall (mm)"), text_rect(&gui, "Dalefield"));
    click_at(&mut gui, egui::pos2(column.center().x, town.center().y));
    let app = gui.app();
    let doc = &app.documents[app.active];
    let row =
        (0..doc.table.n_rows()).find(|&r| doc.table.column(0).unwrap().cell_text(r) == "Dalefield").expect("the town");
    assert_eq!(doc.rows.selected_rows(), vec![row]);
    gui.shot("graph-cluster-map");
}

/// Clicking a node of a Sankey diagram selects the rows that pass through it.
#[test]
fn sankey_node_click_selects_its_rows() {
    let mut gui = canvas(
        "Parcel flows",
        stat_viz::GraphSpec {
            encoding: stat_viz::EncodingPlan {
                x: vec!["Origin".into(), "Hub".into(), "Destination".into()],
                y: vec!["Parcels".into()],
                ..Default::default()
            },
            form: stat_viz::GraphForm::Sankey(stat_viz::FlowSettings { size_by_sum: true, labels: true }),
            ..stat_viz::GraphSpec::default()
        },
    );
    // The middle stage's labels sit just right of their nodes.
    let label = text_rect(&gui, "Hub A");
    click_at(&mut gui, egui::pos2(label.min.x - 18.0, label.center().y));
    let app = gui.app();
    let doc = &app.documents[app.active];
    let expected: Vec<usize> =
        (0..doc.table.n_rows()).filter(|&r| doc.table.column(1).unwrap().cell_text(r) == "Hub A").collect();
    assert!(!expected.is_empty());
    assert_eq!(doc.rows.selected_rows(), expected);
    gui.shot("graph-sankey");
}
