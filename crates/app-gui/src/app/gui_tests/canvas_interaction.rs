//! Drop zones over the graph canvas, columns dropped on a report plot's
//! axis, and the filter panel with stepping through levels.

use super::*;

/// Longley with its own reports closed and a graph canvas of Employed
/// against GNP.
fn graph() -> Gui {
    let mut gui = Gui::with_sample("Longley");
    gui.harness.set_size(egui::vec2(1500.0, 1100.0));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let spec = stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan { x: vec!["GNP".into()], y: vec!["Employed".into()], ..Default::default() },
        points: Some(stat_viz::PointsElement::default()),
        ..stat_viz::GraphSpec::default()
    };
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.run();
    gui
}

fn saved_spec(gui: &Gui) -> stat_viz::GraphSpec {
    let doc = &gui.app().documents[gui.app().active];
    let app_model::SavedReport::Graph { spec } = doc.reports[0].saved() else { panic!("a graph report") };
    *spec
}

fn rect(node: &Node<'_>) -> egui::Rect {
    let b = node.raw_bounds().expect("the control has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// Where the variables list shows `column` (the first widget with that
/// label: the list is drawn before the results).
fn variable(gui: &Gui, column: &str) -> egui::Pos2 {
    rect(&gui.harness.get_all_by_label(column).next().expect("the column in the variables list")).center()
}

fn button(gui: &mut Gui, event: bool, pos: egui::Pos2) {
    gui.harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: event,
        modifiers: egui::Modifiers::NONE,
    });
    gui.harness.step();
}

/// Press on `from` and move to `to` in steps, without letting go.
fn drag_to(gui: &mut Gui, from: egui::Pos2, to: egui::Pos2) {
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(from));
    gui.harness.step();
    button(gui, true, from);
    for i in 1..=8 {
        gui.harness.input_mut().events.push(egui::Event::PointerMoved(from.lerp(to, i as f32 / 8.0)));
        gui.harness.step();
    }
    gui.harness.step();
}

fn release(gui: &mut Gui, at: egui::Pos2) {
    button(gui, false, at);
    gui.run();
}

/// The plot frame of the first panel, from its two axis rulers.
fn plot_frame(gui: &Gui) -> egui::Rect {
    let x = rect(&gui.harness.get_by_label("Horizontal axis ruler"));
    let y = rect(&gui.harness.get_by_label("Vertical axis ruler"));
    egui::Rect::from_min_max(egui::pos2(y.right(), y.top()), egui::pos2(x.right(), x.top()))
}

/// Dragging a column over the panels shows the five zones, names the one
/// under the pointer, and letting go there assigns the column: the centre
/// colours by it, the bottom edge puts it on the horizontal axis.
#[test]
fn columns_dropped_on_the_panels_go_to_the_zone_under_the_pointer() {
    let mut gui = graph();
    // The shelves name the roles too; the zones add a second name each.
    let count = |gui: &Gui, text: &str| gui.painted_text().iter().filter(|t| *t == text).count();
    let on_shelves = count(&gui, "Split across");
    let from = variable(&gui, "Unemployed");
    let centre = plot_frame(&gui).center();
    drag_to(&mut gui, from, centre);
    let painted = gui.painted_text();
    assert_eq!(count(&gui, "Split across"), on_shelves + 1, "the zones are named: {painted:?}");
    assert_eq!(count(&gui, "Split down"), on_shelves + 1);
    assert!(painted.iter().any(|t| t == "Color by Unemployed"), "the zone under the pointer says what it does");
    gui.shot("graph-drop-zones");
    release(&mut gui, centre);
    let spec = saved_spec(&gui);
    assert_eq!(spec.encoding.color.as_deref(), Some("Unemployed"));
    assert_eq!(spec.encoding.x, ["GNP"], "the axes are kept");
    assert_eq!(count(&gui, "Split across"), on_shelves, "the zones go when the drag ends");

    // The horizontal axis's ruler lies in the bottom zone: the column takes
    // the axis's place.
    let from = variable(&gui, "Armed forces");
    let ruler = rect(&gui.harness.get_by_label("Horizontal axis ruler")).center();
    drag_to(&mut gui, from, ruler);
    release(&mut gui, ruler);
    let spec = saved_spec(&gui);
    assert_eq!(spec.encoding.x, ["Armed forces"]);
    assert_eq!(spec.encoding.y, ["Employed"]);
}

