//! Keyboard edits reach the table the main window shows, long cell text is
//! edited in a larger box, an axis limit dragged at equal scale stays where
//! it is dragged, crowded category labels turn rather than disappear, and
//! removing a dashboard asks first and can be undone.

use super::*;
use app_model::{SectionContent, SectionKind};

/// Press and release the primary button at `pos`.
fn press_at(gui: &mut Gui, pos: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(pos));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(true));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(false));
    gui.run();
}

/// Press `key` with `modifiers`, then let the UI settle.
fn key(gui: &mut Gui, key: egui::Key, modifiers: egui::Modifiers) {
    gui.harness.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    gui.run();
}

/// Type `text` as the keyboard would.
fn type_text(gui: &mut Gui, text: &str) {
    gui.harness.input_mut().events.push(egui::Event::Text(text.to_owned()));
    gui.run();
}

fn rect(node: &Node<'_>) -> egui::Rect {
    let b = node.raw_bounds().expect("the node has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// Every numeric column of document `index`, whole.
fn numbers(gui: &Gui, index: usize) -> Vec<Vec<f64>> {
    let table = &gui.app().documents[index].table;
    table.columns().iter().filter_map(|c| c.as_numeric().map(<[f64]>::to_vec)).collect()
}

/// After a pop-out window of another table was used last, typing in the main
/// window's grid changes the table it shows, not that other one. (The grid
/// used to send its edits to the command target, which the menu bar leaves
/// on the window used before.)
#[test]
fn typing_in_the_main_grid_edits_the_table_it_shows() {
    let mut gui = Gui::with_sample("Longley");
    let a = gui.app().shown;
    gui.perform(Action::OpenSample(1));
    let b = gui.app().shown;
    let b_name = gui.app().documents[b].name().to_owned();
    gui.app_mut().documents[b].workspace = Workspace::Data;
    gui.run();
    gui.click("New window");
    gui.perform(Action::Activate(a));
    gui.app_mut().documents[a].workspace = Workspace::Data;
    gui.run();
    // Use the other table's window, then come back through the menu bar.
    let window = rect(&gui.window(&format!("{b_name} · Data · AnovaCraft")));
    press_at(&mut gui, egui::pos2(window.left() + 30.0, window.top() + 40.0));
    assert_eq!(gui.app().active, b);
    let help = gui
        .harness
        .get_all_by(|n| n.role() == Role::Button && n.label().as_deref() == Some("Help"))
        .next()
        .map(|n| rect(&n))
        .expect("the Help menu");
    press_at(&mut gui, egui::pos2(help.right() + 120.0, help.center().y));
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert_eq!((gui.app().shown, gui.app().active), (a, b), "the menu bar leaves the target alone");

    let (a_before, b_before) = (numbers(&gui, a), numbers(&gui, b));
    // The cursor on the first cell of the shown table's first column; type 42, Enter.
    gui.app_mut().documents[a].grid.focus_column(0);
    gui.run();
    type_text(&mut gui, "42");
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    let a_after = numbers(&gui, a);
    assert_eq!(a_after[0][0], 42.0, "the shown table changed");
    assert_eq!(a_after[0][1..], a_before[0][1..]);
    assert_eq!(a_after[1..], a_before[1..]);
    assert_eq!(numbers(&gui, b), b_before, "the other table is untouched");
    assert_eq!(gui.app().active, a, "the edited table is now the target");
}

/// Text wider than its cell is edited in a box over the grid that shows all
/// of it; Enter keeps it as it was, Shift+Enter starts a new line, and a
/// short value keeps the editor inside its cell.
#[test]
fn long_cell_text_is_edited_in_a_larger_box() {
    let mut gui = Gui::with_sample("Espresso machine reviews");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().documents[active].workspace = Workspace::Data;
    gui.run();
    let column = gui.app().documents[active].table.find("Review text").expect("the review column");
    let long = "The crema was thick and lasted a long time, the steam wand heated milk quickly, and cleaning \
                took only a minute after each use; the drip tray is small though, the water tank hard to fill \
                under a cupboard, and the grinder settings are not marked clearly enough to repeat a good shot."
        .to_owned();
    assert!(long.len() > 280);
    gui.grid(vec![
        GridRequest::SetCell { row: 0, column, text: long.clone() },
        GridRequest::SetCell { row: 1, column, text: "Good".to_owned() },
    ]);
    gui.app_mut().documents[active].grid.focus_column(column);
    gui.run();
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    // The sidebar's project notes are a multi-line field too: find the one holding the text.
    let editing = |gui: &Gui, role: Role, text: &str| {
        gui.harness.query_all_by_role(role).filter(|n| n.value().as_deref() == Some(text)).map(|n| rect(&n)).next()
    };
    let r = editing(&gui, Role::MultilineTextInput, &long).expect("the larger box with the whole text");
    // Wider than any column starts (180 points at most), wrapped onto several lines.
    assert!(r.width() > 400.0 && r.height() > 40.0, "{r:?}");
    gui.shot("grid-expanded-cell-editor");
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    assert!(!gui.app().documents[active].grid.is_editing(), "Enter ends the edit");
    assert!(editing(&gui, Role::MultilineTextInput, &long).is_none());
    let cell = |gui: &Gui, row| gui.app().documents[active].table.column(column).unwrap().cell_text(row);
    assert_eq!(cell(&gui, 0), long);
    assert_eq!(gui.app().documents[active].grid.cursor(), Some((1, column)), "and moves down");

    // A short value keeps the editor in its cell.
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    assert!(editing(&gui, Role::MultilineTextInput, "Good").is_none());
    let short = editing(&gui, Role::TextInput, "Good").expect("the in-cell field");
    assert!(short.width() < 200.0, "{short:?}");
    key(&mut gui, egui::Key::Escape, egui::Modifiers::NONE);

    // Shift+Enter starts a new line in the box; Enter stores it.
    key(&mut gui, egui::Key::ArrowUp, egui::Modifiers::NONE);
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    key(&mut gui, egui::Key::Enter, egui::Modifiers::SHIFT);
    type_text(&mut gui, "Second line");
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(cell(&gui, 0), format!("{long}\nSecond line"));
}

/// At equal scale (as Observed vs Predicted and score plots open), dragging
/// the top end of the vertical axis moves only its upper limit; the
/// horizontal axis is rescaled about its centre to keep one unit the same
/// length. (Pulling the limit in used to shift the whole vertical axis, as
/// the plot restored the aspect by widening the vertical range again.)
#[test]
fn equal_scale_edge_drag_moves_only_that_limit() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(2, 0)));
    gui.run();
    gui.click("Plot settings ▾");
    gui.press("Equal scale (one unit is the same length on both axes)");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let view = |gui: &Gui| {
        gui.app().documents[active].reports[0].plot_view(SectionKind::BivariateScatter).and_then(|v| v.0).unwrap()
    };
    let before = view(&gui);
    let y = rect(&gui.widget("Vertical axis ruler"));
    let x = y.center().x;
    // Grab the top end and push it up, so the upper limit comes down to meet the pointer.
    gui.drag(egui::pos2(x, y.top() + 0.15 * y.height()), egui::pos2(x, y.top() + 0.02 * y.height()));
    let after = view(&gui);
    let span = before.y_max - before.y_min;
    assert!((after.y_min - before.y_min).abs() < 1e-6 * span, "the lower limit stays: {before:?} → {after:?}");
    assert!(after.y_max < before.y_max - 0.05 * span, "the upper limit came down: {before:?} → {after:?}");
    let centre = |lo: f64, hi: f64| 0.5 * (lo + hi);
    let x_tol = 1e-6 * (before.x_max - before.x_min);
    assert!((centre(after.x_min, after.x_max) - centre(before.x_min, before.x_max)).abs() < x_tol);
    assert!(after.x_max - after.x_min < before.x_max - before.x_min, "x narrowed to keep the scale");
}

