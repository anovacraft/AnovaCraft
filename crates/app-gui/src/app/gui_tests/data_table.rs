//! The data table's display features: value labels, colour and marker by
//! column, columns shown on hover, and the small charts under the grid's
//! headings.

use super::*;

/// Press and release the primary button at `at`.
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

fn clinical() -> Gui {
    Gui::with_sample("Clinical visits")
}

fn column(gui: &Gui, name: &str) -> usize {
    gui.app().documents[gui.app().active].table.find(name).expect("sample column")
}

/// The grid shows value labels in place of the stored values (Stored
/// values shows the codes); the Value labels dialog renames a level, the
/// model report follows, and Undo puts the old label back.
#[test]
fn value_labels_show_in_the_grid_and_rename_levels() {
    let mut gui = clinical();
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = Workspace::Data;
    gui.run();
    assert!(gui.shows("Placebo") && gui.shows("Female"), "the grid shows labels");
    gui.shot("value-labels-grid");
    gui.click("Stored values");
    assert!(!gui.painted_text().iter().any(|t| t == "Placebo"), "the codes are shown instead");
    gui.click("Stored values");

    let arm = column(&gui, "Arm");
    gui.app_mut().documents[active].grid.place_cursor(0, arm);
    gui.menu(&["Data", "Value labels…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Value labels").is_some());
    let field = |n: &Node<'_>| n.role() == Role::TextInput && n.label().as_deref() == Some("Label for 1");
    gui.harness.get_by(field).click();
    gui.run();
    gui.harness.get_by(field).key_press(egui_kittest::kittest::Key::End);
    gui.harness.get_by(field).type_text(" drug");
    gui.run();
    gui.shot("dialog-value-labels");
    gui.click("Apply");
    let labels = &gui.app().documents[active].table.columns()[arm].value_labels;
    assert_eq!(labels.get("1").map(String::as_str), Some("Active drug"), "{labels:?}");
    let model = gui.app().documents[active].reports.iter().find(|r| r.title().starts_with("Linear Model")).expect("the model");
    let sections = format!("{:?}", model.for_export().sections);
    assert!(sections.contains("Active drug"), "the model names the new label");
    gui.grid(vec![GridRequest::Undo]);
    let labels = &gui.app().documents[active].table.columns()[arm].value_labels;
    assert_eq!(labels.get("1").map(String::as_str), Some("Active"));
}

/// Typing a label into a labelled column's cell stores the value it names.
#[test]
fn typing_a_label_stores_its_value() {
    let mut gui = clinical();
    let arm = column(&gui, "Arm");
    gui.grid(vec![GridRequest::SetCell { row: 0, column: arm, text: "Active".into() }]);
    let col = &gui.app().documents[gui.app().active].table.columns()[arm];
    assert_eq!(col.cell_text(0), "1");
    assert_eq!(col.level_text(0), "Active");
}

/// Data ▸ Row states ▸ Colour and marker by column… colours every row by a
/// column's level, report plots key the colours by level, and Clear row
/// colours and markers removes them.
#[test]
fn rows_take_colours_and_markers_from_a_column() {
    let mut gui = clinical();
    let active = gui.app().active;
    gui.menu(&["Data", "Row states", "Colour and marker by column…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Colour and marker by column").is_some());
    gui.choose("Colour and marker by column", 0, "Arm");
    assert!(gui.shows("Placebo") && gui.shows("Active"), "the dialog previews the levels");
    gui.shot("dialog-color-by-column");
    gui.click("Apply");
    let rows = &gui.app().documents[active].rows;
    assert_eq!(rows.style_column(), Some("Arm"));
    assert!((0..rows.len()).all(|r| rows.color(r).is_some() && rows.marker(r).is_some()));
    assert_ne!(rows.marker(0), rows.marker(1), "the two arms differ in marker as well as colour");
    gui.results();
    assert!(gui.painted_text().iter().any(|t| t == "Arm = Placebo"), "{:?}", gui.painted_text());
    gui.shot("row-colours-in-plot");
    gui.menu(&["Data", "Row states", "Clear row colours and markers"]);
    let rows = &gui.app().documents[active].rows;
    assert!(!rows.has_row_styles() && rows.style_column().is_none());
}

/// A column's right-click menu sets it to show on hover; point tooltips
/// then list its value.
#[test]
fn columns_shown_on_hover_join_point_tooltips() {
    let mut gui = clinical();
    let active = gui.app().active;
    let sex = column(&gui, "Sex");
    gui.grid(vec![GridRequest::SetHover(sex, true)]);
    let table = &gui.app().documents[active].table;
    assert!(table.columns()[sex].hover);
    let lines = crate::report_view::hover_lines(table, 0);
    assert!(lines.contains("Patient: P001"), "{lines}");
    assert!(lines.contains("Sex: Female") || lines.contains("Sex: Male"), "{lines}");
    gui.grid(vec![GridRequest::Undo]);
    assert!(!gui.app().documents[active].table.columns()[sex].hover);
}

/// Column charts draw a small chart under each heading; clicking a bar
/// selects that level's rows.
#[test]
fn heading_charts_select_the_rows_of_a_bar() {
    let mut gui = clinical();
    let active = gui.app().active;
    gui.app_mut().documents[active].workspace = Workspace::Data;
    gui.run();
    gui.click("Column charts");
    assert!(gui.shows("Chart of Arm") && gui.shows("Chart of Baseline SBP"));
    gui.shot("heading-charts");
    let chart = gui.harness.get_by_label("Chart of Arm");
    let b = chart.raw_bounds().expect("the chart has a position");
    // Two bars (Active, Placebo): the left one is Active.
    let at = egui::pos2((b.x0 + (b.x1 - b.x0) * 0.25) as f32, ((b.y0 + b.y1) / 2.0) as f32);

    click_at(&mut gui, at);
    let doc = &gui.app().documents[active];
    let arm = doc.table.find("Arm").unwrap();
    let selected = doc.rows.selected_rows();
    assert!(!selected.is_empty(), "the bar's rows are selected");
    assert!(selected.iter().all(|&r| doc.table.columns()[arm].level_text(r) == "Active"));
    assert_eq!(
        selected.len(),
        (0..doc.table.n_rows()).filter(|&r| doc.table.columns()[arm].cell_text(r) == "1").count()
    );
    assert!(doc.grid.selected_columns().is_empty(), "the click selects rows, not the column");
}
