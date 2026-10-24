//! The look of plot axes: tick direction and size, axis lines moved away
//! from the data, tick labels written by the user, typed ranges and the
//! equal-scale ratio.

use super::*;

/// Longley with one bivariate report: GNP (x) against Employed (y).
fn longley_plot() -> (Gui, usize) {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(2, 0)));
    gui.run();
    (gui, active)
}

/// The screen rectangle of the control labelled `label`.
fn rect_of(gui: &Gui, label: &str) -> egui::Rect {
    let b = gui.widget(label).raw_bounds().expect("the control has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

fn close_menus(gui: &mut Gui) {
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
}

/// The first number field whose shown value starts with `prefix`.
fn number_field(gui: &Gui, prefix: &str) -> egui::accesskit::NodeId {
    gui.harness
        .query_all_by(|n| n.role() == Role::SpinButton && n.value().is_some_and(|v| v.starts_with(prefix)))
        .next()
        .unwrap_or_else(|| panic!("no field {prefix:?}"))
        .id()
}

/// Open the first (horizontal axis) group named `name` in Plot settings.
fn open_group(gui: &mut Gui, name: &str) {
    let header = |n: &Node<'_>| n.role() != Role::Label && n.label().as_deref() == Some(name);
    gui.harness.get_all_by(header).next().expect("the group").click();
    gui.run();
}

/// Numeric tick labels painted under `below` (screen y): (value, ink rect).
fn x_tick_labels(gui: &Gui, below: f32) -> Vec<(f64, egui::Rect)> {
    gui.painted_text_rects()
        .into_iter()
        .filter_map(|(t, r, _)| t.trim().replace(',', "").parse::<f64>().ok().map(|v| (v, r)))
        .filter(|(_, r)| r.top() >= below)
        .collect()
}

fn double_click(gui: &mut Gui, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    // Each step advances the clock a quarter second: one click per step
    // keeps the two within the double-click delay.
    for _ in 0..2 {
        gui.harness.input_mut().events.extend([button(true), button(false)]);
        gui.harness.step();
    }
    gui.run();
}

/// Plot settings ▸ Spine ▸ Offset from data moves the horizontal axis line
/// down, and its draggable ruler and tick labels with it; Ticks ▸ Outside
/// moves the labels further, past the marks.
#[test]
fn offset_axis_moves_its_ruler_and_labels() {
    let (mut gui, active) = longley_plot();
    let ruler = rect_of(&gui, "Horizontal axis ruler");
    // The vertical ruler ends at the plot frame's bottom edge.
    let frame_bottom = |gui: &Gui| rect_of(gui, "Vertical axis ruler").bottom();
    let before = x_tick_labels(&gui, ruler.top() - 1.0);
    assert!(!before.is_empty(), "GNP tick labels under the plot");
    let bottom_before = frame_bottom(&gui);
    gui.click("Plot settings ▾");
    open_group(&mut gui, "Spine");
    let offset = number_field(&gui, "offset ");
    gui.set_number(offset, 12.0);
    close_menus(&mut gui);
    let moved = rect_of(&gui, "Horizontal axis ruler");
    let bottom = frame_bottom(&gui);
    assert!((moved.top() - bottom - 12.0).abs() < 0.6, "the ruler follows the line: {moved:?}, frame at {bottom}");
    let after = x_tick_labels(&gui, bottom);
    // How much further below the frame a label sits than before.
    let shift = |b: &[(f64, egui::Rect)], bottom: f32| {
        let (v, r) = before[0];
        b.iter().find(|(w, _)| *w == v).map(|(_, s)| (s.top() - bottom) - (r.top() - bottom_before)).expect("the same label")
    };
    assert!((shift(&after, bottom) - 12.0).abs() < 0.6, "labels move down with the line");
    gui.shot("axis-offset");
    // Outside marks 5 points long move the labels 5 further.
    gui.app_mut().documents[active].reports[0]
        .plot_axes_mut(app_model::report::SectionKind::BivariateScatter)
        .expect("the scatter plot")
        .x
        .tick_direction = app_model::axis_settings::TickDirection::Outside;
    gui.run();
    gui.run();
    let bottom = frame_bottom(&gui);
    let outside = x_tick_labels(&gui, bottom);
    assert!((shift(&outside, bottom) - 17.0).abs() < 0.6, "labels clear outside marks");
    // Kept with the report.
    let scope = gui.app().documents[active].reports[0].scope().clone();
    let style = &scope.plot_styles.first().expect("a changed style").1;
    assert_eq!(style.axes.x.offset, 12.0);
}

/// Double-clicking a tick label opens an editor in place; the label written
/// there (math markup) replaces the number, and "Use the number" brings the
/// number back. "Add a tick at…" in Plot settings adds a row.
#[test]
fn tick_labels_are_written_in_place() {
    let (mut gui, active) = longley_plot();
    let ruler = rect_of(&gui, "Horizontal axis ruler");
    let labels = x_tick_labels(&gui, ruler.top() - 1.0);
    let (value, at) = labels[labels.len() / 2];
    double_click(&mut gui, egui::pos2(at.center().x, ruler.center().y));
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Tick label at")), "the editor opens: {text:?}");
    gui.harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    gui.harness.input_mut().events.push(egui::Event::Text("\\alpha_0".into()));
    gui.run();
    gui.click("Apply");
    assert!(!gui.shows("Tick label"), "the editor closes");
    let kind = app_model::report::SectionKind::BivariateScatter;
    let axes = gui.app_mut().documents[active].reports[0].plot_axes_mut(kind).unwrap().clone();
    assert_eq!(axes.x.custom_ticks.len(), 1);
    assert!((axes.x.custom_ticks[0].value - value).abs() < 1e-6 * value.abs().max(1.0));
    assert_eq!(axes.x.custom_ticks[0].label, "\\alpha_0");
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.contains('α')), "set as markup: {text:?}");
    assert!(!x_tick_labels(&gui, ruler.top() - 1.0).iter().any(|(v, _)| *v == value), "the number is replaced");
    gui.shot("tick-label-written");
    // Back to the number.
    double_click(&mut gui, egui::pos2(at.center().x, ruler.center().y));
    gui.click("Use the number");
    let axes = gui.app_mut().documents[active].reports[0].plot_axes_mut(kind).unwrap().clone();
    assert!(axes.x.custom_ticks.is_empty());
    // Plot settings ▸ Ticks ▸ Add a tick at…
    gui.click("Plot settings ▾");
    open_group(&mut gui, "Ticks");
    gui.press("Add a tick at…");
    let axes = gui.app_mut().documents[active].reports[0].plot_axes_mut(kind).unwrap().clone();
    assert_eq!(axes.x.custom_ticks.len(), 1, "a labelled tick added");
}

