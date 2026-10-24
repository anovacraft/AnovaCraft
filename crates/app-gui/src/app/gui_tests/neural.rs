//! Model ▸ Neural network and Model search. Neural network: the dialog says what is missing, runs with
//! validation roles from a column and two hidden layers, reopens filled from
//! its report and updates it in place; the report shows its diagram, weight
//! maps and profiler, and offers its prediction columns, including the
//! formula.

use super::*;

const TITLE: &str = "Neural network";

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
fn neural_dialog_runs_and_edits() {
    let mut gui = Gui::with_sample("Polymer batches");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Neural network…"]);
    gui.shot("dialog-neural-first-open");
    assert!(gui.shows("⚠ Choose a response"), "{:?}", gui.painted_text());
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Tensile strength (MPa)");
    for name in ["Cure temperature (°C)", "Pressure (bar)", "Catalyst (%)", "Supplier"] {
        tick(&mut gui, name);
    }
    gui.click("Two layers");
    gui.click("Roles from a column");
    assert!(gui.shows("⚠ Choose the column of validation roles"));
    // Drop-down lists: response, activation, role column.
    gui.choose(TITLE, 2, "Validation");
    gui.shot("dialog-neural-ready");
    let summary = "Neural network for Tensile strength (MPa) on 4 predictors, hidden layers 3–2 (hyperbolic tangent)";
    assert!(shows_part(&gui, summary), "{:?}", gui.painted_text());
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    assert_eq!(reports(&gui), 1);
    gui.results();
    gui.shot("report-neural");
    assert!(
        shows_part(
            &gui,
            "Neural Network (3–2 tanh): Tensile strength (MPa) ~ Cure temperature (°C) + Pressure (bar) + Catalyst (%) + Supplier"
        ),
        "{:?}",
        gui.painted_text()
    );
    for part in [
        "Network Diagram",
        "Penalty Tuning (validation rows)",
        "Weights: Inputs to Hidden Layer 1",
        "Response Explorer",
    ] {
        assert!(shows_part(&gui, part), "{part}: {:?}", gui.painted_text());
    }
    // Reopen the dialog from the report and update in place.
    let id = gui.app().documents[gui.app().active].reports[0].id();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    assert!(gui.shows("Save prediction formula"), "{:?}", gui.painted_text());
    gui.click("Edit analysis…");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_some(), "the dialog reopens");
    assert!(shows_part(&gui, summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("One layer");
    gui.click("Update");
    assert_eq!(reports(&gui), 1, "updated in place");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports[0].id(), id);
    let app_model::SavedReport::NeuralNetwork { request, .. } = doc.reports[0].saved() else {
        panic!("not a neural network report")
    };
    assert_eq!(request.hidden, [3]);
    assert_eq!(request.predictors.len(), 4);
    assert!(matches!(request.validation, app_model::ValidationMethod::Column(_)));
    let sections = app_model::run_report(&doc.reports[0].saved(), &doc.table, &doc.rows).expect("it runs").sections;
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles.iter().filter(|t| t.starts_with("Weights:")).count(), 2, "{titles:?}");
}

/// Model ▸ Model search: runs on validation roles from a column, opens the
/// best model's full report below the ranking, and opens any other model's
/// report from the options menu; the dialog reopens filled from the report.
#[test]
fn model_search_opens_the_best_report() {
    const SEARCH: &str = "Model search";
    let mut gui = Gui::with_sample("Polymer batches");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.menu(&["Model", "Model search…"]);
    gui.shot("dialog-model-search-first-open");
    assert!(gui.shows("⚠ Choose a response"), "{:?}", gui.painted_text());
    gui.choose(SEARCH, 0, "Tensile strength (MPa)");
    for name in ["Cure temperature (°C)", "Pressure (bar)", "Catalyst (%)", "Mixing time (min)"] {
        tick(&mut gui, name);
    }
    // Discriminant analysis does not suit a continuous response.
    let discriminant =
        gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some("Discriminant analysis"));
    assert!(discriminant.is_disabled());
    tick(&mut gui, "Partial least squares");
    gui.click("Roles from a column");
    gui.choose(SEARCH, 1, "Validation");
    gui.shot("dialog-model-search-ready");
    let summary = "Search of 6 models for Tensile strength (MPa) on 4 predictors";
    assert!(shows_part(&gui, summary), "{:?}", gui.painted_text());
    gui.click("Run");
    assert_eq!(reports(&gui), 2, "the search and its best model");
    gui.results();
    gui.shot("report-model-search");
    assert!(shows_part(&gui, "Model Search: Tensile strength (MPa) (4 predictors)"), "{:?}", gui.painted_text());
    assert!(shows_part(&gui, "Models Ranked by Held-out RMSE (validation rows)"), "{:?}", gui.painted_text());
    let doc = &gui.app().documents[gui.app().active];
    let app_model::SavedReport::ModelSearch { request } = doc.reports[0].saved() else { panic!("search first") };
    assert!(!request.models.contains(&app_model::ModelKind::PartialLeastSquares));
    let search = app_model::ModelSearch::run(&doc.table, &doc.rows, &request).unwrap();
    let best = search.best().unwrap();
    assert_eq!(doc.reports[1].saved(), search.model_report(best), "the best model's report opens below");
    // Open the decision tree's report from the options menu.
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    assert!(gui.shows("Open a model's full report"), "{:?}", gui.painted_text());
    gui.click("Decision tree");
    assert_eq!(reports(&gui), 3);
    assert!(
        gui.app().documents[gui.app().active]
            .reports
            .iter()
            .any(|r| matches!(r.saved(), app_model::SavedReport::DecisionTree { .. }))
    );
    // Edit the search in place: the dialog reopens filled.
    let search_report = gui.app().documents[gui.app().active]
        .reports
        .iter()
        .position(|r| matches!(r.saved(), app_model::SavedReport::ModelSearch { .. }))
        .unwrap();
    let id = gui.app().documents[gui.app().active].reports[search_report].id();
    gui.app_mut().edit_report(id);
    gui.run();
    assert!(shows_part(&gui, summary), "filled from the report: {:?}", gui.painted_text());
}
