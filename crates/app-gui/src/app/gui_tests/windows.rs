//! Views in windows of their own: the data grid, the results pane and
//! single reports. In these tests egui draws each window inside the main one
//! (as an ordinary egui window) through the same code as a native window.

use super::*;

/// The document the main window shows.
fn shown(gui: &Gui) -> &Document {
    &gui.app().documents[gui.app().shown]
}

/// The title of the window of view `view` of the document the main window shows.
fn title(gui: &Gui, view: View) -> String {
    let doc = shown(gui);
    let what = match view {
        View::Data => "Data".to_owned(),
        View::Results => "Results".to_owned(),
        View::Report(id) => doc.reports.iter().find(|r| r.id() == id).unwrap().title(),
        View::Dashboard(_) => unreachable!("not a table's view"),
    };
    format!("{} · {what} · AnovaCraft", doc.name())
}

/// Where the control or window node `node` is on screen.
fn rect(node: &Node<'_>) -> egui::Rect {
    let b = node.raw_bounds().expect("the node has a position");
    egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

/// Press and release the primary button at `pos` (a plain click on whatever is there).
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

/// Click in an empty part of window `title`: its bar, left of "Back to main window".
fn use_window(gui: &mut Gui, title: &str) {
    let r = rect(&gui.window(title));
    // Below the title bar, at the left of the window's own button bar.
    press_at(gui, egui::pos2(r.left() + 30.0, r.top() + 40.0));
}

/// Longley with one report: a bivariate fit of Employed against GNP (one plot).
fn longley_bivariate() -> Gui {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(2, 0)));
    gui.run();
    gui
}

/// The only report's id in the document the main window shows.
fn report_id(gui: &Gui) -> u64 {
    shown(gui).reports[0].id()
}

/// The data grid opens in its own window from "New window" and comes back
/// from the main window's panel, from "Back to main window" and from the
/// window's close button.
#[test]
fn data_grid_pops_out_and_comes_back() {
    let mut gui = Gui::with_sample("Longley");
    let shown_index = gui.app().shown;
    gui.app_mut().documents[shown_index].workspace = Workspace::Data;
    gui.run();
    let id = shown(&gui).id;
    gui.click("New window");
    assert!(gui.app().in_window(id, View::Data));
    let data_title = title(&gui, View::Data);
    assert!(data_title.starts_with("Longley") && data_title.ends_with(" · Data · AnovaCraft"), "{data_title}");
    let _ = gui.window(&data_title);
    assert!(gui.shows("This data grid is in its own window"));
    // The dataset stays in the list and the tabs, marked as having a window.
    assert_eq!(gui.harness.query_all_by_label("In its own window").count(), 2, "sidebar and tab marks");
    gui.shot("windows-data-grid");

    gui.press("Show window");
    assert!(gui.app().in_window(id, View::Data), "Show window only brings it forward");
    gui.press("Bring back here");
    assert!(!gui.app().in_window(id, View::Data));
    assert!(!gui.shows("This data grid is in its own window"));
    assert_eq!(gui.harness.query_all_by_label("In its own window").count(), 0);

    gui.click("New window");
    gui.click("Back to main window");
    assert!(!gui.app().in_window(id, View::Data));

    gui.click("New window");
    gui.window(&data_title).get_by_label("Close window").simulate_click();
    gui.run();
    assert!(!gui.app().in_window(id, View::Data), "closing the window puts the grid back");
    assert_eq!(gui.app().documents.len(), 1, "nothing closed");
}

/// Side by side, "New window" moves the results; ⌘W in that window puts
/// them back without closing the dataset.
#[test]
fn results_pop_out_and_command_w_brings_them_back() {
    let mut gui = Gui::with_sample("Longley");
    let id = shown(&gui).id;
    assert_eq!(shown(&gui).workspace, Workspace::SideBySide);
    gui.click("New window");
    assert!(gui.app().in_window(id, View::Results));
    let results_title = title(&gui, View::Results);
    assert!(results_title.ends_with(" · Results · AnovaCraft"));
    assert!(gui.shows("The results are in their own window"));
    // The grid stays in the main window.
    assert!(gui.shows("+ Row"));
    gui.shot("windows-results");

    use_window(&mut gui, &results_title);
    assert_eq!(gui.app().last_window, Some((id, View::Results)));
    let n = gui.app().documents.len();
    gui.harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
    gui.run();
    assert!(!gui.app().in_window(id, View::Results), "⌘W closed the window");
    assert_eq!(gui.app().documents.len(), n, "and not the dataset");
    assert!(!gui.shows("The results are in their own window"));
}

