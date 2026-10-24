//! Design ▸ Factorial analysis… and its report.

use super::*;

const TITLE: &str = "Factorial analysis";

fn tick(gui: &mut Gui, label: &str) {
    let tick = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(tick).next().expect("a check box").simulate_click();
    gui.run();
}

fn says(gui: &Gui, text: &str) -> bool {
    gui.harness.query_all_by_label_contains(text).next().is_some()
}

/// The dialog needs factors and a response, opens the report with the
/// chosen columns, and reopens filled from it ("Edit analysis…", Update).
#[test]
fn factorial_dialog_runs_and_edits() {
    let mut gui = Gui::with_sample("Filtration rate");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Design", "Factorial analysis…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some());
    gui.shot("dialog-factorial-analysis-first-open");
    assert!(gui.shows("⚠ Choose at least one factor"));
    for f in ["Temperature", "Pressure", "Concentration", "Stirring rate"] {
        tick(&mut gui, f);
    }
    assert!(gui.shows("⚠ Choose a response column"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Filtration rate");
    assert!(says(&gui, "Factorial analysis of Filtration rate on 4 factors"), "{:?}", gui.painted_text());
    // Main effects and two-factor interactions only.
    gui.click("Interactions of up to");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::FactorialAnalysis { request } = reports[0].saved() else {
        panic!("not a factorial analysis")
    };
    let t = &gui.app().documents[active].table;
    assert_eq!(request.response, t.find("Filtration rate").unwrap());
    assert_eq!(request.factors.len(), 4);
    assert_eq!(request.terms, app_model::FactorialTerms::Order(2));
    assert_eq!(reports[0].title(), "Factorial Analysis: Filtration rate");
    let id = reports[0].id();

    // Edit analysis… reopens the dialog filled from the report.
    gui.results();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(says(&gui, "Factorial analysis of Filtration rate on 4 factors"), "filled from the report");
    gui.click("Update");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].id(), id, "updated in place");
}

/// The sample's report draws the normal plot of effects with its term
/// labels and the ME and SME marks, and the cube plot's corner means.
#[test]
fn factorial_report_draws_effect_plots() {
    let mut gui = Gui::with_sample("Filtration rate");
    gui.harness.set_size(egui::vec2(1500.0, 6000.0));
    gui.results();
    for title in [
        "Factorial Summary",
        "Effect Estimates",
        "Normal Plot of Effects",
        "Half-Normal Plot of Effects",
        "Main Effects (data means)",
        "Interaction: Temperature × Concentration (data means)",
        "Cube Plot (data means)",
        "Dispersion Effects (unreplicated)",
    ] {
        assert!(gui.shows(title), "{title} missing");
    }
    let text = gui.painted_text();
    for label in ["Temperature × Concentration", "ME", "SME", "Active (beyond SME)", "46.5"] {
        assert!(text.iter().any(|t| t == label), "{label} not drawn: {text:?}");
    }
    gui.shot("factorial-analysis-report");
}
