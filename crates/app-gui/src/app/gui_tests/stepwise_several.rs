//! Stepwise selection for several responses: from the several-responses
//! dialog (each response's terms chosen from the model form's) and from the
//! stepwise dialog (from the candidate effects ticked there); both open a
//! several-responses report with a term-selection summary.

use super::*;

const SEVERAL: &str = "Several responses";
const STEPWISE: &str = "Stepwise selection";

/// The `n`th check box labelled `label` in window `window` (lists of
/// responses come before lists of factors).
fn tick(gui: &mut Gui, window: &str, label: &str, n: usize) {
    gui.window(window)
        .get_all_by(|node| node.role() == Role::CheckBox && node.label().as_deref() == Some(label))
        .nth(n)
        .unwrap_or_else(|| panic!("no check box {label} #{n}"))
        .simulate_click();
    gui.run();
}

/// The coating sample without its reports.
fn coating() -> Gui {
    let mut gui = Gui::with_sample("Coating formulation");
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui.run();
    gui
}

fn several_request(gui: &Gui) -> app_model::MultipleResponsesRequest {
    match gui.app().documents[gui.app().active].reports[0].saved() {
        app_model::SavedReport::MultipleResponses { request } => (*request).clone(),
        _ => panic!("not a several-responses report"),
    }
}

fn reopen(gui: &mut Gui) {
    gui.results();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
}

/// Model ▸ Several responses with "Select terms stepwise for each
/// response": each response keeps its own terms of the quadratic model, the
/// report lists them and each search's steps, and Edit analysis reopens the
/// same dialog with the choice kept.
#[test]
fn several_responses_dialog_selects_terms_stepwise() {
    let mut gui = coating();
    gui.menu(&["Model", "Several responses (desirability, simulation)…"]);
    for response in ["Hardness", "Gloss", "Cost"] {
        tick(&mut gui, SEVERAL, response, 0);
    }
    for factor in ["Temperature (°C)", "Time (min)", "Catalyst (%)"] {
        tick(&mut gui, SEVERAL, factor, 1);
    }
    gui.click("Full quadratic (interactions and squares)");
    gui.click("Select terms stepwise for each response");
    gui.click("AIC");
    let rows = format!(" · {} rows", gui.app().documents[gui.app().active].table.n_rows());
    let summary = format!("Stepwise terms for each of 3 responses on 3 factors, from the quadratic terms{rows}");
    assert!(gui.shows(&summary), "{:?}", gui.painted_text());
    gui.shot("dialog-several-responses-stepwise");
    gui.click("Run");
    let request = several_request(&gui);
    let selection = request.selection.expect("terms selected stepwise");
    assert_eq!(selection.criterion, app_model::StepCriterion::Aic);
    assert!(selection.candidates.is_empty(), "the model form's terms");
    gui.harness.set_size(egui::vec2(1500.0, 3000.0));
    gui.results();
    let text = gui.painted_text();
    assert!(
        text.iter().any(|t| t.starts_with("Term Selection (mixed stepwise by AIC")),
        "{:?}",
        text.iter().filter(|t| t.len() > 12).collect::<Vec<_>>()
    );
    assert!(text.iter().any(|t| t == "Step History: Gloss"), "{text:?}");
    assert!(
        text.iter().any(|t| t == "Temperature (°C), Catalyst (%), Temperature (°C) × Catalyst (%), Temperature (°C)²")
    );
    gui.shot("report-several-responses-stepwise");
    reopen(&mut gui);
    assert!(gui.shows(&summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Fit every term");
    gui.click("Update");
    assert!(several_request(&gui).selection.is_none());
}

/// Model ▸ Stepwise selection with "Several responses": the candidate
/// effects ticked in the dialog are searched separately for each response,
/// which opens a several-responses report; Edit analysis brings back the
/// stepwise dialog with the responses and candidates.
#[test]
fn stepwise_dialog_runs_for_several_responses() {
    let mut gui = coating();
    gui.menu(&["Model", "Stepwise selection…"]);
    assert!(gui.shows("⚠ Choose a response"), "{:?}", gui.painted_text());
    tick(&mut gui, STEPWISE, "Several responses, each with its own search", 0);
    assert!(gui.shows("⚠ Choose at least one response"), "{:?}", gui.painted_text());
    tick(&mut gui, STEPWISE, "Hardness", 0);
    tick(&mut gui, STEPWISE, "Gloss", 0);
    tick(&mut gui, STEPWISE, "Temperature (°C)", 1);
    tick(&mut gui, STEPWISE, "Time (min)", 1);
    gui.click("+ Two-factor interactions");
    let rows = format!(" · {} rows", gui.app().documents[gui.app().active].table.n_rows());
    let summary = format!(
        "Mixed (forward and backward) stepwise selection for each of 2 responses among 3 effects (1 interaction){rows}"
    );
    assert!(gui.shows(&summary), "{:?}", gui.painted_text());
    gui.shot("dialog-stepwise-several");
    gui.click("Run");
    let request = several_request(&gui);
    assert_eq!(request.responses.len(), 2);
    assert_eq!(request.predictors.len(), 2);
    let selection = request.selection.expect("terms selected stepwise");
    assert_eq!(selection.candidates, [vec![(0, 1)], vec![(1, 1)], vec![(0, 1), (1, 1)]]);
    gui.results();
    assert!(gui.painted_text().iter().any(|t| t.starts_with("Term Selection")), "{:?}", gui.painted_text());
    reopen(&mut gui);
    assert!(gui.shows(&summary), "the stepwise dialog, filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports.len(), 1, "Update keeps one report");
    assert_eq!(several_request(&gui).selection.unwrap().candidates.len(), 3);
}