/// A single report opens in its own window from its Options menu; the
/// results show a placeholder with Show window and Bring back here.
#[test]
fn report_pops_out_from_its_options_menu() {
    let mut gui = longley_bivariate();
    let id = shown(&gui).id;
    let report = report_id(&gui);
    gui.click("⋯ Options");
    gui.click("Open in new window");
    assert!(gui.app().in_window(id, View::Report(report)));
    let report_title = title(&gui, View::Report(report));
    let window = gui.window(&report_title);
    assert!(window.query_by_label("Horizontal axis ruler").is_some(), "the plot is drawn in the window");
    assert!(gui.shows("This report is in its own window."));
    gui.shot("windows-report");

    // In its own window the menu item takes it back.
    gui.window(&report_title).get_by_label("⋯ Options").simulate_click();
    gui.run();
    let back =
        gui.harness.get_all_by(|n| n.role() == Role::Button && n.label().as_deref() == Some("Back to main window"));
    // The menu's item is the last one drawn (the window's own button comes first).
    back.last().expect("menu item").simulate_click();
    gui.run();
    assert!(!gui.app().in_window(id, View::Report(report)));
    assert!(!gui.shows("This report is in its own window."));

    gui.click("⋯ Options");
    gui.click("Open in new window");
    gui.press("Bring back here");
    assert!(!gui.app().in_window(id, View::Report(report)));
    // A removed report's window goes with it.
    gui.click("⋯ Options");
    gui.click("Open in new window");
    gui.app_mut().documents[0].reports[0].remove_requested = true;
    gui.run();
    assert!(gui.app().windows.is_empty());
}

/// Closing a dataset closes its windows.
#[test]
fn closing_a_dataset_closes_its_windows() {
    let mut gui = longley_bivariate();
    let id = shown(&gui).id;
    let report = report_id(&gui);
    gui.perform(Action::PopOut { doc: id, view: View::Data });
    gui.perform(Action::PopOut { doc: id, view: View::Report(report) });
    assert_eq!(gui.app().windows.len(), 2);
    let index = gui.app().shown;
    gui.perform(Action::Close(index));
    if gui.shows("Don't Save") {
        gui.click("Don't Save");
    }
    assert!(gui.app().documents.iter().all(|d| d.id != id));
    assert!(gui.app().windows.is_empty());
}

/// Selecting points in a report's own window selects the rows everywhere:
/// in the grid of the main window and in the status bars.
#[test]
fn brushing_in_a_report_window_selects_rows_in_the_grid() {
    let mut gui = longley_bivariate();
    let id = shown(&gui).id;
    let report = report_id(&gui);
    gui.perform(Action::PopOut { doc: id, view: View::Report(report) });
    let report_title = title(&gui, View::Report(report));
    let (x, y) = {
        let window = gui.window(&report_title);
        (rect(&window.get_by_label("Horizontal axis ruler")), rect(&window.get_by_label("Vertical axis ruler")))
    };
    // A box over the left half of the plot area, from just inside its top-left corner.
    let from = egui::pos2(y.right() + 4.0, y.top() + 4.0);
    let to = egui::pos2(x.center().x, x.top() - 4.0);
    gui.drag(from, to);
    let selected = shown(&gui).rows.counts().selected;
    assert!(selected > 0 && selected < 16, "the box selected some rows");
    gui.shot("windows-brushing");
    assert_eq!(gui.app().active, gui.app().shown);
    let status = format!("{} selected", selected);
    let painted = gui.painted_text();
    assert!(painted.iter().any(|t| t.contains(&status)), "status bar and window footer: {status}");
    // Pop the grid out too: three places show the same selection.
    gui.perform(Action::PopOut { doc: id, view: View::Data });
    assert_eq!(shown(&gui).rows.counts().selected, selected);
}

