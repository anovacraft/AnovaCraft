//! Multivariate ▸ Factor analysis and Discriminant analysis: the dialogs say
//! what is missing, run, reopen filled from their report and update it in
//! place; the reports show their main sections and offer their columns.

use super::*;

fn reports(gui: &Gui) -> usize {
    gui.app().documents[gui.app().active].reports.len()
}

fn clear_reports(gui: &mut Gui) {
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
}

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

/// Whether a label containing `part` is on screen.
fn shows_part(gui: &Gui, part: &str) -> bool {
    gui.harness.query_all_by_label_contains(part).next().is_some()
}

/// Run the dialog `title`, check the report, reopen it with "Edit analysis…"
/// (filled: it shows `summary`) and Update it in place.
fn run_edit_update(gui: &mut Gui, title: &str, summary: &str) {
    assert!(shows_part(gui, summary), "{:?}", gui.painted_text());
    gui.click("Run");
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
fn factor_analysis_dialog_runs_and_edits() {
    const TITLE: &str = "Factor analysis";
    let mut gui = Gui::with_sample("Ability tests");
    clear_reports(&mut gui);
    gui.menu(&["Multivariate", "Factor analysis…"]);
    gui.shot("dialog-factor-analysis-first-open");
    assert!(gui.shows("⚠ Choose at least three columns"));
    assert!(gui.widget("Run").is_disabled());
    for name in
        ["Vocabulary", "Reading comprehension", "Arithmetic", "Number series", "Mental rotation", "Paper folding"]
    {
        tick(&mut gui, name);
    }
    gui.click("Promax");
    gui.shot("dialog-factor-analysis-ready");
    run_edit_update(&mut gui, TITLE, "Factor analysis of 6 columns, factors with eigenvalue above 1");
    let doc = &gui.app().documents[gui.app().active];
    let app_model::SavedReport::FactorAnalysis { request } = doc.reports[0].saved() else {
        panic!("not a factor analysis")
    };
    assert_eq!(request.rotation, app_model::Rotation::Promax, "kept through the edit");
    assert_eq!(request.columns.len(), 6);
    let sections = app_model::run_report(&doc.reports[0].saved(), &doc.table, &doc.rows).expect("it runs").sections;
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.contains(&"Scree Plot") && titles.contains(&"Factor Correlations"), "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with("Loading Plot")), "{titles:?}");
    // The report offers its factor scores as columns.
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    assert!(gui.shows("Save factor 1 scores"), "{:?}", gui.painted_text());
}

#[test]
fn factor_analysis_refuses_too_many_factors() {
    let mut gui = Gui::with_sample("Ability tests");
    clear_reports(&mut gui);
    gui.menu(&["Multivariate", "Factor analysis…"]);
    for name in ["Vocabulary", "Arithmetic", "Mental rotation"] {
        tick(&mut gui, name);
    }
    gui.click("Exactly");
    let field = gui.window("Factor analysis").get_all_by_role(Role::SpinButton).next().expect("the factors").id();
    gui.set_number(field, 3.0);
    assert!(gui.shows("⚠ Choose fewer factors than columns (at most 2)"), "{:?}", gui.painted_text());
    gui.click("Cancel");
}

#[test]
fn discriminant_analysis_dialog_runs_with_validation_roles() {
    const TITLE: &str = "Discriminant analysis";
    let mut gui = Gui::with_sample("Honey origins");
    clear_reports(&mut gui);
    gui.menu(&["Multivariate", "Discriminant analysis…"]);
    gui.shot("dialog-discriminant-analysis-first-open");
    assert!(gui.shows("⚠ Choose a response"));
    gui.choose(TITLE, 0, "Moisture");
    tick(&mut gui, "Proline");
    assert!(
        gui.shows("⚠ The response must be categorical (text, or numbers marked nominal or ordinal)"),
        "{:?}",
        gui.painted_text()
    );
    gui.choose(TITLE, 0, "Origin");
    for name in ["Moisture", "Fructose", "Conductivity"] {
        tick(&mut gui, name);
    }
    gui.click("Roles from a column");
    assert!(gui.shows("⚠ Choose the column of validation roles"));
    gui.choose(TITLE, 1, "Validation");
    gui.click("Quadratic");
    gui.shot("dialog-discriminant-analysis-ready");
    run_edit_update(&mut gui, TITLE, "Quadratic discriminant analysis of Origin on 4 predictors");
    let doc = &gui.app().documents[gui.app().active];
    let app_model::SavedReport::DiscriminantAnalysis { request } = doc.reports[0].saved() else {
        panic!("not a discriminant analysis")
    };
    assert!(matches!(request.validation, app_model::ValidationMethod::Column(_)), "kept through the edit");
    let sections = app_model::run_report(&doc.reports[0].saved(), &doc.table, &doc.rows).expect("it runs").sections;
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.iter().any(|t| t.starts_with("Fit by Role")), "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with("Confusion Matrix")), "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with("Canonical Plot")), "{titles:?}");
    // The posterior probabilities can be saved for model comparison.
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    assert!(gui.shows("Save probability of Coast"), "{:?}", gui.painted_text());
    assert!(gui.shows("Save canonical 1 scores"), "{:?}", gui.painted_text());
}

/// Discriminant analysis is also offered with the other classifiers.
#[test]
fn discriminant_analysis_is_in_the_model_menu() {
    let mut gui = Gui::with_sample("Honey origins");
    gui.menu(&["Model", "Discriminant analysis…"]);
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Discriminant analysis").is_some());
    gui.click("Cancel");
}

/// The sample reports reopen complete in their dialogs.
#[test]
fn sample_reports_reopen_complete() {
    for sample in ["Ability tests", "Honey origins"] {
        let mut gui = Gui::with_sample(sample);
        let active = gui.app().active;
        let ids: Vec<u64> = gui.app().documents[active].reports.iter().map(|r| r.id()).collect();
        assert_eq!(ids.len(), 2);
        for id in ids {
            gui.app_mut().edit_report(id);
            gui.run();
            let text = gui.painted_text();
            assert!(text.iter().any(|t| t == "Update"), "{sample}: did not reopen");
            let warning = text.iter().find(|t| t.starts_with("⚠ "));
            assert!(warning.is_none(), "{sample}: reopened incomplete: {warning:?}");
            gui.click("Cancel");
        }
    }
}