/// A column let go over a report plot's axis that shows the report's own
/// column re-runs the report with the new column in its place.
#[test]
fn a_column_dropped_on_a_report_axis_switches_the_report() {
    let mut gui = Gui::with_sample("Plant growth");
    gui.harness.set_size(egui::vec2(1400.0, 1600.0));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let height = gui.app().documents[active].table.find("Height").unwrap();
    let light = gui.app().documents[active].table.find("Light hours").unwrap();
    gui.app_mut().add_report(ReportRequest::Distribution(app_model::DistributionRequest {
        column: height,
        quantile_method: Default::default(),
        censoring: None,
    }));
    gui.results();
    let column = |gui: &Gui| match gui.app().documents[active].reports[0].saved() {
        app_model::SavedReport::Distribution { request, .. } => request.column,
        other => panic!("a distribution report, not {other:?}"),
    };
    let ruler = rect(&gui.harness.get_all_by_label("Horizontal axis ruler").next().expect("the histogram's axis"));
    // Fertilizer is text: it cannot take the place of Height.
    let from = variable(&gui, "Fertilizer");
    drag_to(&mut gui, from, ruler.center());
    assert!(gui.painted_text().iter().any(|t| t == "Fertilizer cannot take the place of Height"));
    release(&mut gui, ruler.center());
    assert_eq!(column(&gui), height);

    let from = variable(&gui, "Light hours");
    drag_to(&mut gui, from, ruler.center());
    assert!(gui.painted_text().iter().any(|t| t == "Analyse Light hours instead of Height"));
    gui.shot("report-axis-drop");
    release(&mut gui, ruler.center());
    assert_eq!(column(&gui), light, "the report now analyses Light hours");
}

/// The report's filter panel lists a categorical column's levels, steps
/// through them one at a time (by hand and with Play) and applies each
/// step to the report.
#[test]
fn report_filter_panel_steps_through_levels() {
    let mut gui = Gui::with_sample("Plant growth");
    gui.harness.set_size(egui::vec2(1400.0, 1600.0));
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let height = gui.app().documents[active].table.find("Height").unwrap();
    gui.app_mut().add_report(ReportRequest::Distribution(app_model::DistributionRequest {
        column: height,
        quantile_method: Default::default(),
        censoring: None,
    }));
    gui.results();
    gui.click("⋯ Options");
    gui.click("Filter rows…");
    gui.click("Filter panel");
    gui.click("+ Add condition");
    let fertilizer = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Fertilizer");
    gui.harness.get_all_by(fertilizer).last().expect("the column in the menu").simulate_click();
    gui.run();
    let filter = |gui: &Gui| gui.app().documents[active].reports[0].scope().filter.clone();
    let levels = |l: &[&str]| {
        vec![app_model::FilterCondition::Levels {
            column: "Fertilizer".into(),
            levels: l.iter().map(|s| (*s).to_owned()).collect(),
        }]
    };
    assert_eq!(filter(&gui), levels(&["A", "B", "C"]));
    assert!(gui.shows("3 of 3 levels"));

    gui.click("Step through Fertilizer");
    assert_eq!(filter(&gui), levels(&["A"]), "stepping starts at the first level");
    assert!(gui.shows("Level 1 of 3: A"));
    assert!(gui.shows("40 of 60 rows filtered out"));
    gui.click("Next ▶");
    assert_eq!(filter(&gui), levels(&["B"]));
    gui.click("◀ Previous");
    gui.click("◀ Previous");
    assert_eq!(filter(&gui), levels(&["C"]), "stepping wraps round");
    gui.shot("report-filter-panel");

    // Play steps on by itself.
    let seconds = gui.harness.get_by_label("Seconds per step").id();
    gui.set_number(seconds, 0.2);
    gui.click("▶ Play");
    for _ in 0..40 {
        gui.harness.step();
    }
    assert_ne!(filter(&gui), levels(&["C"]), "playing moved on");
    gui.click("⏸ Pause");
    let paused = filter(&gui);
    for _ in 0..40 {
        gui.harness.step();
    }
    assert_eq!(filter(&gui), paused, "pausing stops");
    // The panel's edits are saved with the report.
    let saved = gui.app().documents[active].reports[0].saved();
    assert_eq!(saved.scope().map(|s| s.filter.clone()), Some(paused));
}

/// The canvas's filter moves into a panel beside the graph, where a
/// continuous column steps as a moving window.
#[test]
fn canvas_filter_panel_sits_beside_the_graph_and_steps_a_window() {
    let mut gui = graph();
    gui.click("Row filter");
    gui.click("+ Add condition");
    let year = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Year");
    gui.harness.get_all_by(year).last().expect("the column in the menu").simulate_click();
    gui.run();
    let panels_left = plot_frame(&gui).left();
    gui.click("Show beside the graph");
    assert!(gui.shows("Move above the graph"));
    assert!(plot_frame(&gui).left() > panels_left + 200.0, "the panels make room for the filter");
    gui.click("Step through Year");
    let window = |gui: &Gui| match &saved_spec(gui).options.filter.conditions[0] {
        stat_viz::FilterCondition::Range { min, max, .. } => (*min, *max),
        other => panic!("a range condition, not {other:?}"),
    };
    // Longley's years run 1947–1962: a tenth of the range is 1.5 years.
    let (min, max) = window(&gui);
    assert!((min - 1947.0).abs() < 1e-9 && (max - 1948.5).abs() < 1e-9, "{min}–{max}");
    assert!(gui.shows("Window 1 of 10: 1947 ≤ Year ≤ 1948.5"));
    gui.click("Next ▶");
    let (min, max) = window(&gui);
    assert!((min - 1948.5).abs() < 1e-9 && (max - 1950.0).abs() < 1e-9, "{min}–{max}");
    gui.shot("graph-filter-beside");
    gui.click("Move above the graph");
    assert!(gui.shows("Show beside the graph"), "back under the Row filter heading");
}