/// Texts painted turned (rotated), with where each starts.
fn turned_text(gui: &Gui) -> Vec<(String, egui::Pos2)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Text(t) if t.angle != 0.0 => out.push((t.galley.text().to_owned(), t.pos)),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &gui.harness.output().shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

/// A word-frequency chart of 40 terms turns its labels so every term is
/// written under the plot (they used to be thinned to every k-th one), and
/// the exported figure turns them the same way; Horizontal keeps them level.
#[test]
fn crowded_category_labels_turn_instead_of_disappearing() {
    let mut gui = Gui::with_sample("Espresso machine reviews");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    let column = gui.app().documents[active].table.find("Review text").unwrap();
    let request = app_model::text_analysis::TextExplorationRequest {
        chart_terms: 40,
        ..app_model::text_analysis::TextExplorationRequest::new(column)
    };
    gui.app_mut().add_report(ReportRequest::TextExploration(request));
    gui.harness.set_size(egui::vec2(1360.0, 3000.0));
    gui.results();
    gui.run();
    let report = &gui.app().documents[active].reports[0];
    let sections = report.for_export().sections.unwrap();
    let chart = sections.iter().find(|s| s.kind == SectionKind::WordFrequencyChart).expect("the chart").clone();
    let SectionContent::Bars(bars) = &chart.content else { panic!("a bar chart") };
    let terms: Vec<String> = bars.x_ticks.clone().unwrap().into_iter().map(|t| t.1).collect();
    assert_eq!(terms.len(), 40);
    let (_, frame) = report.plot_view(SectionKind::WordFrequencyChart).unwrap();
    let frame = frame.expect("drawn");
    let turned = turned_text(&gui);
    for term in &terms {
        let at = turned.iter().find(|(t, _)| t == term).unwrap_or_else(|| panic!("{term} is not written: {turned:?}"));
        assert!(at.1.y > frame.bottom(), "{term} sits under the plot");
    }
    gui.shot("word-frequencies-turned-labels");

    // The exported figure turns every term too.
    let layout = app_model::report_figure::FigureLayout::default();
    let svg = app_model::report_figure::section_figure_with(&chart, None, stat_viz::export::VectorFormat::Svg, layout)
        .unwrap();
    let svg = String::from_utf8(svg).unwrap();
    for term in &terms {
        let line = svg.lines().find(|l| l.ends_with(&format!(">{term}</text>"))).unwrap_or_else(|| panic!("{term}"));
        assert!(line.contains("rotate(-45)") || line.contains("rotate(-90)"), "{line}");
    }

    // Horizontal keeps the labels level.
    gui.app_mut().documents[active].reports[0]
        .plot_axes_mut(SectionKind::WordFrequencyChart)
        .unwrap()
        .category_labels = stat_viz::CategoryLabels::Horizontal;
    gui.run();
    assert!(turned_text(&gui).iter().all(|(t, _)| !terms.contains(t)), "no turned term labels");
}

