//! Projects that hold several tables: derived tables kept in their source's
//! project, the "Source changed" banner with Update and Undo, saving and
//! reopening a project with its layout, removing a source table, and
//! workbooks opened as one project.

use super::*;
use crate::app::renaming::{self, Place, RenameTarget};

const SUMMARIZE: &str = "Summarize by group";
const PLANTS: &str = "Plant growth (simulated)";
const KEEP: &str = "Keep in this project (Plant growth (simulated))";

/// The Plant growth sample in the light scheme (for the review pictures).
fn plants() -> Gui {
    let gui = Gui::with_sample("Plant growth");
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui
}

/// Tick the `n`th check box labelled `label` (from 0, in drawing order).
fn tick_nth(gui: &mut Gui, label: &str, n: usize) {
    gui.harness
        .get_all_by(|node| node.role() == Role::CheckBox && node.label().as_deref() == Some(label))
        .nth(n)
        .unwrap_or_else(|| panic!("no check box {label} number {n}"))
        .simulate_click();
    gui.run();
}

/// Tick the `n`th check box labelled `label` unless it is ticked already
/// (dialogs remember their settings from the last run).
fn ensure_ticked(gui: &mut Gui, label: &str, n: usize) {
    let on = gui
        .harness
        .get_all_by(|node| node.role() == Role::CheckBox && node.label().as_deref() == Some(label))
        .nth(n)
        .unwrap_or_else(|| panic!("no check box {label} number {n}"))
        .toggled()
        == Some(egui::accesskit::Toggled::True);
    if !on {
        tick_nth(gui, label, n);
    }
}

/// Whether the check box labelled `label` is ticked.
fn ticked(gui: &Gui, label: &str) -> bool {
    let node = gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label));
    node.toggled() == Some(egui::accesskit::Toggled::True)
}

/// Whether some label on screen contains `part`.
fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

/// The index of the open table named `name`.
fn index_of(gui: &Gui, name: &str) -> usize {
    gui.app().documents.iter().position(|d| d.name() == name).unwrap_or_else(|| panic!("no table {name}"))
}

/// Plant growth with Data ▸ Summarize by group of Height by Fertilizer,
/// created with the keep-in-project box as `keep` leaves it.
fn summarize_plants(gui: &mut Gui, keep: bool) {
    let source = index_of(gui, PLANTS);
    gui.perform(Action::Activate(source));
    gui.menu(&["Data", "Summarize by group…"]);
    // Summarize (the first list) Height; group by (the second) Fertilizer.
    ensure_ticked(gui, "Height", 0);
    ensure_ticked(gui, "Fertilizer", 1);
    if keep != ticked(gui, KEEP) {
        gui.click(KEEP);
    }
    assert_eq!(ticked(gui, KEEP), keep);
    gui.click("Create");
    assert!(!gui.harness.query_by_role_and_label(Role::Window, SUMMARIZE).is_some(), "the dialog closes");
}

/// The mean height of fertilizer `level` in the summary table at `index`.
fn mean_of(gui: &Gui, index: usize, level: &str) -> f64 {
    let t = &gui.app().documents[index].table;
    let row = (0..t.n_rows()).position(|r| t.column(0).unwrap().cell_text(r) == level).unwrap();
    let mean = t.find("Height mean").expect("a mean column");
    t.column(mean).unwrap().as_numeric().unwrap()[row]
}