/// With two datasets, using a window of one makes it the target of the
/// menus and shortcuts without changing what the main window shows; undo
/// and copy there act on its dataset.
#[test]
fn a_window_makes_its_dataset_the_command_target() {
    let mut gui = Gui::with_sample("Longley");
    let a = gui.app().shown;
    gui.perform(Action::OpenSample(1));
    let b = gui.app().shown;
    assert_ne!(a, b);
    let b_id = gui.app().documents[b].id;
    let b_name = gui.app().documents[b].name().to_owned();
    let a_cell = gui.app().documents[a].table.column(0).unwrap().cell_text(0);
    gui.app_mut().documents[b].workspace = Workspace::Data;
    gui.run();
    gui.click("New window");
    assert!(gui.app().in_window(b_id, View::Data));
    // The main window goes back to the first dataset, showing only its results.
    gui.perform(Action::Activate(a));
    gui.app_mut().documents[a].workspace = Workspace::Results;
    gui.run();
    assert_eq!((gui.app().shown, gui.app().active), (a, a));

    let data_title = format!("{b_name} · Data · AnovaCraft");
    use_window(&mut gui, &data_title);
    assert_eq!(gui.app().shown, a, "the main window still shows the first dataset");
    assert_eq!(gui.app().active, b, "commands act on the window's dataset");
    assert!(gui.shows("From the window you used last; menus act on this dataset"));
    gui.shot("windows-command-target");

    // An edit, then ⌘Z in the window, undoes it in that dataset.
    let before = gui.app().documents[b].table.column(0).unwrap().cell_text(0);
    gui.grid(vec![GridRequest::SetCell { row: 0, column: 0, text: "12345".into() }]);
    assert_eq!(gui.app().documents[b].table.column(0).unwrap().cell_text(0), "12345");
    use_window(&mut gui, &data_title);
    gui.harness.press_key_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    gui.run();
    assert_eq!(gui.app().documents[b].table.column(0).unwrap().cell_text(0), before);
    assert_eq!(gui.app().documents[a].table.column(0).unwrap().cell_text(0), a_cell, "the other dataset is untouched");

    // Copy in the window copies from its grid.
    let column = gui.app().documents[b].table.column(1).unwrap().name.clone();
    gui.app_mut().documents[b].grid.focus_column(1);
    gui.run();
    gui.harness.input_mut().events.push(egui::Event::Copy);
    gui.harness.step();
    let copied: Vec<String> = gui
        .harness
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert!(copied.iter().any(|t| t.starts_with(&column)), "{copied:?}");
    gui.run();

    // The Edit menu acts on the window's dataset too (the menu bar does not change the target).
    gui.menu(&["Edit", "Copy"]);
    assert_eq!(gui.app().active, b);
    assert!(gui.app().message.as_ref().is_some_and(|(m, error)| !error && m.starts_with("Copied")));

    // A click in the main window's workspace makes its dataset the target again.
    let workspace = gui.app().workspace_rect.unwrap();
    // An empty part of the view tabs strip (the data window covers the middle).
    press_at(&mut gui, egui::pos2(workspace.right() - 40.0, workspace.top() + 12.0));
    assert_eq!(gui.app().active, a);
    assert_eq!(gui.app().shown, a);
    // Choosing the dataset already shown keeps everything where it is.
    gui.perform(Action::Activate(a));
    assert!(gui.app().in_window(b_id, View::Data));
}