/// Removing a dashboard asks first (Keep is the default); once removed,
/// Edit ▸ Undo brings it back with its tiles.
#[test]
fn removing_a_dashboard_asks_first_and_undo_brings_it_back() {
    let mut gui = Gui::start();
    gui.harness.set_size(egui::vec2(1360.0, 1400.0));
    gui.perform(Action::OpenProjectSample(0));
    let id = gui.app().shown_dashboard().expect("the sample shows its dashboard");
    let tiles = |gui: &Gui| gui.app().dashboard(id).map(|d| d.dashboard.tiles.clone());
    let before = tiles(&gui).unwrap();
    assert_eq!(before.len(), 6);

    gui.perform(Action::RemoveDashboard(id));
    assert!(gui.shows("Remove dashboard “Greenhouse overview” from the project?"));
    gui.shot("dashboard-remove-question");
    gui.click("Keep");
    assert_eq!(tiles(&gui).as_ref(), Some(&before), "Keep leaves it as it was");
    // Escape keeps it too.
    gui.perform(Action::RemoveDashboard(id));
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert!(tiles(&gui).is_some());

    gui.perform(Action::RemoveDashboard(id));
    gui.click("Remove");
    assert!(tiles(&gui).is_none(), "removed");
    assert_eq!(gui.app().shown_dashboard(), None);
    gui.menu(&["Edit", "Undo remove dashboard “Greenhouse overview”"]);
    assert_eq!(tiles(&gui).as_ref(), Some(&before), "back with its tiles");
    assert_eq!(gui.app().shown_dashboard(), Some(id), "and shown again");
}
