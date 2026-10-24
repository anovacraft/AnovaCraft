//! Adding several rows or columns at once: every add or insert action of the
//! grid's menus has a "How many" field (remembered for the session, and set
//! to the size of the selection it applies to), column inserts take a type
//! and a name, Enter on the last row adds a row, ⌘⇧I / Ctrl+Shift+I inserts
//! beside the selection, and the status bar says what was added.

use super::*;

/// Press the `button` at `at`, then let the UI settle.
fn click_at(gui: &mut Gui, at: egui::Pos2, button: egui::PointerButton, modifiers: egui::Modifiers) {
    let event = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers };
    // Held keys are the frame's modifiers, as well as the click's.
    gui.harness.input_mut().modifiers = modifiers;
    gui.harness.input_mut().events.push(egui::Event::PointerMoved(at));
    gui.harness.step();
    gui.harness.input_mut().events.push(event(true));
    gui.harness.step();
    gui.harness.input_mut().events.push(event(false));
    gui.harness.step();
    gui.harness.input_mut().modifiers = egui::Modifiers::NONE;
    gui.run();
}

fn right_click_at(gui: &mut Gui, at: egui::Pos2) {
    click_at(gui, at, egui::PointerButton::Secondary, egui::Modifiers::NONE);
}

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

/// The centre of node `node`.
fn centre(node: &Node<'_>) -> egui::Pos2 {
    let b = node.raw_bounds().expect("the node has a position");
    egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

/// The leftmost (`left`) or rightmost text labelled `label` in the grid
/// (right of the sidebar, where the grid's Undo button starts): a row number
/// or cell, or a column heading.
fn text_at(gui: &Gui, label: &str, left: bool) -> egui::Pos2 {
    let grid_left = gui.widget("Undo").raw_bounds().expect("a position").x0 as f32 - 4.0;
    let mut points =
        gui.harness.query_all_by_label(label).map(|n| centre(&n)).filter(|p| p.x > grid_left).collect::<Vec<_>>();
    points.sort_by(|a, b| a.x.total_cmp(&b.x));
    *(if left { points.first() } else { points.last() }).unwrap_or_else(|| panic!("no {label}"))
}

/// The active document.
fn doc(gui: &Gui) -> &Document {
    &gui.app().documents[gui.app().active]
}

/// Type `n` into the open menu's "How many" field.
fn how_many(gui: &mut Gui, n: f64) {
    let field = gui.widget("How many").id();
    gui.set_number(field, n);
}

/// The open menu's "How many" value.
fn shown_count(gui: &Gui) -> f64 {
    gui.widget("How many").numeric_value().expect("a number field")
}

/// Type `text` into the open menu's "Name" field.
fn type_name(gui: &mut Gui, text: &str) {
    gui.widget("Name").focus();
    gui.run();
    gui.harness.input_mut().events.push(egui::Event::Text(text.to_owned()));
    gui.run();
}

fn status(gui: &Gui) -> String {
    gui.app().message.as_ref().map(|m| m.0.clone()).unwrap_or_default()
}

fn names(gui: &Gui) -> Vec<String> {
    doc(gui).table.columns().iter().map(|c| c.name.clone()).collect()
}

fn data_view(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = Workspace::Data;
    gui.run();
    gui
}

#[test]
fn column_heading_menu_inserts_typed_named_columns() {
    let mut gui = data_view("Longley");
    let cols0 = doc(&gui).table.n_cols();
    let first = doc(&gui).table.column(0).unwrap().name.clone();
    let at = text_at(&gui, &first, false);
    right_click_at(&mut gui, at);
    assert_eq!(shown_count(&gui), 1.0, "one column unless asked for more");
    assert!(gui.shows("Insert column to the right"));
    how_many(&mut gui, 3.0);
    gui.click("Text");
    type_name(&mut gui, "Rep");
    gui.shot("insert-several-columns");
    gui.click("Insert 3 columns to the right");
    let table = &doc(&gui).table;
    assert_eq!(table.n_cols(), cols0 + 3);
    assert_eq!(names(&gui)[..4], [first.clone(), "Rep 1".into(), "Rep 2".into(), "Rep 3".into()]);
    assert!(table.columns()[1..4].iter().all(|c| c.kind() == stat_data::ColumnKind::Text));
    assert_eq!(status(&gui), "Added 3 columns (Rep 1 to Rep 3). Undo removes them.");
    assert_eq!(doc(&gui).history.next_undo().unwrap().0, "add 3 columns");
    gui.grid(vec![GridRequest::Undo]);
    assert_eq!(doc(&gui).table.n_cols(), cols0, "one undo removes all three");

    // The name was used up; the number and type are kept.
    let at = text_at(&gui, &first, false);
    right_click_at(&mut gui, at);
    assert_eq!(shown_count(&gui), 3.0, "the last number is offered again");
    assert_eq!(gui.widget("Name").value().unwrap_or_default(), "");
    gui.click("Insert 3 columns to the left");
    assert_eq!(names(&gui)[..4], ["Column 1", "Column 2", "Column 3", first.as_str()]);
    assert!(doc(&gui).table.column(0).unwrap().kind() == stat_data::ColumnKind::Text);
}

#[test]
fn toolbar_and_edit_menu_add_several_at_the_end() {
    let mut gui = data_view("Longley");
    let (rows0, cols0) = (doc(&gui).table.n_rows(), doc(&gui).table.n_cols());

    gui.click("+ Column");
    how_many(&mut gui, 2.0);
    type_name(&mut gui, "Weight");
    gui.click("Numbers");
    gui.click("Add 2 columns at the end");
    assert_eq!(names(&gui)[cols0..], ["Weight 1", "Weight 2"]);
    gui.grid(vec![GridRequest::Undo]);

    // + Row is a menu: above or below need a selected row or the cell cursor.
    gui.click("+ Row");
    assert_eq!(shown_count(&gui), 1.0);
    assert!(gui.widget("Insert row above").is_disabled(), "no rows selected and no cell cursor");
    how_many(&mut gui, 5.0);
    gui.click("Add 5 rows at the end");
    assert_eq!(doc(&gui).table.n_rows(), rows0 + 5);
    assert!(doc(&gui).table.column(0).unwrap().as_numeric().unwrap()[rows0].is_nan());
    assert_eq!(status(&gui), "Added 5 rows. Undo removes them.");
    gui.grid(vec![GridRequest::Undo]);

    // Below the selected rows 3 and 4.
    let active = gui.app().active;
    gui.app_mut().documents[active].rows.select([2, 3], stat_data::SelectionMode::Replace);
    gui.run();
    gui.click("+ Row");
    assert_eq!(shown_count(&gui), 2.0, "the selection's size");
    assert!(gui.shows("Above or below the selected rows (3–4)"));
    gui.click("Insert 2 rows below");
    let x = doc(&gui).table.column(0).unwrap().as_numeric().unwrap().to_vec();
    assert!(x[4].is_nan() && x[5].is_nan() && !x[3].is_nan() && !x[6].is_nan());
    gui.grid(vec![GridRequest::Undo]);
    gui.app_mut().documents[active].rows.clear_selection();
    gui.run();

    // Edit ▸ Add rows shares the toolbar's number.
    gui.menu(&["Edit", "Add rows", "Add 2 rows at the end"]);
    assert_eq!(doc(&gui).table.n_rows(), rows0 + 2);
}

/// Opening + Row or + Column ends the cell edit in progress, keeping the value.
#[test]
fn row_and_column_menus_end_a_cell_edit() {
    let mut gui = data_view("Longley");
    let rows0 = doc(&gui).table.n_rows();
    for (button, typed) in [("+ Row", "11"), ("+ Column", "12")] {
        let cell = doc(&gui).table.column(0).unwrap().cell_text(2);
        let at = text_at(&gui, &cell, true);
        click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
        gui.harness.input_mut().events.push(egui::Event::Text(typed.to_owned()));
        gui.run();
        assert!(doc(&gui).grid.is_editing(), "the cell is open for editing");
        gui.click(button);
        assert!(!doc(&gui).grid.is_editing(), "{button} closes the editor");
        assert_eq!(doc(&gui).table.column(0).unwrap().cell_text(2), typed, "and keeps the value");
        // Above the cell cursor's row, which is still that cell.
        if button == "+ Row" {
            assert!(gui.shows("Above or below row 3 (the cell cursor)"));
            gui.click("Insert row above");
            assert_eq!(doc(&gui).table.n_rows(), rows0 + 1);
            assert!(doc(&gui).table.column(0).unwrap().as_numeric().unwrap()[2].is_nan());
            gui.grid(vec![GridRequest::Undo]);
        } else {
            // Right of the cell cursor's column.
            let first = names(&gui)[0].clone();
            assert!(gui.shows(&format!("Left or right of {first} (the cell cursor)")));
            gui.click("Insert column to the right");
            assert_eq!(names(&gui)[..2], [first.as_str(), "Column 1"]);
            gui.grid(vec![GridRequest::Undo]);
        }
    }
}

#[test]
fn row_menu_inserts_as_many_as_are_selected() {
    let mut gui = data_view("Longley");
    let rows0 = doc(&gui).table.n_rows();
    let id = doc(&gui).id;
    // Select rows 3 to 6 (indices 2..=5).
    let active = gui.app().active;
    gui.app_mut().documents[active].rows.select([2, 3, 4, 5], stat_data::SelectionMode::Replace);
    gui.run();
    let at = text_at(&gui, "4", true);
    right_click_at(&mut gui, at);
    assert_eq!(shown_count(&gui), 4.0, "the selection's size");
    gui.click("Insert 4 rows below");
    let x = doc(&gui).table.column(0).unwrap().as_numeric().unwrap().to_vec();
    assert_eq!(x.len(), rows0 + 4);
    assert!(x[6..10].iter().all(|v| v.is_nan()), "below the last selected row");
    assert!(!x[5].is_nan() && !x[10].is_nan());
    assert_eq!(doc(&gui).id, id);
    gui.grid(vec![GridRequest::Undo]);

    // A row outside the selection: back to the remembered number.
    let at = text_at(&gui, "10", true);
    right_click_at(&mut gui, at);
    assert_eq!(shown_count(&gui), 4.0);
    how_many(&mut gui, 2.0);
    gui.click("Insert 2 rows above");
    let x = doc(&gui).table.column(0).unwrap().as_numeric().unwrap().to_vec();
    assert!(x[9].is_nan() && x[10].is_nan() && !x[8].is_nan());
}

#[test]
fn enter_on_the_last_row_and_the_insert_shortcut() {
    let mut gui = data_view("Longley");
    let rows0 = doc(&gui).table.n_rows();
    let last = doc(&gui).table.column(0).unwrap().cell_text(rows0 - 1);
    // Click the last row's first cell, type a value and press Enter.
    let first_cell = text_at(&gui, &last, true);
    click_at(&mut gui, first_cell, egui::PointerButton::Primary, egui::Modifiers::NONE);
    gui.harness.input_mut().events.push(egui::Event::Text("7".to_owned()));
    gui.run();
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    let table = &doc(&gui).table;
    assert_eq!(table.n_rows(), rows0 + 1, "Enter on the last row adds one");
    assert_eq!(table.column(0).unwrap().cell_text(rows0 - 1), "7");
    // The cursor is on the new row: typing goes there.
    gui.harness.input_mut().events.push(egui::Event::Text("3".to_owned()));
    gui.run();
    key(&mut gui, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(doc(&gui).table.column(0).unwrap().cell_text(rows0), "3");
    assert_eq!(doc(&gui).table.n_rows(), rows0 + 2);

    // ⌘⇧I with a block of three rows: three rows above it.
    key(&mut gui, egui::Key::ArrowUp, egui::Modifiers::NONE);
    key(&mut gui, egui::Key::ArrowUp, egui::Modifiers::SHIFT);
    key(&mut gui, egui::Key::ArrowUp, egui::Modifiers::SHIFT);
    let n = doc(&gui).table.n_rows();
    key(&mut gui, egui::Key::I, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
    let x = doc(&gui).table.column(0).unwrap().as_numeric().unwrap().to_vec();
    assert_eq!(x.len(), n + 3);
    assert!(x[rows0 - 2..rows0 + 1].iter().all(|v| v.is_nan()), "above the block");
    assert_eq!(status(&gui), "Added 3 rows. Undo removes them.");
}

/// Clicking a cell or a heading ends a row selection; clicking a cell or a
/// row number ends a column selection.
#[test]
fn row_and_column_selections_end_on_other_clicks() {
    let mut gui = data_view("Longley");
    let active = gui.app().active;
    let select_rows = |gui: &mut Gui| {
        gui.app_mut().documents[active].rows.select([2, 3], stat_data::SelectionMode::Replace);
        gui.run();
    };
    let selected = |gui: &Gui| doc(gui).rows.selected_rows();
    let cell = doc(&gui).table.column(1).unwrap().cell_text(5);

    select_rows(&mut gui);
    let at = text_at(&gui, &cell, true);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert!(selected(&gui).is_empty(), "a cell click deselects the rows");

    select_rows(&mut gui);
    let heading = doc(&gui).table.column(2).unwrap().name.clone();
    let at = text_at(&gui, &heading, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert!(selected(&gui).is_empty(), "a heading click deselects the rows");
    assert_eq!(doc(&gui).grid.selected_columns(), [2]);

    let at = text_at(&gui, "8", true);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert_eq!(selected(&gui), [7]);
    assert!(doc(&gui).grid.selected_columns().is_empty(), "a row-number click deselects the columns");

    let at = text_at(&gui, &heading, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    let at = text_at(&gui, &cell, true);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert!(doc(&gui).grid.selected_columns().is_empty(), "a cell click deselects the columns");
}

/// Delete selected columns: toolbar, Edit menu and heading menu, one undo step.
#[test]
fn selected_columns_are_deleted_together() {
    // A new table: the samples' columns are used by their reports, so cannot be deleted.
    let mut gui = Gui::blank();
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = Workspace::Data;
    let at = doc(&gui).table.n_cols();
    let name = "Col".to_owned();
    gui.grid(vec![GridRequest::InsertColumn { at, kind: stat_data::ColumnKind::Numeric, count: 5, name }]);
    let names0 = names(&gui);
    assert!(gui.widget("Delete selected columns").is_disabled());
    let second = names0[1].clone();
    let fourth = names0[3].clone();
    let at = text_at(&gui, &second, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    let at = text_at(&gui, &fourth, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::COMMAND);
    assert_eq!(doc(&gui).grid.selected_columns(), [1, 3]);
    gui.click("Delete selected columns");
    let expected: Vec<String> = names0.iter().filter(|n| **n != second && **n != fourth).cloned().collect();
    assert_eq!(names(&gui), expected);
    assert!(doc(&gui).grid.selected_columns().is_empty());
    assert_eq!(doc(&gui).history.next_undo().unwrap().0, "delete 2 columns");
    gui.grid(vec![GridRequest::Undo]);
    assert_eq!(names(&gui), names0, "one undo puts both back");

    // The heading menu of a selected heading offers the whole selection.
    let at = text_at(&gui, &second, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    let at = text_at(&gui, &fourth, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::SHIFT);
    right_click_at(&mut gui, at);
    gui.click("Delete 3 selected columns");
    assert_eq!(names(&gui).len(), names0.len() - 3);
    gui.grid(vec![GridRequest::Undo]);

    // Edit ▸ Delete selected columns, with the count.
    let at = text_at(&gui, &second, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    gui.menu(&["Edit", "Delete selected columns (1)"]);
    assert_eq!(names(&gui).len(), names0.len() - 1);
    assert!(!names(&gui).contains(&second));
}

/// The + Column menu inserts beside the selected columns (left of the first,
/// right of the last), as many as are selected.
#[test]
fn column_menu_inserts_beside_the_selected_columns() {
    let mut gui = data_view("Longley");
    let before = names(&gui);
    let (b, c) = (before[1].clone(), before[2].clone());
    let at = text_at(&gui, &b, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::NONE);
    let at = text_at(&gui, &c, false);
    click_at(&mut gui, at, egui::PointerButton::Primary, egui::Modifiers::SHIFT);
    assert_eq!(doc(&gui).grid.selected_columns(), [1, 2]);

    gui.click("+ Column");
    assert!(gui.shows(&format!("Left or right of the selected columns ({b} to {c})")));
    assert_eq!(shown_count(&gui), 2.0, "as many as are selected");
    gui.click("Insert 2 columns to the right");
    assert_eq!(names(&gui)[1..5], [b.as_str(), c.as_str(), "Column 1", "Column 2"]);
    gui.grid(vec![GridRequest::Undo]);

    gui.click("+ Column");
    gui.click("Insert 2 columns to the left");
    assert_eq!(names(&gui)[..4], [before[0].as_str(), "Column 1", "Column 2", b.as_str()]);
    gui.grid(vec![GridRequest::Undo]);

    // A formula column goes beside the selection too.
    gui.click("+ Column");
    gui.click("Insert formula column to the right…");
    gui.harness.input_mut().events.push(egui::Event::Text("1".to_owned()));
    gui.run();
    gui.click("Apply");
    assert_eq!(names(&gui)[1..4], [b.as_str(), c.as_str(), "Formula 1"]);
    assert!(doc(&gui).table.column(3).unwrap().formula.is_some());
}