/// A summary made with the box ticked joins Plant growth's project, under a
/// short name, with a link to its source; with the box cleared it opens as a
/// separate project, as before.
#[test]
fn keep_in_project_box_on_and_off() {
    let mut gui = plants();
    let docs = gui.app().documents.len();
    let project = gui.app().documents[index_of(&gui, PLANTS)].project;
    gui.menu(&["Data", "Summarize by group…"]);
    tick_nth(&mut gui, "Height", 0);
    tick_nth(&mut gui, "Fertilizer", 1);
    assert!(shows_part(&gui, "New table “Summary by Fertilizer” in Plant growth (simulated): 3 rows"));
    // What is summarized on the left, the grouping columns on the right.
    let at = |label: &str| gui.harness.get_by_label(label).raw_bounds().expect("placed").x0;
    assert!(at("Summarize:") < at("Group by:"));
    gui.shot("projects-keep-box");
    gui.click("Create");

    assert_eq!(gui.app().documents.len(), docs + 1);
    let summary = index_of(&gui, "Summary by Fertilizer");
    let doc = &gui.app().documents[summary];
    assert_eq!(doc.project, project, "kept in the source's project");
    assert_eq!(gui.app().shown, summary, "the new table is shown");
    let lineage = doc.lineage.as_ref().expect("made by an operation");
    let source = lineage.sources[0].as_ref().expect("linked to its source");
    assert_eq!(source.table, gui.app().documents[index_of(&gui, PLANTS)].table_id);
    assert_eq!(gui.app().members(project).len(), 2);
    // The sidebar shows the project over its tables; the tabs name the project.
    assert!(gui.shows("Made from Plant growth (simulated)"));
    assert!(gui.app().project_modified(project), "the project has a new table to save");
    gui.shot("projects-derived-kept");

    // Cleared: a separate project, with no link to follow.
    summarize_plants(&mut gui, false);
    let separate = gui.app().documents.iter().position(|d| d.name() == "Summary by Fertilizer 2").expect("unique name");
    let doc = &gui.app().documents[separate];
    assert_ne!(doc.project, project, "a separate document");
    assert!(doc.lineage.as_ref().unwrap().sources.iter().all(Option::is_none));
    assert_eq!(gui.app().members(project).len(), 2, "the project did not grow");
}

/// Editing a source marks the table made from it; Update makes it again as
/// one undoable edit; Undo puts the old values (and the mark) back, Redo the
/// new ones. Nothing updates by itself.
#[test]
fn source_changed_banner_update_and_undo() {
    let mut gui = plants();
    summarize_plants(&mut gui, true);
    let summary = index_of(&gui, "Summary by Fertilizer");
    let summary_id = gui.app().documents[summary].id;
    let before = mean_of(&gui, summary, "A");
    assert!(!gui.shows("Source changed"), "up to date");

    // Edit the source: the first plant with fertilizer A grows to 1000.
    let source = index_of(&gui, PLANTS);
    gui.perform(Action::Activate(source));
    let t = &gui.app().documents[source].table;
    let row = (0..t.n_rows()).position(|r| t.column(0).unwrap().cell_text(r) == "A").unwrap();
    gui.grid(vec![GridRequest::SetCell { row, column: 3, text: "1000".into() }]);
    assert!(gui.shows("Source changed"), "a ⚠ by the summary's name");
    assert!(gui.shows("⚠ 1 table can be updated"), "the status bar counts it");
    let summary = index_of(&gui, "Summary by Fertilizer");
    assert_eq!(mean_of(&gui, summary, "A"), before, "never updated automatically");

    gui.perform(Action::Activate(summary));
    assert!(gui.shows("⚠ Source changed: “Plant growth (simulated)” was edited after this table was made."));
    gui.shot("projects-source-changed");
    gui.click("Update");
    let after = mean_of(&gui, summary, "A");
    assert!(after > before + 10.0, "the mean follows the edit: {before} → {after}");
    assert!(!gui.shows("Source changed"), "up to date again");
    assert_eq!(gui.app().documents[summary].id, summary_id, "the same table, updated in place");

    gui.grid(vec![GridRequest::Undo]);
    assert_eq!(mean_of(&gui, summary, "A"), before, "Undo puts the old values back");
    assert!(gui.shows("Source changed"), "and with them the mark");
    gui.grid(vec![GridRequest::Redo]);
    assert_eq!(mean_of(&gui, summary, "A"), after);
    assert!(!gui.shows("Source changed"));
}

