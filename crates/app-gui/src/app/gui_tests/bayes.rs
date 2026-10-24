//! The Bayesian dialogs (Compare ▸ Bayesian and Model ▸ Bayesian).

use super::*;

/// The sample `name` with its saved reports closed.
fn empty_sample(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui
}

fn first_saved(gui: &Gui) -> app_model::SavedReport {
    gui.app().documents[gui.app().active].reports[0].saved()
}

/// Open the top report's ⋯ Options menu.
fn top_options(gui: &mut Gui) {
    gui.results();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
}

/// Compare ▸ Bayesian ▸ Bayes factor tests…: the dialog says what is
/// missing, runs a two-sample test with its robustness plot, and the
/// report's options change the alternative in place.
#[test]
fn bayes_factor_dialog_runs_and_changes_in_place() {
    const TITLE: &str = "Bayes factor tests";
    let mut gui = empty_sample("Stroop reaction times (simulated)");
    gui.menu(&["Compare", "Bayesian", "Bayes factor tests…"]);
    assert!(gui.shows("⚠ Choose the first column"));
    assert!(gui.widget("Run").is_disabled());
    gui.choose(TITLE, 0, "Interference");
    assert!(gui.shows("⚠ Choose the group (two levels)"));
    gui.choose(TITLE, 1, "Age group");
    gui.shot("dialog-bayes-factor");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog closes");
    let app_model::SavedReport::Bayesian { request: app_model::BayesRequest::Factor(r) } = first_saved(&gui) else {
        panic!("not a Bayes factor report")
    };
    assert!(matches!(r.test, app_model::BayesTestKind::TwoSample { .. }));
    gui.harness.set_size(egui::vec2(1500.0, 3600.0));
    gui.results();
    assert!(gui.app().documents[gui.app().active].reports[0].title().starts_with("Bayes Factor t Test"));
    assert!(gui.shows("Bayes Factor"));
    let kinds: Vec<app_model::SectionKind> =
        gui.app().documents[gui.app().active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect();
    assert!(kinds.contains(&app_model::SectionKind::BayesRobustness), "{kinds:?}");
    top_options(&mut gui);
    gui.click("Effect > 0");
    gui.run();
    let app_model::SavedReport::Bayesian { request: app_model::BayesRequest::Factor(r) } = first_saved(&gui) else {
        panic!("not a Bayes factor report")
    };
    assert_eq!(r.direction, stat_bayes::bayes_factor::Direction::Greater);
}

/// Model ▸ Bayesian ▸ Bayesian regression or hierarchical model…: the
/// eight schools as a known-SD hierarchical model; the report shows the
/// posterior summary, sampler diagnostics and trace plots, and Edit
/// analysis… reopens it filled in.
#[test]
fn hierarchical_model_dialog_runs_the_eight_schools() {
    const TITLE: &str = "Bayesian regression or hierarchical model";
    let mut gui = empty_sample("Eight schools (Rubin 1981)");
    gui.menu(&["Model", "Bayesian", "Bayesian regression or hierarchical model…"]);
    gui.choose(TITLE, 0, "Estimated effect");
    gui.click("+ Add group effect");
    assert!(gui.shows("⚠ Choose a grouping column"));
    gui.choose(TITLE, 1, "School");
    gui.choose(TITLE, 3, "Standard error");
    gui.shot("dialog-bayes-model");
    gui.click("Run");
    let app_model::SavedReport::Bayesian { request: app_model::BayesRequest::Model(r) } = first_saved(&gui) else {
        panic!("not a Bayesian model report")
    };
    let table = &gui.app().documents[gui.app().active].table;
    assert_eq!(r.known_sd, table.find("Standard error"));
    assert_eq!(r.random.len(), 1);
    gui.harness.set_size(egui::vec2(1500.0, 3600.0));
    gui.results();
    let title = gui.app().documents[gui.app().active].reports[0].title();
    assert_eq!(title, "Bayesian Hierarchical Normal Model: Estimated effect");
    assert!(gui.painted_text().iter().any(|t| t.starts_with("Posterior Summary")));
    let kinds: Vec<app_model::SectionKind> =
        gui.app().documents[gui.app().active].reports[0].plot_keys().into_iter().map(|(k, _)| k).collect();
    for kind in [app_model::SectionKind::BayesTrace, app_model::SectionKind::BayesPredictive] {
        assert!(kinds.contains(&kind), "{kind:?} missing from {kinds:?}");
    }
    gui.shot("bayes-model-report");
    top_options(&mut gui);
    gui.click("Edit analysis…");
    assert!(gui.shows("Update"));
    gui.click("Update");
    assert_eq!(gui.app().documents[gui.app().active].reports.len(), 1, "Update keeps one report");
}

/// Compare ▸ Bayesian ▸ Bayesian estimation…: two proportions by arm with
/// an odds ratio, and Model ▸ Bayesian ▸ Bayesian linear model or ANOVA…
/// with an interaction.
#[test]
fn estimation_and_linear_model_dialogs_run() {
    let mut gui = empty_sample("Response rates by site (simulated)");
    gui.menu(&["Compare", "Bayesian", "Bayesian estimation…"]);
    gui.click("Proportion");
    gui.click("Two groups");
    gui.choose("Bayesian estimation", 0, "Response");
    gui.choose("Bayesian estimation", 1, "Arm");
    gui.click("Odds ratio");
    gui.click("Run");
    let app_model::SavedReport::Bayesian { request: app_model::BayesRequest::Estimation(r) } = first_saved(&gui) else {
        panic!("not a Bayesian estimation report")
    };
    assert_eq!(r.parameter, app_model::EstimatedParameter::Proportion);
    assert_eq!(r.contrast, stat_bayes::univariate::Contrast::OddsRatio);
    assert!(r.group.is_some());

    let mut gui = empty_sample("Stroop reaction times (simulated)");
    gui.menu(&["Model", "Bayesian", "Bayesian linear model or ANOVA…"]);
    gui.choose("Bayesian linear model or ANOVA", 0, "Interference");
    assert!(gui.shows("⚠ Choose at least one predictor"));
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some("Age group")).simulate_click();
    gui.run();
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some("Strategy")).simulate_click();
    gui.run();
    gui.click("+ Two-factor interactions");
    gui.click("Run");
    let app_model::SavedReport::Bayesian { request: app_model::BayesRequest::Linear(r) } = first_saved(&gui) else {
        panic!("not a Bayesian linear model report")
    };
    assert_eq!(r.effects.len(), 3, "both main effects and their interaction");
    gui.results();
    assert_eq!(gui.app().documents[gui.app().active].reports[0].title(), "Bayesian ANOVA: Interference");
}
