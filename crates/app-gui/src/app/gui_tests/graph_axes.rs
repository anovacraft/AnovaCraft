//! Graph canvas: the right axis, axis breaks and math in titles.

use super::*;

/// Longley with its own reports closed and a graph canvas of Employed
/// against GNP, changed by `change` before it is drawn.
fn graph_with(change: impl FnOnce(&mut stat_viz::GraphSpec)) -> Gui {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let mut spec = stat_viz::GraphSpec {
        encoding: stat_viz::EncodingPlan { x: vec!["GNP".into()], y: vec!["Employed".into()], ..Default::default() },
        points: Some(stat_viz::PointsElement::default()),
        ..stat_viz::GraphSpec::default()
    };
    change(&mut spec);
    gui.app_mut().add_report(ReportRequest::Graph(Box::new(spec)));
    gui.run();
    gui
}

/// The graph's settings as the report would save them.
fn saved_spec(gui: &Gui) -> stat_viz::GraphSpec {
    let doc = &gui.app().documents[gui.app().active];
    let app_model::SavedReport::Graph { spec } = doc.reports[0].saved() else { panic!("a graph report") };
    *spec
}

fn rect(node: &Node<'_>) -> egui::Rect {
    let b = node.raw_bounds().expect("the control has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

fn rect_of(gui: &Gui, label: &str) -> egui::Rect {
    rect(&gui.harness.get_by_label(label))
}

/// Texts painted right of the plot frame (the right axis's tick labels).
fn right_tick_labels(gui: &Gui) -> Vec<String> {
    let ruler = rect_of(gui, "Right axis ruler");
    let mut out: Vec<String> = gui
        .painted_text_rects()
        .into_iter()
        .filter(|(_, r, _)| r.center().x > ruler.left() && r.center().x < ruler.right() + 40.0)
        .filter(|(_, r, _)| r.center().y > ruler.top() && r.center().y < ruler.bottom())
        .map(|(t, ..)| t)
        .filter(|t| t.chars().any(|c| c.is_ascii_digit()))
        .collect();
    out.sort();
    out
}

/// Without right-axis columns nothing about the graph changes: no right
/// ruler, no axis names in the key.
#[test]
fn graph_without_right_axis_is_unchanged() {
    let gui = graph_with(|_| {});
    assert!(!gui.shows("Right axis ruler"));
    assert!(!gui.shows("Employed (left axis)"));
    assert!(saved_spec(&gui).axes.is_default(), "no axis settings are stored");
}

/// A column chosen for the Right axis shelf gets a second axis with its own
/// ticks, and the key says which series uses which axis.
#[test]
fn right_axis_shelf_adds_a_second_axis() {
    let mut gui = graph_with(|_| {});
    let shelf = gui
        .painted_text_rects()
        .into_iter()
        .find(|(t, ..)| t == "Right axis")
        .map(|(_, r, _)| r)
        .expect("the Right axis shelf");
    let plus = gui
        .harness
        .get_all_by(|n| n.label().as_deref() == Some("+") && n.role() != Role::Label)
        .map(|n| (n.id(), rect(&n)))
        .filter(|(_, r)| (r.center().y - shelf.center().y).abs() < 8.0 && r.left() > shelf.right())
        .min_by(|a, b| a.1.left().total_cmp(&b.1.left()))
        .expect("the shelf's + button")
        .0;
    gui.harness.get_by(|n| n.id() == plus).simulate_click();
    gui.run();
    gui.click("Population 14+");
    assert_eq!(saved_spec(&gui).encoding.y2, vec!["Population 14+".to_owned()]);
    gui.shot("graph-right-axis");
    assert!(gui.shows("Right axis ruler"));
    assert!(gui.shows("Employed (left axis)"));
    assert!(gui.shows("Population 14+ (right axis)"));
    assert!(!right_tick_labels(&gui).is_empty(), "the right axis has tick labels");
}

/// The right axis drags like the left one: from its middle it shifts (its
/// tick labels change while the left axis stays), and a double-click
/// re-fits it.
#[test]
fn right_axis_drags_as_a_ruler() {
    let mut gui = graph_with(|s| s.encoding.y2 = vec!["Population 14+".into()]);
    let before = right_tick_labels(&gui);
    let left_before = saved_spec(&gui);
    let ruler = rect_of(&gui, "Right axis ruler");
    gui.drag(ruler.center(), ruler.center() + egui::vec2(0.0, 0.25 * ruler.height()));
    let after = right_tick_labels(&gui);
    assert_ne!(before, after, "the right axis moved");
    assert_eq!(saved_spec(&gui), left_before, "a drag is a view change, not a setting");
    // Double-click re-fits.
    let at = rect_of(&gui, "Right axis ruler").center();
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    // Each step advances the clock a quarter second: one click per step
    // keeps the two within the double-click delay.
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for _ in 0..2 {
        gui.harness.input_mut().events.extend([button(true), button(false)]);
        gui.harness.step();
    }
    gui.run();
    assert_eq!(right_tick_labels(&gui), before, "double-click re-fits the right axis");
}

/// Right-axis points are selected by a box like any other: with the left
/// axis moved away from its data, a box over the whole plot selects the
/// rows through their right-axis points.
#[test]
fn right_axis_points_can_be_selected() {
    let mut gui = graph_with(|s| s.encoding.y2 = vec!["Population 14+".into()]);
    gui.click("Plot settings ▾");
    let is_range = |n: &Node<'_>| n.role() == Role::SpinButton && n.value().is_some_and(|v| v.starts_with("max "));
    let y_max = gui.harness.get_all_by(is_range).nth(1).expect("y max").id();
    let is_min = |n: &Node<'_>| n.role() == Role::SpinButton && n.value().is_some_and(|v| v.starts_with("min "));
    let y_min = gui.harness.get_all_by(is_min).nth(1).expect("y min").id();
    gui.set_number(y_min, 0.0);
    gui.set_number(y_max, 1.0);
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let (x, y) = (rect_of(&gui, "Horizontal axis ruler"), rect_of(&gui, "Vertical axis ruler"));
    gui.drag(egui::pos2(y.right() + 4.0, y.top() + 4.0), egui::pos2(x.right() - 4.0, x.top() - 4.0));
    let selected = gui.app().documents[gui.app().active].rows.counts().selected;
    assert_eq!(selected, 16, "every row, through its right-axis point");
}

/// Plot settings ▸ Axis break ▸ Leave out takes a stretch out of the
/// vertical axis: points inside it are not drawn (a note counts them), the
/// break is saved with the graph, and clearing it restores the graph.
#[test]
fn axis_break_leaves_a_stretch_out() {
    let mut gui = graph_with(|_| {});
    gui.click("Plot settings ▾");
    let boxes: Vec<_> = gui.harness.get_all_by_label("Leave out").map(|n| n.id()).collect();
    assert_eq!(boxes.len(), 2, "a break for each continuous axis");
    gui.harness.get_by(|n| n.id() == boxes[1]).click();
    gui.run();
    let brk = *saved_spec(&gui).axes.y_breaks.breaks.first().expect("a vertical break");
    assert!(brk.from < brk.to && brk.from > 60_000.0 && brk.to < 71_000.0, "{brk:?}");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    gui.shot("graph-axis-break");
    // The note sits below the graph, perhaps out of sight: look for its widget.
    let note = |gui: &Gui| {
        gui.harness
            .query_all_by(|n| {
                [n.label(), n.value()].into_iter().flatten().any(|t| t.contains("inside an axis break are not drawn"))
            })
            .next()
            .is_some()
    };
    assert!(note(&gui));
    gui.click("Plot settings ▾");
    let boxes: Vec<_> = gui.harness.get_all_by_label("Leave out").map(|n| n.id()).collect();
    gui.harness.get_by(|n| n.id() == boxes[1]).click();
    gui.run();
    assert!(saved_spec(&gui).axes.is_default());
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert!(!note(&gui));
}

/// Plot settings ▸ Axis breaks: *Add a break* leaves a second stretch out,
/// the first segment goes on a log scale (a note under the graph says the
/// scales differ), the marks become zigzags, and all of it is saved with the
/// graph; removing the second break keeps the first.
#[test]
fn several_breaks_with_segment_scales() {
    let mut gui = graph_with(|_| {});
    gui.click("Plot settings ▾");
    let boxes: Vec<_> = gui.harness.get_all_by_label("Leave out").map(|n| n.id()).collect();
    gui.harness.get_by(|n| n.id() == boxes[1]).click();
    gui.run();
    gui.press("Add a break");
    let breaks = saved_spec(&gui).axes.y_breaks;
    assert_eq!(breaks.breaks.len(), 2, "{breaks:?}");
    assert!(breaks.breaks[0].to < breaks.breaks[1].from, "in axis order: {breaks:?}");
    let header = |n: &Node<'_>| n.role() != Role::Label && n.label().as_deref() == Some("Segments and marks");
    gui.harness.get_by(header).click();
    gui.run();
    gui.press("Zigzag");
    gui.widget("Scale of segment 1").click();
    gui.run();
    let entry = |n: &Node<'_>| n.role() != Role::Label && n.label().as_deref() == Some("Log (base 10)");
    gui.harness.get_by(entry).click();
    gui.run();
    let breaks = saved_spec(&gui).axes.y_breaks;
    assert_eq!(breaks.mark, stat_viz::BreakMark::Zigzag);
    assert_eq!(breaks.segment(0).scale, Some(stat_viz::AxisScale::Log10), "{breaks:?}");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    gui.shot("graph-axis-breaks");
    let note = gui.harness.query_all_by(|n| {
        [n.label(), n.value()].into_iter().flatten().any(|t| t.contains("broken into segments on different scales"))
    });
    assert!(note.count() > 0, "the note names the scales");
    // The graph file keeps every setting.
    let spec = saved_spec(&gui);
    let text = app_model::graph_json::pipeline_text(&spec);
    assert_eq!(app_model::graph_json::pipeline_from_text(&text).unwrap().axes.y_breaks, spec.axes.y_breaks);
    gui.click("Plot settings ▾");
    gui.press("Remove break 2");
    assert_eq!(saved_spec(&gui).axes.y_breaks.breaks.len(), 1);
}

/// A title with math markup is set with scripts (named by its plain text),
/// never shown as the raw markup.
#[test]
fn markup_title_is_set_with_scripts() {
    let gui = graph_with(|s| s.axes.title = Some(r"R_{adj}^2 by \alpha".into()));
    assert!(gui.shows("R_{adj}^2 by α"), "the title is named by its plain text");
    assert!(!gui.painted_text().iter().any(|t| t.contains(r"\alpha") || t.contains("_{")));
}

/// A graph keeps its plot style: a graph without one saves none, and a
/// graph opened with one (as from a project or graph file) draws with it
/// and saves it unchanged, titles and breaks staying in their own place.
#[test]
fn graph_keeps_its_plot_style() {
    use app_model::axis_settings::{GridLines, MinorTicks, TickFormat};
    let gui = graph_with(|_| {});
    assert_eq!(saved_spec(&gui).axes.style, None, "an untouched graph saves no style");

    let mut style = app_model::plot_style::PlotStyle::default();
    style.axes.y.grid = GridLines::MajorAndMinor;
    style.axes.y.minor = MinorTicks::Subdivisions(4);
    style.axes.x.format = TickFormat::Decimals(1);
    style.axes.show_key = false;
    let gui = graph_with(|spec| {
        spec.axes.style = Some(style.clone());
        spec.axes.x_title = Some("GNP (billions)".into());
    });
    let saved = saved_spec(&gui);
    assert_eq!(saved.axes.style, Some(style));
    assert_eq!(saved.axes.x_title.as_deref(), Some("GNP (billions)"));
    assert!(gui.painted_text_rects().iter().any(|(t, ..)| t.contains("GNP (billions)")), "the title is drawn");
    let text = app_model::graph_json::pipeline_text(&saved);
    assert_eq!(app_model::graph_json::pipeline_from_text(&text).unwrap(), saved);
}
