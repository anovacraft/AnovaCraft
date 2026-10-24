//! Folders in a project's tree: reports and canvases listed under their
//! table, new folders by menu, items moved in by menu and by dragging,
//! clicking a folder to show what it holds, and folders saved with the
//! project.

use super::*;
use crate::app::folders::{Dragged, FolderOp, TreeItem, placement};

const PLANTS: &str = "Plant growth (simulated)";

fn center(node: &Node<'_>) -> egui::Pos2 {
    let b = node.raw_bounds().expect("the control has a position");
    egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

/// The sidebar row of table `name` (its label carries a marker before the name).
fn table_row<'a>(gui: &'a Gui, name: &'a str) -> Node<'a> {
    gui.harness.get_by(move |n| {
        n.role() != Role::Label
            && in_sidebar(n)
            && n.label().is_some_and(|l| l.ends_with(name) && l.trim_start_matches(['▶', ' ']) == name)
    })
}

/// Right-click at `at`, then let the UI settle.
fn right_click_at(gui: &mut Gui, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(true));
    gui.harness.step();
    gui.harness.input_mut().events.push(button(false));
    gui.run();
}

/// The project heading named `name` in the sidebar (with or without the unsaved mark).
fn heading<'a>(gui: &'a Gui, name: &'a str) -> Node<'a> {
    gui.harness.get_by(move |n| {
        n.role() == Role::Label
            && in_sidebar(n)
            && text(n).is_some_and(|l| l == name || l.strip_prefix(name) == Some("\u{a0}•"))
    })
}

/// A node's text: its label, or for plain text its value.
fn text(n: &Node<'_>) -> Option<String> {
    n.label().or_else(|| n.value())
}

/// Whether `n` is drawn in the sidebar (the left edge of the window).
fn in_sidebar(n: &Node<'_>) -> bool {
    n.raw_bounds().is_some_and(|b| b.x1 < 300.0)
}

/// The folder named `name` in the sidebar.
fn folder<'a>(gui: &'a Gui, name: &'a str) -> Node<'a> {
    gui.harness.get_by(move |n| n.role() == Role::Label && in_sidebar(n) && text(n).as_deref() == Some(name))
}

fn index_of(gui: &Gui, name: &str) -> usize {
    gui.app().documents.iter().position(|d| d.name() == name).unwrap_or_else(|| panic!("no table {name}"))
}

/// Plant growth with a second table (a copy) in the same project, so the
/// project shows as a tree.
fn plants_project() -> (Gui, u64) {
    let mut gui = Gui::with_sample("Plant growth");
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    let index = gui.app().active;
    let project = gui.app().documents[index].project;
    gui.app_mut().project_mut(project).unwrap().name = Some("Trial".to_owned());
    gui.perform(Action::AddToProject(project, AddSource::Blank));
    gui.perform(Action::Activate(index_of(&gui, PLANTS)));
    (gui, project)
}

fn type_name(gui: &mut Gui, name: &str) {
    assert!(gui.shows("New name"), "the new folder's name is a text box");
    let event = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: if key == egui::Key::A { egui::Modifiers::COMMAND } else { egui::Modifiers::NONE },
    };
    gui.harness.input_mut().events.push(event(egui::Key::A));
    gui.run();
    gui.harness.get_by_label("New name").type_text(name);
    gui.run();
    gui.harness.input_mut().events.push(event(egui::Key::Enter));
    gui.run();
}

/// A table's reports are listed under it in the sidebar; clicking one shows
/// it, and the triangle before the table folds them away.
#[test]
fn reports_are_listed_under_their_table() {
    let mut gui = Gui::with_sample("Plant growth");
    let index = gui.app().active;
    let titles: Vec<String> = gui.app().documents[index].reports.iter().map(Report::title).collect();
    assert!(!titles.is_empty(), "the sample opens with reports");
    let first = titles[0].clone();
    let is_row =
        |n: &Node<'_>, part: &str| n.role() == Role::Label && in_sidebar(n) && text(n).as_deref() == Some(part);
    assert!(gui.harness.query_all_by(|n| is_row(n, &first)).next().is_some(), "the report's row: {titles:?}");

    gui.app_mut().documents[index].workspace = Workspace::Data;
    gui.run();
    let row = gui.harness.get_all_by(|n| is_row(n, &first)).next().unwrap();
    row.simulate_click();
    gui.run();
    assert_ne!(gui.app().documents[index].workspace, Workspace::Data, "the report is in view");

    let fold = format!("Reports of {PLANTS}");
    gui.click(&fold);
    assert!(gui.harness.query_all_by(|n| is_row(n, &first)).next().is_none(), "folded away");
    gui.click(&fold);
    assert!(gui.harness.query_all_by(|n| is_row(n, &first)).next().is_some(), "shown again");
}