/// A range typed in Plot settings is kept with the report (and reopens);
/// Fit view to data lets the axis fit the data again.
#[test]
fn typed_ranges_are_kept() {
    let (mut gui, active) = longley_plot();
    gui.click("Plot settings ▾");
    let x_min = number_field(&gui, "min ");
    gui.set_number(x_min, 200_000.0);
    assert!(gui.shows("Automatic"), "the axis can go back to automatic");
    close_menus(&mut gui);
    let doc = &gui.app().documents[active];
    let style = doc.reports[0].scope().plot_styles.first().expect("kept").1.clone();
    let [lo, _] = style.axes.x.range.expect("the typed range");
    assert_eq!(lo, 200_000.0);
    // Saved and read back; a report made from it shows the range.
    let saved = doc.reports[0].saved();
    let json = app_model::project::report_to_json(&saved, &doc.table);
    let back = app_model::project::report_from_json(&json, &doc.table).unwrap();
    assert_eq!(back, saved);
    let (table, rows) = (doc.table.clone(), doc.rows.clone());
    let copy = crate::report_view::Report::new(9_999, back, &table, &rows);
    gui.app_mut().documents[active].reports = vec![copy];
    gui.run();
    gui.run();
    gui.click("Plot settings ▾");
    let shown = gui.harness.get_by(|n| n.id() == number_field(&gui, "min ")).numeric_value().unwrap();
    assert!((shown - 200_000.0).abs() < 1e-6, "the reopened plot shows the typed range: {shown}");
    gui.press("Fit view to data");
    let kind = app_model::report::SectionKind::BivariateScatter;
    let axes = gui.app_mut().documents[active].reports[0].plot_axes_mut(kind).unwrap().clone();
    assert_eq!(axes.x.range, None);
}