/// A project of two tables, one view changed and one table's data grid in
/// a window of its own, saves to one file and reopens the same: tables,
/// link, the table shown, the view and the window (put back on screen when
/// it was saved off it).
#[test]
fn multi_table_project_saves_and_reopens_with_layout() {
    let mut gui = plants();
    summarize_plants(&mut gui, true);
    let source = index_of(&gui, PLANTS);
    gui.app_mut().documents[source].workspace = Workspace::Results;
    let summary = index_of(&gui, "Summary by Fertilizer");
    let summary_id = gui.app().documents[summary].id;
    gui.perform(Action::PopOut { doc: summary_id, view: View::Data });
    gui.perform(Action::Activate(summary));
    assert!(gui.app().windows.iter().any(|w| w.document == summary_id && w.rect.is_some()), "its place is known");
    gui.shot("projects-before-save");

    let project = gui.app().documents[summary].project;
    let dir = temp_dir("multi-table-project");
    let path = dir.join("plants.acraft");
    let mut file = gui.app().project_file(project, true);
    assert_eq!(file.tables.len(), 2);
    assert_eq!(file.layout.windows.len(), 1);
    // As if saved on a wider desk: the window sat on a second monitor.
    file.layout.windows[0].position = Some([4200.0, -600.0]);
    file_io::write_project_file(&file, &path).expect("project written");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"tables\"") && text.contains("\"source_tables\"") && text.contains("\"layout\""));

    gui.app_mut().close_project(project);
    assert!(gui.app().documents.iter().all(|d| d.project != project));
    gui.perform(Action::OpenPaths(vec![path.clone()]));
    let reopened = gui.app().documents[gui.app().shown].project;
    assert_eq!(gui.app().members(reopened).len(), 2, "one project, two tables");
    assert!(!gui.app().project_modified(reopened), "just opened");
    let shown = &gui.app().documents[gui.app().shown];
    assert_eq!(shown.name(), "Summary by Fertilizer", "the table shown when saved");
    assert!(shown.lineage.as_ref().unwrap().sources[0].is_some(), "still linked to its source");
    let source = index_of(&gui, PLANTS);
    assert_eq!(gui.app().documents[source].workspace, Workspace::Results, "its view");
    let window = gui.app().windows.iter().find(|w| w.document == shown.id).expect("the window is back");
    assert_eq!(window.view, View::Data);
    let screen = gui.harness.ctx.screen_rect();
    let rect = window.rect.expect("drawn");
    assert!(screen.contains_rect(rect.shrink(1.0)), "on screen: {rect:?} in {screen:?}");
    assert!(!gui.shows("Source changed"));
    gui.shot("projects-reopened");
    let _ = std::fs::remove_dir_all(dir);
}

/// Removing a table that others were made from asks first; the made tables
/// stay, unlinked. A table nothing was made from is removed at once.
#[test]
fn closing_a_source_table_asks_first() {
    let mut gui = plants();
    summarize_plants(&mut gui, true);
    let project = gui.app().documents[index_of(&gui, PLANTS)].project;
    gui.app_mut().mark_project_saved(project);

    gui.perform(Action::Close(index_of(&gui, PLANTS)));
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Remove table from project?").is_some());
    assert!(shows_part(&gui, "“Summary by Fertilizer” keeps its data"));
    gui.shot("projects-remove-source");
    gui.click("Cancel");
    assert_eq!(gui.app().members(project).len(), 2, "nothing removed");

    gui.perform(Action::Close(index_of(&gui, PLANTS)));
    gui.click("Remove table");
    assert_eq!(gui.app().members(project).len(), 1);
    let summary = &gui.app().documents[index_of(&gui, "Summary by Fertilizer")];
    assert_eq!(summary.project, project, "the project stays open");
    assert!(summary.lineage.as_ref().unwrap().sources[0].is_none(), "no longer linked");
    assert!(gui.app().project_modified(project), "the removal is an unsaved change");

    // A table nothing was made from goes without a question.
    let mut gui = plants();
    summarize_plants(&mut gui, true);
    let project = gui.app().documents[index_of(&gui, PLANTS)].project;
    gui.perform(Action::Close(index_of(&gui, "Summary by Fertilizer")));
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Remove table from project?").is_none());
    assert_eq!(gui.app().members(project).len(), 1);
}

/// A workbook with two sheets opens as one project named after the file,
/// with one table per sheet.
#[test]
fn workbook_opens_as_one_project() {
    let mut gui = Gui::start();
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data/two_sheet_workbook.xlsx");
    gui.perform(Action::OpenPaths(vec![path]));
    // The Open Data dialog offers both sheets with data (the empty one cannot be chosen).
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Open Data: two_sheet_workbook.xlsx").is_some());
    gui.click("Open");
    assert_eq!(gui.app().documents.len(), 2, "one table per sheet");
    let project = gui.app().documents[0].project;
    assert!(gui.app().documents.iter().all(|d| d.project == project), "one project");
    assert_eq!(gui.app().project_name(project), "two_sheet_workbook");
    assert!(!gui.app().project_modified(project));
    assert!(gui.shows("▾ two_sheet_workbook"), "the tabs are grouped under the project's name");
    gui.shot("projects-workbook");
}

