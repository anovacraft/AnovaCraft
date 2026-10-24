//! Model ▸ Treatment effects (observational data)…: the dialog's roles,
//! its method settings, and the report's sections and in-place options.

use super::*;

/// Tick the `nth` check box labelled `label` (the covariate and subgroup
/// lists hold the same columns).
fn tick_nth(gui: &mut Gui, label: &str, nth: usize) {
    gui.harness
        .get_all_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label))
        .nth(nth)
        .expect("no such check box")
        .simulate_click();
    gui.run();
}

fn paints_start(gui: &Gui, start: &str) -> bool {
    gui.painted_text().iter().any(|t| t.starts_with(start))
}

/// The dialog asks for a treatment, outcome and covariates, refuses ATE
/// matching without replacement, and opens a doubly robust report with
/// balance, overlap and subgroup sections.
#[test]
fn treatment_dialog_runs_a_doubly_robust_estimate() {
    const TITLE: &str = "Treatment effects";
    let mut gui = Gui::with_sample("Cardiac rehabilitation");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Treatment effects (observational data)…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog opens");
    assert!(gui.shows("⚠ Choose a treatment"), "{:?}", gui.painted_text());
    gui.choose(TITLE, 0, "Programme");
    gui.choose(TITLE, 1, "Enrolled");
    gui.choose(TITLE, 2, "Fitness gain");
    assert!(gui.shows("⚠ Choose at least one covariate"), "{:?}", gui.painted_text());
    for c in ["Age", "Sex", "Severity", "Smoker", "Region"] {
        tick_nth(&mut gui, c, 0);
    }
    tick_nth(&mut gui, "Age group", 1);
    assert!(
        paints_start(&gui, "ATE of Programme on Fitness gain adjusting for 5 covariates, by 1 subgroup column"),
        "{:?}",
        gui.painted_text()
    );
    // Matching for the ATE needs replacement.
    gui.choose(TITLE, 3, "Nearest-neighbour matching");
    assert!(gui.shows("⚠ Matching for the average effect over everyone (ATE) needs matching with replacement"));
    gui.choose(TITLE, 3, "Doubly robust (weighting with outcome models)");
    gui.shot("treatment-effects-dialog");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::TreatmentEffects { request } = reports[0].saved() else {
        panic!("not a treatment report")
    };
    assert_eq!(request.method, app_model::EffectMethod::DoublyRobust);
    assert_eq!(request.treated_level.as_deref(), Some("Enrolled"));
    assert_eq!(request.covariates.len(), 5);
    assert_eq!(request.subgroups.len(), 1);
    gui.harness.set_size(egui::vec2(1500.0, 3200.0));
    gui.results();
    for heading in ["Covariate Balance", "Balance Plot", "Propensity Score Overlap", "Subgroup Forest Plot"] {
        assert!(
            gui.harness.query_all_by(|n| n.label().is_some_and(|l| l.starts_with(heading))).count() > 0,
            "{heading}"
        );
    }
    gui.shot("treatment-effects-report");
    // The options switch the effect to the ATT in place.
    gui.click("⋯ Options");
    gui.click("ATT");
    gui.harness.press_key(egui::Key::Escape);
    gui.run();
    let app_model::SavedReport::TreatmentEffects { request } = gui.app().documents[active].reports[0].saved() else {
        panic!()
    };
    assert_eq!(request.estimand, app_model::Estimand::Att, "kept with the report");
}

/// The sample opens with both reports computed, and "Edit analysis…"
/// reopens the dialog filled from the matching report.
#[test]
fn treatment_sample_reports_open_and_edit() {
    let gui = Gui::with_sample("Cardiac rehabilitation");
    let active = gui.app().active;
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 2);
    let saved = reports[1].saved().clone();
    let table = gui.app().documents[active].table.clone();
    let dialog = crate::treatment_dialog::TreatmentDialog::from_saved(&saved, &table).expect("fills from the report");
    let _ = dialog;
    assert!(crate::app::editable(&saved));
}
