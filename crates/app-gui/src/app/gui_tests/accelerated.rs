//! Reliability ▸ Life–stress models… and Reliability ▸ Degradation…: the
//! dialogs fill requests, refuse incomplete or clashing choices, reopen
//! through Edit analysis…, and their reports draw their plots.

use super::*;
use app_model::SectionKind;

fn clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

fn is_open(gui: &Gui, title: &str) -> bool {
    gui.harness.query_by_role_and_label(Role::Window, title).is_some()
}

fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

/// The saved request of the active document's first report.
fn first_saved(gui: &Gui) -> app_model::SavedReport {
    let doc = &gui.app().documents[gui.app().active];
    doc.reports[0].saved()
}

/// Plots the active document's first report drew.
fn drawn_plots(gui: &mut Gui) -> Vec<SectionKind> {
    gui.harness.set_size(egui::vec2(1500.0, 4200.0));
    gui.results();
    gui.app().documents[gui.app().active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect()
}

/// Reopen the first report with Edit analysis… and check its summary.
fn edit_first_report(gui: &mut Gui, title: &str, summary: &str) {
    gui.results();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(is_open(gui, title), "Edit analysis… reopens {title}");
    assert!(shows_part(gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    assert_eq!(gui.app().documents[gui.app().active].reports.len(), 1, "Update keeps one report");
}

/// Life–stress: Device-A hours by temperature with survivors and counts;
/// the stress cannot repeat the time column; the report draws the
/// life–stress and probability plots.
#[test]
fn life_stress_dialog_fits_device_a() {
    const TITLE: &str = "Life–stress models";
    let mut gui = Gui::with_sample("Device-A");
    clear_reports(&mut gui);
    gui.menu(&["Reliability", "Life–stress models…"]);
    gui.shot("dialog-life-stress-first-open");
    assert!(gui.shows("⚠ Choose a time column"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Hours");
    gui.choose(TITLE, 1, "Censored");
    gui.choose(TITLE, 2, "Units");
    assert!(gui.shows("⚠ Choose a stress column"));
    gui.choose(TITLE, 3, "Hours");
    assert!(gui.shows("⚠ A column is chosen twice"));
    gui.choose(TITLE, 3, "Temperature (°C)");
    gui.choose(TITLE, 5, "Lognormal");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    assert!(!is_open(&gui, TITLE), "the dialog closes");
    let app_model::SavedReport::LifeStress { request } = first_saved(&gui) else {
        panic!("not a life–stress report")
    };
    assert_eq!(request.family, app_model::RegressionFamily::Lognormal);
    assert_eq!(request.stresses.len(), 1);
    assert_eq!(request.stresses[0].transform, app_model::StressTransform::Arrhenius);
    assert!(request.frequency.is_some());
    let plots = drawn_plots(&mut gui);
    for kind in [SectionKind::LifeStressPlot, SectionKind::LifeStressProbabilityPlot] {
        assert!(plots.contains(&kind), "{kind:?} not drawn: {plots:?}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Activation Energy")), "{text:?}");
    gui.shot("life-stress-report");
    edit_first_report(&mut gui, TITLE, "Lognormal–Arrhenius (temperature) model of Hours by Temperature (°C)");
}

/// A second stress adds its own column, relationship and use condition.
#[test]
fn life_stress_dialog_takes_a_second_stress() {
    const TITLE: &str = "Life–stress models";
    let mut gui = Gui::with_sample("Device-A");
    clear_reports(&mut gui);
    gui.menu(&["Reliability", "Life–stress models…"]);
    gui.choose(TITLE, 0, "Hours");
    gui.choose(TITLE, 1, "Censored");
    gui.choose(TITLE, 3, "Temperature (°C)");
    tick(&mut gui, "Add a second stress");
    assert!(gui.shows("⚠ Choose a stress column"), "the second stress needs a column");
    // Lists after the first stress: its relationship (4), then the second
    // stress's column (5) and relationship (6), then the family (7).
    gui.choose(TITLE, 5, "Units");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    let app_model::SavedReport::LifeStress { request } = first_saved(&gui) else { panic!() };
    assert_eq!(request.stresses.len(), 2);
    assert_eq!(request.stresses[1].transform, app_model::StressTransform::InversePower);
    assert_eq!(request.use_stress.len(), 2);
}

/// Degradation: laser current paths extended to a 10% increase, then the
/// destructive layout for bond strength with an Arrhenius rate.
#[test]
fn degradation_dialog_runs_both_layouts() {
    const TITLE: &str = "Degradation";
    let mut gui = Gui::with_sample("Laser degradation");
    clear_reports(&mut gui);
    gui.menu(&["Reliability", "Degradation…"]);
    gui.shot("dialog-degradation-first-open");
    assert!(gui.shows("⚠ Choose a time column"));
    gui.choose(TITLE, 0, "Laser");
    gui.choose(TITLE, 1, "Hours");
    gui.choose(TITLE, 2, "Hours");
    assert!(gui.shows("⚠ A column is chosen twice"));
    gui.choose(TITLE, 2, "Current increase (%)");
    assert!(!gui.widget("Run").is_disabled());
    gui.click("Run");
    assert!(!is_open(&gui, TITLE), "the dialog closes");
    let app_model::SavedReport::Degradation { request } = first_saved(&gui) else { panic!("not a degradation report") };
    assert_eq!(request.threshold, 10.0);
    assert_eq!(request.method, app_model::DegradationMethod::PseudoFailureTimes);
    let plots = drawn_plots(&mut gui);
    for kind in [SectionKind::DegradationPathPlot, SectionKind::DegradationProbabilityPlot] {
        assert!(plots.contains(&kind), "{kind:?} not drawn: {plots:?}");
    }
    gui.shot("degradation-report");
    edit_first_report(&mut gui, TITLE, "degradation paths and pseudo failure times of Current increase (%)");

    let mut gui = Gui::with_sample("Adhesive bond");
    clear_reports(&mut gui);
    gui.menu(&["Reliability", "Degradation…"]);
    gui.click("Destructive: one measurement per unit");
    gui.choose(TITLE, 0, "Weeks");
    gui.choose(TITLE, 1, "Strength (N)");
    gui.click("Falls to the threshold");
    let threshold = gui.window(TITLE).get_all_by_role(Role::SpinButton).next().expect("threshold field").id();
    gui.set_number(threshold, 50.0);
    gui.choose(TITLE, 2, "Log");
    gui.choose(TITLE, 3, "Square root");
    tick(&mut gui, "Accelerating stress (accelerated degradation)");
    assert!(gui.shows("⚠ Choose a stress column"));
    gui.choose(TITLE, 5, "Temperature (°C)");
    gui.click("Run");
    let app_model::SavedReport::Degradation { request } = first_saved(&gui) else { panic!() };
    assert!(matches!(request.layout, app_model::DegradationLayout::Destructive { .. }));
    assert_eq!(request.threshold, 50.0);
    assert_eq!(request.direction, app_model::FailureDirection::Falling);
    assert!(request.stress.is_some() && request.use_stress.is_some());
    let plots = drawn_plots(&mut gui);
    for kind in [SectionKind::DegradationPathPlot, SectionKind::DegradationLifeCurve] {
        assert!(plots.contains(&kind), "{kind:?} not drawn: {plots:?}");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t.starts_with("Activation energy")), "{text:?}");
}