/// A graph canvas in its own window lists the columns to drag onto its
/// shelves (drag and drop does not cross windows).
#[test]
fn graph_canvas_window_has_its_own_column_list() {
    let mut gui = Gui::with_sample("Longley");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Graph(Box::default()));
    gui.run();
    let id = shown(&gui).id;
    let report = report_id(&gui);
    gui.perform(Action::PopOut { doc: id, view: View::Report(report) });
    let graph_title = title(&gui, View::Report(report));
    let (column, shelf) = {
        let window = gui.window(&graph_title);
        assert!(window.query_by_label("Drag onto a shelf").is_some());
        let column = window.get_all_by_label("GNP").next().expect("GNP in the column list");
        let shelf = window.get_by_label("Horizontal axis");
        (rect(&column), rect(&shelf))
    };
    gui.drag(column.center(), shelf.center() + egui::vec2(40.0, 0.0));
    let saved = shown(&gui).reports[0].saved();
    let app_model::SavedReport::Graph { spec, .. } = saved else { panic!("a graph") };
    assert_eq!(spec.encoding.x, vec!["GNP".to_owned()], "the column landed on the shelf");
    gui.shot("windows-graph-canvas");
}

/// The export dialog opens in the window it was asked from: a report's
/// own window (its Export… button), the results window (the shortcut
/// pressed there) or the main window (the File menu).
#[test]
fn export_dialog_opens_in_the_window_it_was_asked_from() {
    let mut gui = longley_bivariate();
    let id = shown(&gui).id;
    let report = report_id(&gui);
    gui.perform(Action::PopOut { doc: id, view: View::Report(report) });
    let report_title = title(&gui, View::Report(report));
    gui.window(&report_title).get_by_label("Export…").simulate_click();
    gui.run();
    assert!(gui.shows("Export reports"));
    assert_eq!(gui.app().export_window, Some((id, View::Report(report))));
    // Bringing the report back moves the dialog to the main window.
    gui.perform(Action::BringBack { doc: id, view: View::Report(report) });
    assert!(gui.shows("Export reports"), "still open");
    assert_eq!(gui.app().export_window, None);
    gui.click("Cancel");

    gui.perform(Action::PopOut { doc: id, view: View::Results });
    let results_title = title(&gui, View::Results);
    use_window(&mut gui, &results_title);
    gui.harness.press_key_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::E);
    gui.run();
    assert!(gui.shows("Export reports"));
    assert_eq!(gui.app().export_window, Some((id, View::Results)));
    gui.click("Cancel");

    gui.menu(&["File", "Export reports…"]);
    assert!(gui.shows("Export reports"));
    assert_eq!(gui.app().export_window, None);
    gui.click("Cancel");
}

/// Side by side with the results in their own window shows the grid across
/// the whole width under a one-line note (it used to keep an empty
/// right-hand pane, drawn as a black strip, with a cut-off placeholder).
#[test]
fn side_by_side_with_results_away_fills_the_width_with_the_grid() {
    let mut gui = Gui::with_sample("Longley");
    let id = shown(&gui).id;
    assert_eq!(shown(&gui).workspace, Workspace::SideBySide);
    gui.click("New window");
    assert!(gui.app().in_window(id, View::Results));
    if gui.shows("Got it") {
        gui.click("Got it");
    }
    let main = |gui: &Gui, label: &str| {
        let node = gui.harness.query_all_by_label(label).next().unwrap_or_else(|| panic!("no {label}"));
        rect(&node)
    };
    let note = main(&gui, "Bring back here");
    let add_row = main(&gui, "+ Row");
    assert!(note.bottom() <= add_row.top(), "the note is a line above the grid, not a panel beside it");
    let screen = gui.harness.ctx.screen_rect();
    let text = main(&gui, "The results are in their own window");
    assert!(text.left() < screen.center().x, "the note starts at the left edge");
    // No results pane: the last data column reaches past the middle.
    let last = shown(&gui).table.columns().last().unwrap().name.clone();
    let heading = gui.harness.query_all_by_label(&last).map(|n| rect(&n).left()).fold(f32::MIN, f32::max);
    assert!(heading > screen.center().x, "the grid runs across the whole width");

    // Drag the results window (drawn inside the main one here) to the
    // right, so the screenshot shows the main window.
    let title_bar = rect(&gui.window(&title(&gui, View::Results))).center_top() + egui::vec2(0.0, 12.0);
    gui.drag(title_bar, title_bar + egui::vec2(700.0, 380.0));
    gui.shot("side-by-side-results-away");

    gui.click("Back to main window");
    assert!(!gui.app().in_window(id, View::Results));
    assert!(!gui.shows("The results are in their own window"));
}