/// A project whose file names a column the table does not have (edited by
/// hand) still opens: the unreadable report is left out, the others open,
/// and a note says which report was dropped.
#[test]
fn unreadable_report_is_left_out_with_a_note() {
    let table = DataTable::new(
        "Weights",
        vec![
            stat_data::Column::numeric("x", vec![1.0, 2.0, 3.0, 4.0]),
            stat_data::Column::numeric("y", vec![2.0, 4.5, 5.5, 8.0]),
        ],
    )
    .unwrap();
    let rows = stat_data::RowStateManager::new(table.n_rows());
    let project = app_model::project::Project {
        external: None,
        origin: None,
        scripts: Vec::new(),
        table,
        rows,
        reports: Vec::new(),
    };
    let text = app_model::project::save_project(&project).replace(
        "\"reports\": []",
        r#""reports": [{"analysis": "distribution", "column": "gone"}, {"analysis": "distribution", "column": "y"}]"#,
    );
    let dir = temp_dir("unreadable-report");
    let path = dir.join("weights.acraft");
    std::fs::write(&path, text).unwrap();
    let mut gui = Gui::start();
    gui.perform(Action::OpenPaths(vec![path]));
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.table.n_rows(), 4);
    assert_eq!(doc.reports.len(), 1, "the readable report opens");
    let (message, error) = gui.app().message.clone().expect("a note");
    assert!(!error && message.contains("report 1") && message.contains("gone"), "{message}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Double-click at `at` with the primary button, then let the UI settle.
fn double_click_at(gui: &mut Gui, at: egui::Pos2) {
    let event = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    // Each step advances the clock a quarter second, so a click per step
    // keeps the two within the double-click delay.
    for _ in 0..2 {
        gui.harness.input_mut().events.extend([event(true), event(false)]);
        gui.harness.step();
    }
    gui.run();
}

/// Press `key` (with `modifiers`), then let the UI settle.
fn press_key(gui: &mut Gui, key: egui::Key, modifiers: egui::Modifiers) {
    let event = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    gui.harness.input_mut().events.push(event);
    gui.run();
}

/// Replace the text in the rename box with `name` and press Enter, or
/// Escape when `keep` is false.
fn type_new_name(gui: &mut Gui, name: &str, keep: bool) {
    assert!(gui.shows("New name"), "the name became a text box");
    press_key(gui, egui::Key::A, egui::Modifiers::COMMAND);
    gui.harness.get_by_label("New name").type_text(name);
    gui.run();
    press_key(gui, if keep { egui::Key::Enter } else { egui::Key::Escape }, egui::Modifiers::NONE);
    assert!(!gui.shows("New name"), "the text box closed");
}

/// A table is renamed in place by double-clicking its tab (undoable like any
/// edit; Escape keeps the old name), and a project by its heading's
/// Rename: the new name shows in the sidebar, counts as an unsaved change and
/// is saved in the project file.
#[test]
fn tables_and_projects_rename_in_place() {
    let mut gui = plants();
    summarize_plants(&mut gui, true);
    let summary = index_of(&gui, "Summary by Fertilizer");
    gui.perform(Action::Activate(summary));
    let project = gui.app().documents[summary].project;
    gui.app_mut().mark_project_saved(project);

    // The table's tab along the bottom edge.
    let tab = gui
        .harness
        .get_all_by_label("Summary by Fertilizer")
        .filter_map(|n| n.raw_bounds())
        .max_by(|a, b| a.y0.total_cmp(&b.y0))
        .expect("the tab");
    let at = egui::pos2(((tab.x0 + tab.x1) / 2.0) as f32, ((tab.y0 + tab.y1) / 2.0) as f32);
    double_click_at(&mut gui, at);
    gui.shot("rename-table-in-place");
    type_new_name(&mut gui, "Means by fertilizer", true);
    assert_eq!(gui.app().documents[summary].name(), "Means by fertilizer");
    assert!(shows_part(&gui, "Means by fertilizer"));
    gui.perform(Action::Grid(vec![GridRequest::Undo]));
    assert_eq!(gui.app().documents[summary].name(), "Summary by Fertilizer", "Undo puts the old name back");
    gui.perform(Action::Grid(vec![GridRequest::Redo]));
    assert_eq!(gui.app().documents[summary].name(), "Means by fertilizer");

    // Escape keeps the name it had.
    let table = RenameTarget::Table(gui.app().documents[summary].id);
    renaming::start(&gui.harness.ctx.clone(), table, Place::Tabs, "Means by fertilizer");
    gui.run();
    type_new_name(&mut gui, "Dropped", false);
    assert_eq!(gui.app().documents[summary].name(), "Means by fertilizer");

    // The project, from its heading's right-click Rename.
    let name = gui.app().project_name(project);
    renaming::start(&gui.harness.ctx.clone(), RenameTarget::Project(project), Place::Sidebar, &name);
    gui.run();
    type_new_name(&mut gui, "Greenhouse trial", true);
    assert_eq!(gui.app().project_name(project), "Greenhouse trial");
    assert!(shows_part(&gui, "Greenhouse trial"), "the sidebar and tabs follow");
    assert!(gui.app().project_modified(project), "a new name is a change to save");
    assert_eq!(gui.app().project_file(project, false).name.as_deref(), Some("Greenhouse trial"));
    gui.shot("rename-project-in-place");
}

/// Deleting a column that reports use is refused with a box that says why
/// and names the reports (it used to be only a line in the status bar, easy
/// to miss); Show the reports takes the user to them, OK closes the box.
#[test]
fn deleting_a_column_in_use_says_why() {
    let mut gui = plants();
    let index = gui.app().active;
    let j = gui.app().documents[index].table.find("Height").unwrap();
    gui.perform(Action::Grid(vec![GridRequest::RemoveColumn(j)]));
    assert!(gui.app().documents[index].table.find("Height").is_some(), "the column stays");
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Column not deleted").is_some(), "a box says why");
    assert!(shows_part(&gui, "Column “Height” is used by"), "{:?}", gui.painted_text());
    let title = gui.app().documents[index].reports[0].title();
    assert!(shows_part(&gui, &format!("“{title}”")), "the reports are named");
    gui.shot("column-not-deleted");
    gui.click("OK");
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Column not deleted").is_none());

    // Deleting selected columns says the same; Show the reports goes to them.
    gui.perform(Action::Grid(vec![GridRequest::RemoveColumns(vec![j])]));
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Column not deleted").is_some());
    gui.click("Show the reports");
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Column not deleted").is_none());
    assert_ne!(gui.app().documents[index].workspace, Workspace::Data, "the reports are in view");
}

/// The unsaved-changes mark follows what the file keeps (cell values,
/// exclusions, report options) but not the selection, and saving clears
/// it. The check compares revisions, so it follows each change at once.
#[test]
fn modified_mark_follows_saved_changes_not_selection() {
    let mut gui = Gui::with_sample("Silicon resistivity");
    let active = gui.app().active;
    let project = gui.app().documents[active].project;
    gui.app_mut().documents[active].reports.clear();
    gui.app_mut().add_report(ReportRequest::Bivariate(app_model::BivariateRequest::new(0, 1)));
    gui.app_mut().mark_project_saved(project);
    gui.results();
    assert!(!gui.app().project_shows_modified(project), "just saved");

    gui.app_mut().documents[active].rows.select([0, 1, 2], stat_data::SelectionMode::Replace);
    gui.run();
    assert!(!gui.app().project_modified(project), "a selection is not a change to save");
    assert!(!gui.app().project_shows_modified(project));

    let before = gui.app().documents[active].reports[0].options_revision();
    gui.click("⋯ Options");
    gui.widget("Pairwise comparisons: Tukey–Kramer").hover();
    gui.run();
    gui.click("Games–Howell");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    assert!(gui.app().documents[active].reports[0].options_revision() > before, "the option is a new revision");
    assert!(gui.app().project_shows_modified(project), "an option change shows at once");
    gui.app_mut().mark_project_saved(project);
    gui.run();
    assert!(!gui.app().project_shows_modified(project), "saved");
    let steady = gui.app().documents[active].reports[0].options_revision();
    gui.run();
    gui.run();
    assert_eq!(gui.app().documents[active].reports[0].options_revision(), steady, "redrawing changes nothing");

    gui.app_mut().documents[active].rows.set_excluded([3], true);
    gui.run();
    assert!(gui.app().project_shows_modified(project), "an exclusion is a change to save");
    gui.app_mut().mark_project_saved(project);

    let doc = &mut gui.app_mut().documents[active];
    let copy = (*doc.table).clone();
    doc.table = std::sync::Arc::new(copy);
    assert!(!gui.app().project_modified(project), "the same data in a new table is no change");
    let table = std::sync::Arc::make_mut(&mut gui.app_mut().documents[active].table);
    table.set_value(0, 1, stat_data::CellValue::Number(1.5)).unwrap();
    gui.run();
    assert!(gui.app().project_shows_modified(project), "a new value is a change to save");
}
