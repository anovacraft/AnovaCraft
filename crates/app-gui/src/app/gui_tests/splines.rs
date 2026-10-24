//! Model ▸ Adaptive regression splines: the dialog says what is missing,
//! runs with validation roles from a column, reopens filled from its report
//! and updates it in place; the report shows its sections and offers its
//! prediction columns, including the formula.

use super::*;

const TITLE: &str = "Multivariate adaptive regression splines";

fn reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

#[test]
fn mars_dialog_runs_and_edits() {
    let mut gui = Gui::with_sample("Fuel consumption");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Adaptive regression splines…"]);
    gui.shot("dialog-mars-first-open");
    assert!(gui.shows("⚠ Choose a response"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    // A categorical response is refused.
    gui.choose(TITLE, 0, "Fuel");
    tick(&mut gui, "Speed (km/h)");
    assert!(shows_part(&gui, "Adaptive regression splines model a continuous response"), "{:?}", gui.painted_text());
    gui.choose(TITLE, 0, "Consumption (L/100 km)");
    for name in ["Load (%)", "Fuel"] {
        tick(&mut gui, name);
    }
    gui.click("Roles from a column");
    assert!(gui.shows("⚠ Choose the column of validation roles"));
    gui.choose(TITLE, 1, "Validation");
    gui.shot("dialog-mars-ready");
    let summary = "Adaptive splines for Consumption (L/100 km) on 3 predictors, interactions of up to 2 predictors";
    assert!(shows_part(&gui, summary), "{:?}", gui.painted_text());
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    assert_eq!(reports(&gui), 1);
    gui.results();
    assert!(
        shows_part(&gui, "Adaptive splines (degree 2): Consumption (L/100 km) ~ Speed (km/h) + Load (%) + Fuel"),
        "{:?}",
        gui.painted_text()
    );
    assert!(shows_part(&gui, "Basis Functions"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Response Explorer"), "{:?}", gui.painted_text());
    // Reopen the dialog from the report and update in place.
    let id = gui.app().documents[gui.app().active].reports[0].id();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    assert!(gui.shows("Save prediction formula"), "{:?}", gui.painted_text());
    gui.click("Edit analysis…");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog reopens");
    assert!(shows_part(&gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("None (additive)");
    gui.click("Update");
    assert_eq!(reports(&gui), 1, "updated in place");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports[0].id(), id);
    let app_model::SavedReport::Mars { request, .. } = doc.reports[0].saved() else { panic!("not an adaptive splines report") };
    assert_eq!(request.max_degree, 1);
    assert_eq!(request.predictors.len(), 3);
    assert!(matches!(request.validation, app_model::ValidationMethod::Column(_)));
    let sections = app_model::run_report(&doc.reports[0].saved(), &doc.table, &doc.rows).expect("it runs").sections;
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.iter().any(|t| t.starts_with("Model Size")), "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with("Forward Pass")), "{titles:?}");
}
