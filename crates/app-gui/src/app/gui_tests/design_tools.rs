//! Design tools: constraints in optimal designs and augmenting a design.

use super::*;
use stat_data::Column;

const AUGMENT: &str = crate::augment_dialog::TITLE;

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
        || gui.painted_text().iter().any(|t| t.contains(part))
}

/// The D-optimal dialog's Constraints box: the constraints are listed in the
/// preview's notes, every run of the created design satisfies them, and a
/// mistake is reported with its line number.
#[test]
fn optimal_design_with_constraints() {
    let mut gui = Gui::with_sample("Longley");
    let docs = gui.app().documents.len();
    gui.menu(&["Design", "D-optimal (coordinate exchange)…"]);
    gui.widget("Constraints").type_text("A + B <= 0.5\nexclude A < -0.5 and C > 0.5");
    gui.run();
    assert!(
        shows_part(&gui, "Constraints (every run satisfies them): A + B <= 0.5; Exclude A < -0.5 and C > 0.5."),
        "{:?}",
        gui.painted_text()
    );
    assert!(shows_part(&gui, "2 constraints"), "the description counts them");
    gui.shot("design-constraints");
    gui.click("Create");
    assert_eq!(gui.app().documents.len(), docs + 1, "the design opens as a new document");
    let table = &gui.app().documents[gui.app().active].table;
    let col = |name: &str| table.column(table.find(name).expect("factor column")).unwrap().as_numeric().unwrap();
    let (a, b, c) = (col("A"), col("B"), col("C"));
    assert_eq!(a.len(), 12);
    for r in 0..12 {
        assert!(a[r] + b[r] <= 0.5 + 1e-9, "run {r}: A + B = {}", a[r] + b[r]);
        assert!(!(a[r] < -0.5 && c[r] > 0.5), "run {r} is a disallowed combination");
    }
    // Reopened, the constraints are remembered; a bad line is reported.
    gui.menu(&["Design", "D-optimal (coordinate exchange)…"]);
    gui.widget("Constraints").type_text("\nPressure <= 3");
    gui.run();
    assert!(shows_part(&gui, "⚠ Constraint line 3: unknown factor \"Pressure\""), "{:?}", gui.painted_text());
    assert!(gui.widget("Create").is_disabled());
}

/// The half fraction 2^(3−1) (I = ABC) with a text factor and a response.
fn screening_table() -> DataTable {
    DataTable::new(
        "Screening",
        vec![
            Column::numeric("Temp", vec![150.0, 190.0, 150.0, 190.0]),
            Column::numeric("Time", vec![10.0, 10.0, 30.0, 30.0]),
            Column::text("Catalyst", vec!["Y".into(), "X".into(), "X".into(), "Y".into()]),
            Column::numeric("Yield", vec![61.0, 74.0, 66.0, 80.0]),
        ],
    )
    .unwrap()
}

/// Tick the `nth` check box labelled `label` in the augment dialog (the
/// factor list comes before the response list).
/// Tick `label` in the Factors list, the right-hand one (a numeric column
/// is also listed under Responses, on the left, until it is a factor).
fn tick_factor(gui: &mut Gui, label: &str) {
    gui.window(AUGMENT)
        .get_all_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label))
        .last()
        .unwrap_or_else(|| panic!("no check box {label}"))
        .simulate_click();
    gui.run();
}

fn tick(gui: &mut Gui, label: &str, nth: usize) {
    gui.window(AUGMENT)
        .get_all_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label))
        .nth(nth)
        .unwrap_or_else(|| panic!("no check box {label}"))
        .simulate_click();
    gui.run();
}

/// Design ▸ Augment design… folds over a half fraction into a new table with
/// a Block column, refuses center runs with a categorical factor, and adds
/// D-optimal runs that complete the factorial.
#[test]
fn augment_dialog_creates_a_table() {
    let mut gui = Gui::start();
    gui.app_mut().open_table(screening_table(), None);
    gui.run();
    let original = gui.app().active;
    let docs = gui.app().documents.len();
    gui.menu(&["Design", "Augment design…"]);
    assert!(gui.shows("⚠ Choose the factor columns"));
    assert!(gui.widget("Create").is_disabled());
    for factor in ["Temp", "Time", "Catalyst"] {
        tick_factor(&mut gui, factor);
    }
    tick(&mut gui, "Yield", 0);
    assert!(shows_part(&gui, "Adds 4 runs to Screening"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Temp with Time×Catalyst"), "the notes list the aliases removed");
    gui.shot("design-augment");
    gui.click("Create");
    assert_eq!(gui.app().documents.len(), docs + 1);
    let table = &gui.app().documents[gui.app().active].table;
    assert_eq!(table.name(), "Screening (augmented)");
    assert_eq!(table.n_rows(), 8);
    let block = table.column(table.find("Block").expect("block column")).unwrap();
    assert_eq!((block.cell_text(0), block.cell_text(7)), ("1".to_owned(), "2".to_owned()));
    let yield_col = table.column(table.find("Yield").unwrap()).unwrap().as_numeric().unwrap();
    assert!(yield_col[4..].iter().all(|v| v.is_nan()), "responses are left empty");

    // Back on the original table the choices are remembered.
    gui.app_mut().active = original;
    gui.run();
    gui.menu(&["Design", "Augment design…"]);
    gui.click("Center runs");
    assert!(
        shows_part(&gui, "⚠ Center runs need continuous factors (Catalyst is categorical)"),
        "{:?}",
        gui.painted_text()
    );
    assert!(gui.widget("Create").is_disabled());
    gui.click("D-optimal runs");
    assert!(shows_part(&gui, "Adds 4 runs to Screening"));
    gui.click("Create");
    let table = &gui.app().documents[gui.app().active].table;
    let mut combos: Vec<String> =
        (0..8).map(|r| (0..3).map(|c| table.column(c).unwrap().cell_text(r)).collect::<Vec<_>>().join("/")).collect();
    combos.sort();
    combos.dedup();
    assert_eq!(combos.len(), 8, "the added runs complete the 2³ factorial: {combos:?}");
}

/// Switching to another document closes the augment dialog, like every other
/// dialog that reads the active table: its column choices belong to the table
/// it was opened on (it used to stay open and augment the other table's
/// columns at the same positions).
#[test]
fn augment_dialog_closes_when_the_document_changes() {
    let mut gui = Gui::with_sample("Longley");
    gui.app_mut().open_table(screening_table(), None);
    gui.run();
    let screening = gui.app().active;
    gui.menu(&["Design", "Augment design…"]);
    for factor in ["Temp", "Time"] {
        tick_factor(&mut gui, factor);
    }
    assert!(gui.app().augment_dialog.is_some());
    gui.perform(Action::Activate(0));
    assert_ne!(gui.app().active, screening);
    assert!(gui.app().augment_dialog.is_none(), "the dialog closed with its document");
    assert!(gui.harness.query_by_role_and_label(Role::Window, AUGMENT).is_none());
}
