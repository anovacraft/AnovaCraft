//! Mixed-model additions: the ordinal family, residual variances by group,
//! robust standard errors and between–within df in the mixed-model dialog,
//! and random batches in the shelf-life dialog.

use super::*;

const MIXED: &str = "Mixed model";

fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

fn fresh(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui
}

fn saved_mixed(gui: &Gui, k: usize) -> app_model::MixedRequest {
    match gui.app().documents[gui.app().active].reports[k].saved() {
        app_model::SavedReport::Mixed { request, .. } => request,
        _ => panic!("not a mixed model"),
    }
}

/// Choosing the ordinal family accepts a rating column and the report shows
/// the thresholds and category probabilities.
#[test]
fn ordinal_mixed_model_runs_from_the_dialog() {
    let mut gui = fresh("Wine bitterness");
    gui.menu(&["Model", "Mixed model…"]);
    gui.click("Ordinal (cumulative logit link)");
    gui.choose(MIXED, 0, "Bitterness");
    gui.choose(MIXED, 1, "Judge");
    tick(&mut gui, "Temperature");
    tick(&mut gui, "Skin contact");
    gui.shot("dialog-mixed-model-ordinal");
    gui.click("Run");
    let request = saved_mixed(&gui, 0);
    assert_eq!(request.family, app_model::MixedFamily::Ordinal);
    gui.results();
    assert!(gui.shows("Thresholds (logit P(Bitterness ≤ category) = threshold − linear predictor)"));
    assert!(gui.shows("Category Probabilities (observed and predicted proportions)"));
}

/// A residual variance per level with its test, robust standard errors, and
/// the between–within df choice reach the saved request and the report.
#[test]
fn mixed_dialog_sets_residual_groups_robust_and_between_within() {
    let mut gui = fresh("Plant growth");
    gui.menu(&["Model", "Mixed model…"]);
    gui.choose(MIXED, 0, "Height");
    gui.choose(MIXED, 1, "Greenhouse");
    tick(&mut gui, "Fertilizer");
    tick(&mut gui, "Light hours");
    assert!(gui.shows("Residual variance differs by (optional):"));
    assert!(gui.shows("Frequency (optional):"));
    gui.choose(MIXED, 5, "Fertilizer");
    gui.click("More options");
    tick(&mut gui, "Robust standard errors, clustered on the outermost grouping");
    gui.click("Run");
    let request = saved_mixed(&gui, 0);
    assert!(request.robust);
    assert!(request.residual_groups.is_some());
    gui.results();
    assert!(gui.harness.query_all_by_label_contains("Test of Equal Residual Variances").next().is_some());

    let mut gui = fresh("Plant growth");
    gui.menu(&["Model", "Mixed model…"]);
    gui.choose(MIXED, 0, "Height");
    gui.choose(MIXED, 1, "Greenhouse");
    tick(&mut gui, "Fertilizer");
    gui.click("More options");
    gui.click("Between–within");
    gui.click("Run");
    let request = saved_mixed(&gui, 0);
    assert_eq!(request.df_method, app_model::DfMethod::BetweenWithin);
    assert!(!request.robust && request.residual_groups.is_none());
}

/// Batches as a random sample, with the bound for the mean of all batches.
#[test]
fn shelf_life_dialog_offers_random_batches() {
    const SHELF: &str = "Shelf life";
    let mut gui = fresh("Capsule stability");
    gui.menu(&["Model", "Shelf life…"]);
    gui.choose(SHELF, 0, "Month");
    gui.choose(SHELF, 1, "Assay (% label claim)");
    assert!(!gui.shows("A random sample of batches"));
    gui.choose(SHELF, 2, "Lot");
    gui.click("A random sample of batches");
    assert!(gui.shows("A future batch"));
    gui.click("The mean of all batches");
    assert!(gui.harness.query_all_by_label_contains("over Month, Lot as random").next().is_some());
    gui.shot("dialog-shelf-life-random-batches");
    gui.click("Run");
    let active = gui.app().active;
    let app_model::SavedReport::ShelfLife { request } = gui.app().documents[active].reports[0].saved() else {
        panic!("not a shelf-life report")
    };
    assert!(request.random_batches && !request.future_batch);
    gui.results();
    assert!(gui.shows("Batch-to-Batch Variation"));
}
