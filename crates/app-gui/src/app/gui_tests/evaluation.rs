//! The design evaluation dialog (Design ▸ This table ▸ Evaluate design…).

use super::*;

const TITLE: &str = "Evaluate a design";

/// Tick the first check box labelled `label`.
fn tick(gui: &mut Gui, label: &str) {
    let tick = |n: &Node<'_>| n.role() == Role::CheckBox && n.label().as_deref() == Some(label);
    gui.harness.get_all_by(tick).next().expect("a check box").simulate_click();
    gui.run();
}

/// Whether any label contains `text`.
fn says(gui: &Gui, text: &str) -> bool {
    gui.harness.query_all_by_label_contains(text).next().is_some()
}

/// The dialog asks for factors, checks the formula, and runs the
/// comparison of the two stacked designs; the report shows every section,
/// reopens in the dialog with its settings, and its anticipated coefficient
/// is changed in place from the options menu.
#[test]
fn evaluate_design_dialog_compares_two_designs() {
    let mut gui = Gui::with_sample("Design comparison");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Design", "Evaluate design…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some());
    assert!(gui.shows("⚠ Choose at least one factor"));
    for name in ["Temperature", "Time", "Pressure"] {
        tick(&mut gui, name);
    }
    assert!(says(&gui, "Evaluate 3 factors for a 7-term model"), "{:?}", gui.painted_text());
    gui.click("Full quadratic (response surface)");
    assert!(says(&gui, "Evaluate 3 factors for a 10-term model"));
    gui.choose(TITLE, 0, "Design");
    let summary = "Evaluate 3 factors for a 10-term model, comparing the designs in Design";
    assert!(says(&gui, summary), "{:?}", gui.painted_text());
    gui.shot("evaluate-design-dialog");
    // The button, not the check box of the column called Run.
    let run = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Run");
    gui.harness.get_by(run).simulate_click();
    gui.run();
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::DesignEvaluation { request } = reports[0].saved() else {
        panic!("not a design evaluation")
    };
    assert_eq!(request.factors.len(), 3);
    assert_eq!(request.compare_by, gui.app().documents[active].table.find("Design"));
    assert!(request.model.contains("Temperature²"), "{}", request.model);
    let id = reports[0].id();

    gui.harness.set_size(egui::vec2(1500.0, 4000.0));
    gui.results();
    assert_eq!(gui.app().documents[active].reports[0].title(), "Design Evaluation: Box–Behnken vs I-optimal (Design)");
    for heading in [
        "Design Summary",
        "Power by Effect",
        "Prediction Variance Profile",
        "Fraction of Design Space",
        "Alias Matrix",
        "Design Comparison",
    ] {
        assert!(gui.shows(heading), "{heading} missing");
    }
    let text = gui.painted_text();
    assert!(text.iter().any(|t| t == "I-optimal"), "the plots' key names the designs");
    gui.shot("evaluate-design-report");

    // The report reopens in the dialog, which updates it in place.
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(says(&gui, summary), "filled from the report");
    gui.click("Update");
    let reports = &gui.app().documents[active].reports;
    assert_eq!((reports.len(), reports[0].id()), (1, id), "updated in place");

    // Settings in place: the anticipated coefficient.
    gui.results();
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    assert!(gui.shows("Alias terms:") || says(&gui, "Alias terms"));
    let field = gui.harness.get_all_by(|n| n.role() == Role::SpinButton).next().expect("the coefficient field").id();
    gui.set_number(field, 2.0);
    let app_model::SavedReport::DesignEvaluation { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a design evaluation")
    };
    assert_eq!(request.coefficients, [2.0]);
}

/// Constraints typed in the dialog restrict the evaluation region: the
/// summary and the report say so, and the report reopens with them.
#[test]
fn evaluate_design_dialog_takes_constraints() {
    let mut gui = Gui::with_sample("Design comparison");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Design", "Evaluate design…"]);
    for name in ["Temperature", "Time"] {
        tick(&mut gui, name);
    }
    gui.widget("Region constraints").type_text("Temperature + Time <= 200");
    gui.run();
    assert!(says(&gui, "over the constrained region"), "{:?}", gui.painted_text());
    let run = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("Run");
    gui.harness.get_by(run).simulate_click();
    gui.run();
    let reports = &gui.app().documents[active].reports;
    assert_eq!(reports.len(), 1);
    let app_model::SavedReport::DesignEvaluation { request } = reports[0].saved() else {
        panic!("not a design evaluation")
    };
    assert_eq!(request.constraints, "Temperature + Time <= 200");
    gui.harness.set_size(egui::vec2(1500.0, 4000.0));
    gui.results();
    assert!(gui.shows("Prediction Variance over the Constrained Region"), "{:?}", gui.painted_text());
    // Reopened from the report, the dialog keeps the constraints.
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(says(&gui, "over the constrained region"), "filled from the report");
}

/// An optimal design made with constraints opens with its evaluation over
/// the constrained region.
#[test]
fn constrained_optimal_design_opens_with_its_evaluation() {
    let mut gui = Gui::with_sample("Longley");
    gui.menu(&["Design", "D-optimal (coordinate exchange)…"]);
    gui.widget("Constraints").type_text("A + B <= 0.5");
    gui.run();
    gui.click("Create");
    let doc = &gui.app().documents[gui.app().active];
    let [report] = doc.reports.as_slice() else { panic!("one report expected, got {}", doc.reports.len()) };
    let app_model::SavedReport::DesignEvaluation { request } = report.saved() else {
        panic!("not a design evaluation")
    };
    assert_eq!(request.constraints, "A + B <= 0.5");
    gui.harness.set_size(egui::vec2(1500.0, 4000.0));
    gui.results();
    assert!(gui.shows("Prediction Variance over the Constrained Region"), "{:?}", gui.painted_text());
}
