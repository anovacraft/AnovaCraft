//! The Open Data dialog (text files, workbooks and pasted text) and
//! Data ▸ Rows ▸ Use row as column names.

use super::*;

const SHIFTS: &str = "Open Data: shift_weights_report.xlsx";

fn report_workbook() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data/shift_weights_report.xlsx")
}

fn names(gui: &Gui) -> Vec<String> {
    gui.app().documents[gui.app().active].table.columns().iter().map(|c| c.name.clone()).collect()
}

/// A workbook laid out like a printed report (title, heading line, the
/// table from C4, a footnote) opens through the dialog: the detected block
/// shows in the preview, clicks in the preview move it, and only the chosen
/// sheet opens, with the names from row 4 and the twelve rows of values.
#[test]
fn workbook_table_is_picked_in_the_preview() {
    let mut gui = Gui::start();
    apply_color_scheme(&gui.harness.ctx.clone(), ColorScheme::Light);
    gui.perform(Action::OpenPaths(vec![report_workbook()]));
    assert!(gui.app().documents.is_empty(), "nothing opens before the dialog's Open");
    assert!(gui.harness.query_by_role_and_label(Role::Window, SHIFTS).is_some());
    let block = |gui: &Gui| gui.app().open_data[0].current_block();
    // Detected: names in row 4, values from row 5 to the last value (the footnote left out), from column C.
    let detected = block(&gui);
    assert_eq!((detected.header_row, detected.data_start, detected.data_end), (Some(4), 5, Some(16)));
    assert_eq!(detected.first_column, 3);
    gui.shot("open-data-workbook");

    // Clicks move the names to row 2, then put the table's corner back at C4.
    gui.choose(SHIFTS, 0, "Row of column names");
    gui.click("Row 2, column 1");
    assert_eq!(block(&gui).header_row, Some(2));
    assert_eq!(block(&gui).data_start, 5, "the values still start below");
    gui.choose(SHIFTS, 0, "Top-left cell of the table");
    gui.click("Row 4, column 3");
    let b = block(&gui);
    assert_eq!((b.header_row, b.data_start, b.first_column), (Some(4), 5, 3));

    // Only the first sheet.
    gui.click("Open sheet Line 2");
    gui.click("Open");
    assert!(gui.harness.query_by_role_and_label(Role::Window, SHIFTS).is_none(), "the dialog closes");
    assert_eq!(gui.app().documents.len(), 1);
    assert_eq!(names(&gui), ["Shift", "Operator", "Weight"]);
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.table.n_rows(), 12);
    assert!(doc.table.column(2).unwrap().as_numeric().is_some(), "weights are numbers");
    assert_eq!(doc.table.name(), "shift_weights_report \u{2013} Line 1");
}

/// The two sheets of the report stack into one table with a column of sheet
/// names.
#[test]
fn workbook_sheets_stack_into_one_table() {
    let mut gui = Gui::start();
    gui.perform(Action::OpenPaths(vec![report_workbook()]));
    let stack = "Stack the chosen sheets into one table, with their names in a column called";
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(stack)).simulate_click();
    gui.run();
    gui.click("Open");
    assert_eq!(gui.app().documents.len(), 1);
    assert_eq!(names(&gui), ["Sheet", "Shift", "Operator", "Weight"]);
    assert_eq!(gui.app().documents[gui.app().active].table.n_rows(), 24);
}

