//! The space-filling designs in the Design menu.

use super::*;

const TITLE: &str = "Designed experiment";

/// The `n`-th number field of the design dialog.
fn number(gui: &Gui, n: usize) -> (egui::accesskit::NodeId, Option<f64>) {
    let node = gui.window(TITLE).get_all_by_role(Role::SpinButton).nth(n).expect("a number field");
    (node.id(), node.numeric_value())
}

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

/// Opening a maximin Latin hypercube from the menu, setting its runs and
/// creating it: the preview gives the criteria, there is no center-point
/// field, and the new table has one row per run within the factor ranges.
#[test]
fn space_filling_design_from_the_menu() {
    let mut gui = Gui::with_sample("Longley");
    let docs = gui.app().documents.len();
    gui.menu(&["Design", "Latin hypercube (maximin)…"]);
    gui.shot("dialog-space-filling");
    assert!(!shows_part(&gui, "Center points"), "no center points for a space-filling design");
    // Fields: number of factors, low and high of the three factors, runs, seed.
    let (runs, value) = number(&gui, 7);
    assert_eq!(value, Some(12.0), "default runs");
    gui.set_number(runs, 25.0);
    assert!(shows_part(&gui, "Maximin Latin hypercube (25 runs)"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Minimum distance between runs"), "criteria in the preview");
    assert!(shows_part(&gui, "Centered L2 discrepancy"));
    gui.click("Create");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    assert_eq!(gui.app().documents.len(), docs + 1, "the design opens as a new document");
    let table = &gui.app().documents[gui.app().active].table;
    assert_eq!(table.name(), "Latin hypercube (maximin)");
    assert_eq!(table.n_rows(), 25);
    let a = table.column(table.find("A").expect("factor A")).unwrap().as_numeric().unwrap();
    assert!(a.iter().all(|v| (-1.0..=1.0).contains(v)));
    assert!(table.find("Point type").is_none());
}

/// Every space-filling design is in the menu and builds.
#[test]
fn every_space_filling_design_builds() {
    let mut gui = Gui::with_sample("Longley");
    for (label, description) in [
        ("Maximin distance…", "Maximin distance design (12 runs)"),
        ("Minimax distance…", "Minimax distance design (12 runs)"),
        ("Uniform (centered L2 discrepancy)…", "Uniform design (12 runs)"),
    ] {
        gui.menu(&["Design", label]);
        assert!(shows_part(&gui, description), "{label}: {:?}", gui.painted_text());
        assert!(shows_part(&gui, "Fill distance"));
        gui.click("Cancel");
    }
}