/// Report scatter plots take axis breaks: Plot settings ▸ Leave out on the
/// vertical axis leaves the middle third out (points there are not drawn, a
/// note counts them), the tick labels come from both pieces, and the break is
/// kept with the report through the project file.
#[test]
fn report_plots_take_axis_breaks() {
    let (mut gui, active) = longley_plot();
    gui.click("Plot settings ▾");
    let boxes: Vec<_> = gui.harness.get_all_by_label("Leave out").map(|n| n.id()).collect();
    assert_eq!(boxes.len(), 2, "both continuous axes take breaks");
    gui.harness.get_by(|n| n.id() == boxes[1]).click();
    gui.run();
    close_menus(&mut gui);
    gui.run();
    gui.shot("report-axis-break");
    let doc = &gui.app().documents[active];
    let style = doc.reports[0].scope().plot_styles.first().expect("kept").1.clone();
    let brk = *style.axes.y.breaks.breaks.first().expect("a vertical break");
    assert!(brk.from > 60_000.0 && brk.to < 71_000.0, "{brk:?}");
    assert!(
        gui.painted_text().iter().any(|t| t.contains("inside an axis break are not drawn")),
        "the note counts the hidden points"
    );
    // Tick labels below and above the break.
    let labels: Vec<f64> =
        gui.painted_text().iter().filter_map(|t| t.trim().replace(',', "").parse::<f64>().ok()).collect();
    assert!(labels.iter().any(|v| *v < brk.from) && labels.iter().any(|v| *v > brk.to), "{labels:?}");
    let saved = doc.reports[0].saved();
    let json = app_model::project::report_to_json(&saved, &doc.table);
    let back = app_model::project::report_from_json(&json, &doc.table).unwrap();
    assert_eq!(back, saved);
    // Clearing it restores the plot.
    gui.click("Plot settings ▾");
    let boxes: Vec<_> = gui.harness.get_all_by_label("Leave out").map(|n| n.id()).collect();
    gui.harness.get_by(|n| n.id() == boxes[1]).click();
    gui.run();
    close_menus(&mut gui);
    assert!(!gui.painted_text().iter().any(|t| t.contains("inside an axis break")));
}

/// At equal scale with a ratio of 2, one horizontal unit is drawn as long as
/// two vertical units.
#[test]
fn equal_scale_ratio_is_kept_on_screen() {
    let (mut gui, active) = longley_plot();
    let kind = app_model::report::SectionKind::BivariateScatter;
    {
        let axes = gui.app_mut().documents[active].reports[0].plot_axes_mut(kind).unwrap();
        axes.equal_scale = true;
        axes.scale_ratio = 2.0;
    }
    gui.run();
    gui.run();
    gui.click("Plot settings ▾");
    let value = |p: &str| gui.harness.get_by(|n| n.id() == number_field(&gui, p)).numeric_value().unwrap();
    let (x0, x1) = (value("min "), value("max "));
    let fields: Vec<f64> = gui
        .harness
        .query_all_by(|n| n.role() == Role::SpinButton && n.value().is_some_and(|v| v.starts_with("min ") || v.starts_with("max ")))
        .map(|n| n.numeric_value().unwrap())
        .collect();
    let (y0, y1) = (fields[2], fields[3]);
    close_menus(&mut gui);
    let (x, y) = (rect_of(&gui, "Horizontal axis ruler"), rect_of(&gui, "Vertical axis ruler"));
    let x_per_point = (x1 - x0) / f64::from(x.width());
    let y_per_point = (y1 - y0) / f64::from(y.height());
    assert!((x_per_point / y_per_point - 0.5).abs() < 0.02, "{x_per_point} / {y_per_point}");
}
