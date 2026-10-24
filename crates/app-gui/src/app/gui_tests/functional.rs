//! Multivariate ▸ Functional data: the dialog says what is missing, runs
//! with factors, reopens filled from its report and updates it in place
//! with registration; the report shows its sections and the Curve
//! Explorer, and offers its score columns.

use super::*;

const TITLE: &str = "Functional data";

fn reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
        || gui.painted_text().iter().any(|t| t.contains(part))
}

#[test]
fn functional_dialog_runs_and_edits() {
    let mut gui = Gui::with_sample("Exotherm profiles");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Multivariate", "Functional data…"]);
    gui.shot("dialog-functional-first-open");
    assert!(gui.shows("⚠ Choose the curve identifier, time and value columns"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Batch");
    gui.choose(TITLE, 1, "Time (min)");
    gui.choose(TITLE, 2, "Time (min)");
    assert!(shows_part(&gui, "must be different"), "{:?}", gui.painted_text());
    gui.choose(TITLE, 2, "Temperature (°C)");
    tick(&mut gui, "Catalyst (%)");
    tick(&mut gui, "Agitator");
    let summary = "Curves of Temperature (°C) by Time (min) for each Batch, scores modelled on 2 factors";
    assert!(shows_part(&gui, summary), "{:?}", gui.painted_text());
    gui.shot("dialog-functional-ready");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    assert_eq!(reports(&gui), 1);
    gui.results();
    assert!(
        shows_part(&gui, "Functional Data: Temperature (°C) by Time (min) (24 curves of Batch)"),
        "{:?}",
        gui.painted_text()
    );
    for part in ["Smoothed Curves", "Eigenfunctions", "Score Model", "Curve Explorer"] {
        assert!(shows_part(&gui, part), "{part}: {:?}", gui.painted_text());
    }
    gui.shot("report-functional");
    // Reopen from the report, add a landmark and continuous registration.
    let id = gui.app().documents[gui.app().active].reports[0].id();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    assert!(gui.shows("Save FPC 1 scores"), "{:?}", gui.painted_text());
    gui.click("Edit analysis…");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog reopens");
    assert!(shows_part(&gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Add landmark");
    tick(&mut gui, "Continuous registration to the mean curve");
    assert!(shows_part(&gui, "registered, scores modelled"), "{:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(reports(&gui), 1, "updated in place");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports[0].id(), id);
    let app_model::SavedReport::FunctionalData { request, .. } = doc.reports[0].saved() else {
        panic!("not a functional data report")
    };
    assert_eq!(request.landmarks.len(), 1);
    assert!(request.landmarks[0].maximum && request.landmarks[0].window.is_none());
    assert!(request.continuous);
    assert_eq!(request.factors.len(), 2);
    let sections = app_model::run_report(&doc.reports[0].saved(), &doc.table, &doc.rows).expect("it runs").sections;
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    for t in ["Landmarks", "Registered Curves", "Warping Functions"] {
        assert!(titles.contains(&t), "{titles:?}");
    }
}