/// Move to folder ▸ New folder makes a folder holding the table and names
/// it in place; a report dragged onto the folder joins it; clicking the
/// folder shows its table; the folders are saved in the project file and
/// come back when it reopens; deleting a folder keeps what it held.
#[test]
fn folders_take_items_by_menu_and_drag_and_are_saved() {
    let (mut gui, project) = plants_project();
    let source = index_of(&gui, PLANTS);
    let (source_id, report_id) = {
        let doc = &gui.app().documents[source];
        (doc.id, doc.reports[0].id())
    };
    gui.app_mut().mark_project_saved(project);

    let row = center(&table_row(&gui, "Untitled"));
    right_click_at(&mut gui, row);
    gui.widget("Move to folder").hover();
    gui.run();
    gui.click("New folder");
    type_name(&mut gui, "Week 1");
    let folders = gui.app().folders(project).to_vec();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, "Week 1");
    let blank_id = gui.app().documents[index_of(&gui, "Untitled")].id;
    assert_eq!(folders[0].items, [TreeItem::Table(blank_id)]);
    assert!(gui.app().project_modified(project), "a folder is a change to save");

    // Drag the first report's row onto the folder.
    let title = gui.app().documents[source].reports[0].title();
    let report_row = gui
        .harness
        .get_all_by(|n| n.role() == Role::Label && in_sidebar(n) && text(n).as_deref() == Some(title.as_str()))
        .next()
        .map(|n| center(&n))
        .expect("the report's row");
    let target = center(&folder(&gui, "Week 1"));
    gui.drag(report_row, target);
    let report = TreeItem::Report { doc: source_id, report: report_id };
    let folder_id = gui.app().folders(project)[0].id;
    assert_eq!(placement(gui.app().folders(project)).get(&report), Some(&folder_id), "dropped into the folder");
    gui.shot("project-folders");

    // Clicking the folder shows its table.
    folder(&gui, "Week 1").simulate_click();
    gui.run();
    assert_eq!(gui.app().documents[gui.app().shown].id, blank_id, "its table is shown");

    // Saved and reopened.
    let dir = temp_dir("project-folders");
    let path = dir.join("folders.acraft");
    let file = gui.app().project_file(project, true);
    assert_eq!(file.folders.len(), 1);
    file_io::write_project_file(&file, &path).expect("project written");
    gui.app_mut().close_project(project);
    gui.perform(Action::OpenPaths(vec![path]));
    let reopened = gui.app().documents[gui.app().shown].project;
    assert!(!gui.app().project_modified(reopened), "just opened");
    let folders = gui.app().folders(reopened).to_vec();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, "Week 1");
    assert_eq!(folders[0].items.len(), 2, "{:?}", folders[0].items);
    let source = &gui.app().documents[index_of(&gui, PLANTS)];
    let report = TreeItem::Report { doc: source.id, report: source.reports[0].id() };
    assert!(folders[0].items.contains(&report));
    assert!(folders[0].items.contains(&TreeItem::Table(gui.app().documents[index_of(&gui, "Untitled")].id)));

    // A folder inside it, then deleting the outer one keeps everything.
    gui.perform(Action::Folder(reopened, FolderOp::New { parent: Some(folders[0].id), with: None }));
    type_name(&mut gui, "Plots");
    assert_eq!(gui.app().folders(reopened)[0].folders[0].name, "Plots");
    gui.perform(Action::Folder(
        reopened,
        FolderOp::Move { what: Dragged::Item(report.clone()), to: Some(gui.app().folders(reopened)[0].folders[0].id) },
    ));
    gui.perform(Action::Folder(reopened, FolderOp::Delete(folders[0].id)));
    let left = gui.app().folders(reopened);
    assert_eq!(left.len(), 1, "Plots moved up to the top level");
    assert_eq!(left[0].name, "Plots");
    assert_eq!(left[0].items, [report]);
    let _ = std::fs::remove_dir_all(dir);
}

/// A project's right-click menu offers New folder; dragging an item onto
/// the project's name takes it out of every folder.
#[test]
fn the_project_heading_makes_folders_and_takes_items_back() {
    let (mut gui, project) = plants_project();
    let at = center(&heading(&gui, "Trial"));
    right_click_at(&mut gui, at);
    gui.click("New folder");
    type_name(&mut gui, "Raw data");
    let folder = gui.app().folders(project)[0].id;
    let source_id = gui.app().documents[index_of(&gui, PLANTS)].id;
    let table = TreeItem::Table(source_id);
    gui.perform(Action::Folder(project, FolderOp::Move { what: Dragged::Item(table.clone()), to: Some(folder) }));
    assert_eq!(placement(gui.app().folders(project)).get(&table), Some(&folder));

    gui.perform(Action::Folder(project, FolderOp::OpenEach(folder)));
    assert!(gui.app().in_window(source_id, View::Data), "Open each in a window gives the table a window");
    gui.perform(Action::BringBack { doc: source_id, view: View::Data });

    let from = center(&table_row(&gui, PLANTS));
    let to = center(&heading(&gui, "Trial"));
    gui.drag(from, to);
    assert!(placement(gui.app().folders(project)).is_empty(), "back at the top of the project");
}
