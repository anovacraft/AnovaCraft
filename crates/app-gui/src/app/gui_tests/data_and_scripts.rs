//! The script editor's folding, completion and find/replace; script
//! notebooks; saved row subsets; the data grid's text pane; and Describe ▸
//! Multiple response….

use super::*;

/// Let a notebook's cell (run on a thread) finish and its result arrive.
pub(super) fn settle_notebooks(gui: &mut Gui) {
    for _ in 0..500 {
        gui.run();
        if !gui.app().notebook_windows.iter().any(crate::notebook_window::NotebookWindow::is_running) {
            gui.run();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the notebook cell did not finish");
}

/// The Script window (its title names the script).
fn script_window(gui: &Gui) -> Node<'_> {
    gui.harness.get_by(|n| n.role() == Role::Window && n.label().is_some_and(|l| l.starts_with("Script — ")))
}

/// The script editor folds blocks from its Fold menu (the folded text runs
/// unchanged), offers script functions as you type, and finds and replaces
/// across the script.
#[test]
fn script_editor_folds_completes_and_replaces() {
    let mut gui = Gui::with_sample("Norris");
    gui.menu(&["Scripts", "Script window"]);
    let source = "fn twice(x) {\n    x * 2\n}\nlet t = current_table();\nprint(twice(2));\n";
    gui.app_mut().script.open_text(source.to_owned());
    gui.run();
    gui.menu(&["Fold", "Fold all blocks"]);
    let shown = script_window(&gui).get_by_role(Role::MultilineTextInput).value().unwrap_or_default();
    assert!(shown.starts_with("fn twice(x) {⋯}"), "{shown:?}");
    assert_eq!(gui.app().script.current_script().0, source, "folding changes only what is shown");
    gui.menu(&["Fold", "Unfold all"]);
    let shown = script_window(&gui).get_by_role(Role::MultilineTextInput).value().unwrap_or_default();
    assert_eq!(shown, source);

    // Completion: typing a function's start lists it; clicking inserts it.
    gui.app_mut().script.open_text(String::new());
    gui.run();
    let editor = script_window(&gui).get_by_role(Role::MultilineTextInput);
    editor.click();
    gui.run();
    script_window(&gui).get_by_role(Role::MultilineTextInput).type_text("current_ta");
    gui.run();
    assert!(gui.shows("current_table"), "the completion list offers it: {:?}", gui.painted_text());
    gui.shot("script-completion");
    gui.click("current_table");
    assert_eq!(gui.app().script.current_script().0, "current_table(");

    // Find and replace.
    gui.app_mut().script.open_text("let a = 1;\nlet ab = a + a;\n".to_owned());
    gui.run();
    gui.click("Find…");
    let window = script_window(&gui);
    let boxes: Vec<_> = window.get_all_by_role(Role::TextInput).collect();
    boxes[0].type_text("a");
    drop(boxes);
    gui.run();
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some("Whole names")).simulate_click();
    gui.run();
    assert!(gui.shows("3 matches"), "{:?}", gui.painted_text());
    let window = script_window(&gui);
    window.get_all_by_role(Role::TextInput).nth(1).expect("replace box").type_text("x");
    gui.run();
    gui.click("Replace all");
    assert_eq!(gui.app().script.current_script().0, "let x = 1;\nlet ab = x + x;\n");
    assert!(gui.shows("Replaced 3"));
}

/// Scripts ▸ New notebook adds a notebook to the table's project; running
/// its code cell shows the table as output, and the notebook is saved with
/// the project.
#[test]
fn notebooks_run_cells_and_are_saved_with_the_project() {
    let mut gui = Gui::with_sample("Norris");
    gui.menu(&["Scripts", "New notebook"]);
    let project = gui.app().documents[gui.app().active].project;
    assert_eq!(gui.app().project(project).unwrap().notebooks.len(), 1);
    assert!(gui.harness.query_all_by_role(Role::Window).any(|w| w.label().is_some_and(|l| l.starts_with("Notebook"))));
    gui.click("▶ Run");
    settle_notebooks(&mut gui);
    let notebook = &gui.app().project(project).unwrap().notebooks[0];
    let outputs = &notebook.cells[1].outputs;
    assert!(
        outputs.iter().any(|o| matches!(o, app_model::notebook::CellOutput::Table { .. })),
        "the table is shown: {outputs:?}"
    );
    assert_eq!(notebook.cells[1].run, Some(1));
    gui.shot("notebook");
    // A second cell sees the first cell's variable.
    gui.click("+ Code");
    let p = gui.app_mut().project_mut(project).unwrap();
    p.notebooks[0].cells[2].source = "t.rows()".to_owned();
    gui.run();
    gui.click("▶ Run all");
    settle_notebooks(&mut gui);
    settle_notebooks(&mut gui);
    let notebook = &gui.app().project(project).unwrap().notebooks[0];
    assert_eq!(
        notebook.cells[2].outputs,
        vec![app_model::notebook::CellOutput::Value("36".to_owned())],
        "{:?}",
        notebook.cells[2].outputs
    );
    let file = gui.app().project_file(project, false);
    assert_eq!(file.notebooks.len(), 1, "saved with the project");
}

