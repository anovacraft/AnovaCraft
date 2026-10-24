//! Quality ▸ Principal-components control chart and Monitoring alert and
//! action limits: the dialogs say what is missing, run, reopen filled from
//! their report and update it in place.

use super::*;

fn reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

fn clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

fn tick(gui: &mut Gui, label: &str) {
    let tick = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(tick).next().expect("a check box").simulate_click();
    gui.run();
}

/// Whether a label containing `part` is on screen.
fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

/// Run the dialog `title`, reopen its report with "Edit analysis…" (filled:
/// it shows `summary`) and Update it in place.
fn run_edit_update(gui: &mut Gui, title: &str, summary: &str) {
    assert!(shows_part(gui, summary), "{:?}", gui.painted_text());
    let run = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Run");
    gui.harness.get_by(run).simulate_click();
    gui.run();
    assert!(gui.harness.query_by_role_and_label(Role::Window, title).is_none(), "the dialog closes");
    assert_eq!(reports(gui), 1);
    gui.results();
    let id = gui.app().documents[gui.app().active].reports[0].id();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(gui.harness.query_by_role_and_label(Role::Window, title).is_some(), "the dialog reopens");
    assert!(shows_part(gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(reports(gui), 1, "updated in place");
    assert_eq!(gui.app().documents[gui.app().active].reports[0].id(), id);
}

#[test]
fn pca_chart_dialog_runs_and_edits() {
    const TITLE: &str = "Principal-components control chart";
    let mut gui = Gui::with_sample("Reactor sensors");
    clear_reports(&mut gui);
    gui.menu(&["Quality", "Principal-components control chart…"]);
    gui.shot("dialog-pca-chart-first-open");
    assert!(gui.shows("⚠ Choose at least two measurements"));
    assert!(gui.widget("Run").is_disabled());
    tick(&mut gui, "Reactor temperature (°C)");
    assert!(gui.shows("⚠ Choose at least two measurements"));
    for name in ["Jacket temperature (°C)", "Coolant temperature (°C)", "Feed flow (L/min)"] {
        tick(&mut gui, name);
    }
    tick(&mut gui, "Fit the model to the first");
    run_edit_update(&mut gui, TITLE, "T² and SPE charts of 4 measurements, model from the first");
    let doc = &gui.app().documents[gui.app().active];
    let app_model::SavedReport::PcaChart { request } = doc.reports[0].saved() else {
        panic!("not a principal-components chart")
    };
    assert_eq!(request.columns.len(), 4);
    assert!(request.reference.is_some());
    assert!(gui.shows("T² Chart") || shows_part(&gui, "T²"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Components retained"), "{:?}", gui.painted_text());
}

#[test]
fn monitoring_limits_dialog_runs_and_edits() {
    const TITLE: &str = "Monitoring alert and action limits";
    let mut gui = Gui::with_sample("Clean-room monitoring");
    clear_reports(&mut gui);
    gui.menu(&["Quality", "Monitoring alert and action limits…"]);
    gui.shot("dialog-monitoring-limits-first-open");
    assert!(gui.shows("⚠ Choose the reading column"), "{:?}", gui.painted_text());
    gui.choose(TITLE, 0, "Air colonies (CFU)");
    gui.choose(TITLE, 1, "Location");
    gui.run();
    run_edit_update(&mut gui, TITLE, "Monitoring limits of Air colonies (CFU) by Location");
    let doc = &gui.app().documents[gui.app().active];
    let app_model::SavedReport::MonitoringLimits { request } = doc.reports[0].saved() else {
        panic!("not a monitoring limits report")
    };
    assert_eq!(request.kind, stat_quality::ReadingKind::Counts);
    assert!(shows_part(&gui, "Alert and Action Levels"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Excursions and Early Warnings"), "{:?}", gui.painted_text());

    // Positive samples ask for the number taken.
    clear_reports(&mut gui);
    gui.menu(&["Quality", "Monitoring alert and action limits…"]);
    gui.click("Reset");
    gui.click("Positive samples out of samples taken");
    gui.choose(TITLE, 0, "Contact plates positive");
    assert!(gui.shows("⚠ Choose the column of samples taken"), "{:?}", gui.painted_text());
    gui.click("Cancel");
}