/// A semicolon file with decimal commas: both are detected and shown, and
/// the table opens with numbers. With Settings ▸ "Open files directly…" on,
/// a plain file opens without the dialog, but this one still asks.
#[test]
fn text_file_shows_detected_separator_and_decimal_mark() {
    let dir = temp_dir("open-data-text");
    let path = dir.join("weights.csv");
    std::fs::write(&path, "Line;Weight\nA;500,5\nB;499,25\n").unwrap();
    let mut gui = Gui::start();
    gui.perform(Action::OpenPaths(vec![path.clone()]));
    let painted = gui.painted_text();
    assert!(painted.iter().any(|t| t == "Detect (semicolon)"), "{painted:?}");
    assert!(painted.iter().any(|t| t == "Comma 1,5 (detected)"));
    gui.shot("open-data-text");
    gui.press("Open");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.table.column(1).unwrap().as_numeric().unwrap(), &[500.5, 499.25]);

    gui.app_mut().settings.open_plain_directly = true;
    let plain = dir.join("plain.csv");
    std::fs::write(&plain, "x,y\n1,2\n").unwrap();
    gui.perform(Action::OpenPaths(vec![plain]));
    assert!(gui.app().open_data.is_empty(), "opened directly");
    assert_eq!(names(&gui), ["x", "y"]);
    gui.perform(Action::OpenPaths(vec![path]));
    assert_eq!(gui.app().open_data.len(), 1, "a decimal comma still asks");
    gui.click("Cancel");
    assert!(gui.app().open_data.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// Pasted text with a title line: the dialog proposes the table below it.
#[test]
fn pasted_text_goes_through_the_dialog() {
    let mut gui = Gui::start();
    let dialog =
        crate::open_data_dialog::OpenDataDialog::from_clipboard("Run log\n\nRun\tYield\n1\t4.5\n2\t5\n".to_owned())
            .unwrap();
    gui.app_mut().open_data.push_back(dialog);
    gui.run();
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Open Data: pasted text").is_some());
    assert_eq!(gui.app().open_data[0].current_block().header_row, Some(3));
    gui.click("Open");
    assert_eq!(names(&gui), ["Run", "Yield"]);
    assert_eq!(gui.app().documents[gui.app().active].table.n_rows(), 2);
}

/// Data ▸ Rows ▸ Use row as column names takes the cursor's row as the
/// names, makes the columns numeric again, and undoes in one step.
#[test]
fn use_row_as_column_names() {
    let mut gui = Gui::start();
    let table = DataTable::new(
        "Raw",
        vec![
            stat_data::Column::text("Column 1", vec!["Run".into(), "1".into(), "2".into()]),
            stat_data::Column::text("Column 2", vec!["Yield".into(), "4.5".into(), "5".into()]),
        ],
    )
    .unwrap();
    gui.app_mut().open_table(table, None);
    gui.run();
    let active = gui.app().active;
    gui.app_mut().documents[active].rows.select([0], stat_data::SelectionMode::Replace);
    gui.menu(&["Data", "Rows", "Use row as column names"]);
    assert_eq!(names(&gui), ["Run", "Yield"]);
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.table.n_rows(), 2);
    assert_eq!(doc.table.column(1).unwrap().as_numeric().unwrap(), &[4.5, 5.0]);
    gui.perform(Action::Grid(vec![GridRequest::Undo]));
    assert_eq!(names(&gui), ["Column 1", "Column 2"]);
    assert_eq!(gui.app().documents[gui.app().active].table.n_rows(), 3);
}

/// Tables can join the open project: File ▸ Add to this project ▸ Empty
/// table adds one to the active project, and the Open Data dialog offers
/// the active project in a box (cleared by default, so a file still opens
/// as a project of its own unless the box is ticked).
#[test]
fn tables_join_the_open_project() {
    let mut gui = Gui::with_sample("Plant growth");
    let project = gui.app().documents[gui.app().active].project;
    gui.menu(&["File", "Add to this project", "Empty table"]);
    assert_eq!(gui.app().documents.len(), 2);
    assert_eq!(gui.app().members(project).len(), 2, "the empty table joined the project");
    assert_eq!(gui.app().documents[gui.app().shown].table.n_rows(), 0, "and is shown");

    // The dialog's box, ticked: the workbook's sheet joins the project too.
    gui.perform(Action::OpenPaths(vec![report_workbook()]));
    let label = format!("Add to the open project “{}”", gui.app().project_name(project));
    let node = gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label.as_str()));
    assert_eq!(node.toggled(), Some(egui::accesskit::Toggled::False), "cleared unless asked for");
    gui.shot("open-data-add-to-project");
    gui.click(&label);
    gui.click("Open sheet Line 2");
    gui.click("Open");
    assert_eq!(gui.app().members(project).len(), 3, "the sheet joined the project");
    assert_eq!(gui.app().documents[gui.app().shown].project, project);

    // Cleared: a project of its own, as before.
    gui.perform(Action::OpenPaths(vec![report_workbook()]));
    gui.click("Open sheet Line 2");
    gui.click("Open");
    assert_eq!(gui.app().members(project).len(), 3);
    assert_ne!(gui.app().documents[gui.app().shown].project, project);
}

/// A newly opened data file shows only its data: no report runs (even with
/// two numeric columns that a linear model could use) until the user asks
/// for one, while a sample still opens with its ready-made reports.
#[test]
fn new_data_opens_without_a_report() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data");
    let mut gui = Gui::start();
    gui.perform(Action::OpenPaths(vec![examples.join("catalyst_experiment.json")]));
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.table.numeric_columns().len(), 3);
    assert!(doc.reports.is_empty(), "no report runs on opening");
    assert_eq!(doc.workspace, Workspace::Data, "the report panel stays closed");

    gui.app_mut().settings.open_plain_directly = true;
    gui.perform(Action::OpenPaths(vec![examples.join("michelson_speed_of_light.csv")]));
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.table.name(), "michelson_speed_of_light");
    assert!(doc.reports.is_empty());
    assert_eq!(doc.workspace, Workspace::Data);

    let gui = Gui::with_sample("Longley");
    let doc = &gui.app().documents[gui.app().active];
    assert!(!doc.reports.is_empty(), "samples keep their reports");
    assert_eq!(doc.workspace, Workspace::SideBySide);
}