/// Data ▸ Row subsets saves the selected rows under a name; making the
/// subset active leaves the other rows out of every report.
#[test]
fn row_subsets_are_saved_and_reports_follow_the_active_one() {
    let mut gui = Gui::with_sample("Norris");
    let active = gui.app().active;
    let n = gui.app().documents[active].rows.len();
    gui.app_mut().documents[active].rows.select(0..10, stat_data::SelectionMode::Replace);
    gui.run();
    gui.menu(&["Data", "Row subsets", "Save selection as subset…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Save selection as subset").is_some());
    assert_eq!(gui.app().subset_prompt.as_ref().unwrap().name, "Subset", "a name is suggested");
    gui.app_mut().subset_prompt.as_mut().unwrap().name = "Subset low".to_owned();
    gui.run();
    gui.click("Save");
    let rows = &gui.app().documents[active].rows;
    assert_eq!(rows.subset_names(), ["Subset low"]);
    assert_eq!(rows.subset_rows(0), (0..10).collect::<Vec<_>>());
    assert_eq!(rows.active_subset(), None, "saving does not switch to it");
    gui.menu(&["Data", "Row subsets", "Subset low (10 rows)"]);
    let rows = &gui.app().documents[active].rows;
    assert_eq!(rows.active_subset(), Some(0));
    assert_eq!(rows.counts().outside_subset, n - 10);
    assert!((10..n).all(|r| rows.is_excluded(r)) && (0..10).all(|r| !rows.is_excluded(r)));
    assert!(gui.shows("Subset: Subset low"), "the sidebar names it");
    gui.shot("row-subsets");
    gui.menu(&["Data", "Row subsets", "All rows"]);
    assert_eq!(gui.app().documents[active].rows.active_subset(), None);
    assert!((0..n).all(|r| !gui.app().documents[active].rows.is_excluded(r)));
}

/// The grid's text pane shows the cursor cell's whole text and stores an
/// edit as one undoable cell edit.
#[test]
fn text_pane_edits_long_text() {
    let mut gui = Gui::with_sample("Visitor survey");
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = Workspace::Data;
    gui.run();
    let table = &gui.app().documents[active].table;
    let comment = table.find("Comment").unwrap();
    let row = (0..table.n_rows())
        .find(|&r| table.column(comment).unwrap().cell_text(r).starts_with("The café"))
        .expect("a long comment");
    gui.app_mut().documents[active].grid.place_cursor(row, comment);
    gui.run();
    gui.click("Text pane");
    assert!(gui.shows("Revert"), "{:?}", gui.painted_text());
    let is_pane =
        |n: &Node<'_>| n.role() == Role::MultilineTextInput && n.value().is_some_and(|v| v.starts_with("The café"));
    gui.harness.get_by(is_pane).click();
    gui.run();
    gui.harness.get_by(is_pane).type_text(" Long note.");
    gui.run();
    gui.harness.get_by(|n| n.role() == Role::Button && n.label().as_deref() == Some("Save")).simulate_click();
    gui.run();
    let text = gui.app().documents[active].table.column(comment).unwrap().cell_text(row);
    assert!(text.ends_with("Long note."), "{text:?}");
    gui.shot("text-pane");
    gui.grid(vec![GridRequest::Undo]);
    let text = gui.app().documents[active].table.column(comment).unwrap().cell_text(row);
    assert!(!text.ends_with("Long note."), "undone");
}

/// Describe ▸ Multiple response… counts a column of lists and cross-tabs
/// it by a group column.
#[test]
fn multiple_response_dialog_counts_items_by_group() {
    const TITLE: &str = "Multiple response";
    let mut gui = Gui::with_sample("Visitor survey");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Describe", "Multiple response…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog opens");
    assert!(gui.shows("⚠ Choose the answer column"), "{:?}", gui.painted_text());
    gui.choose(TITLE, 0, "Heard about it from");
    gui.choose(TITLE, 1, "Age group");
    gui.shot("multiple-response-dialog");
    gui.click("Run");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::MultipleResponseItems { request } = reports[0].saved() else {
        panic!("not a multiple-response report")
    };
    assert!(request.group.is_some());
    gui.harness.set_size(egui::vec2(1500.0, 2400.0));
    gui.results();
    let painted = gui.painted_text();
    for start in ["Response Frequencies", "Cross-tabulation by Age group", "Tests of Marginal Independence"] {
        assert!(painted.iter().any(|t| t.starts_with(start)), "{start}: {painted:?}");
    }
    gui.shot("multiple-response-report");
}
