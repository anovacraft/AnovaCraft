//! Multivariate ▸ Gaussian mixture clustering…, Latent class analysis… and
//! Dimension reduction (MDS, t-SNE, UMAP)…: fill each dialog, run it, check
//! the report, and reopen it with "Edit analysis…".

use super::*;

const MIXTURE: &str = "Gaussian mixture clustering";
const MAP: &str = "Dimension reduction";

/// Open a sample with no reports.
fn empty_sample(name: &str) -> Gui {
    let mut gui = Gui::with_sample(name);
    let active = gui.app().active;
    gui.app_mut().documents[active].reports.clear();
    gui
}

/// Tick the check box labelled `label`.
fn tick(gui: &mut Gui, label: &str) {
    gui.harness.get_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some(label)).simulate_click();
    gui.run();
}

fn reports(gui: &Gui) -> Vec<app_model::SavedReport> {
    gui.app().documents[gui.app().active].reports.iter().map(|r| r.saved()).collect()
}

/// Reopen the only report in its dialog, check its summary and Update it in place.
fn edit_and_update(gui: &mut Gui, summary: &str) {
    gui.results();
    let id = gui.app().documents[gui.app().active].reports[0].id();
    let options = |n: &Node<'_>| n.role() == Role::Button && n.label().as_deref() == Some("⋯ Options");
    gui.harness.get_all_by(options).next().expect("a report").simulate_click();
    gui.run();
    gui.click("Edit analysis…");
    assert!(gui.shows(summary), "filled from the report: {:?}", gui.painted_text());
    gui.click("Update");
    let doc = &gui.app().documents[gui.app().active];
    assert_eq!(doc.reports.len(), 1);
    assert_eq!(doc.reports[0].id(), id);
}

#[test]
fn gaussian_mixture_dialog_runs_and_edits() {
    let mut gui = empty_sample("Cell populations");
    gui.menu(&["Multivariate", "Gaussian mixture clustering…"]);
    gui.shot("dialog-gaussian-mixture-first-open");
    assert!(gui.shows("⚠ Choose at least one column"));
    assert!(gui.widget("Run").is_disabled());
    tick(&mut gui, "CD4 marker");
    tick(&mut gui, "CD8 marker");
    let summary = "Gaussian mixtures of 2 columns, 1 to 6 components · 360 rows";
    assert!(gui.shows(summary), "{:?}", gui.painted_text());
    // Every structure unticked: nothing to compare.
    for s in app_model::CovarianceStructure::ALL {
        tick(&mut gui, &format!("{} — {}", s.key(), s.label().to_lowercase()));
    }
    assert!(gui.shows("⚠ Tick at least one covariance structure"));
    for s in app_model::CovarianceStructure::ALL {
        tick(&mut gui, &format!("{} — {}", s.key(), s.label().to_lowercase()));
    }
    gui.shot("dialog-gaussian-mixture-ready");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, MIXTURE).is_none(), "the dialog closes");
    let saved = reports(&gui);
    assert_eq!(saved.len(), 1);
    let app_model::SavedReport::GaussianMixture { request } = &saved[0] else { panic!("not a mixture: {saved:?}") };
    assert_eq!(request.columns.len(), 2);
    gui.results();
    assert!(gui.shows("Gaussian Mixture (VVV, 3 components): CD4 marker, CD8 marker"), "{:?}", gui.painted_text());
    assert!(gui.shows("BIC by Number of Components (smaller is better)"));
    edit_and_update(&mut gui, summary);
}

#[test]
fn latent_class_dialog_runs_and_edits() {
    let mut gui = empty_sample("Symptom checklist");
    gui.menu(&["Multivariate", "Latent class analysis…"]);
    gui.shot("dialog-latent-class-first-open");
    assert!(gui.shows("⚠ Choose at least two items"));
    assert!(gui.widget("Run").is_disabled());
    // Continuous columns are not offered as items.
    assert!(
        gui.harness.query_by(|n| n.role() == Role::CheckBox && n.label().as_deref() == Some("Age")).is_none(),
        "Age is continuous"
    );
    tick(&mut gui, "Fatigue");
    assert!(gui.shows("⚠ Choose at least two items"));
    for item in ["Poor sleep", "Headache", "Muscle pain", "Low mood", "Worry", "Poor concentration"] {
        tick(&mut gui, item);
    }
    let summary = "Latent classes of 7 items, 1 to 5 classes · 600 rows";
    assert!(gui.shows(summary), "{:?}", gui.painted_text());
    gui.shot("dialog-latent-class-ready");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, "Latent class analysis").is_none(), "the dialog closes");
    let saved = reports(&gui);
    assert_eq!(saved.len(), 1);
    let app_model::SavedReport::LatentClass { request } = &saved[0] else { panic!("not latent classes: {saved:?}") };
    assert_eq!(request.items.len(), 7);
    gui.results();
    assert!(
        gui.shows("Latent Class Analysis (3 classes): Fatigue, Poor sleep, Headache, … (7 items)"),
        "{:?}",
        gui.painted_text()
    );
    assert!(gui.shows("Model Selection by Number of Classes (smaller criteria are better)"));
    assert!(gui.shows("Response Probability Profiles (one stem per class at each level)"));
    edit_and_update(&mut gui, summary);
}

#[test]
fn dimension_reduction_dialog_runs_and_edits() {
    let mut gui = empty_sample("Expression profiles");
    gui.menu(&["Multivariate", "Dimension reduction (MDS, t-SNE, UMAP)…"]);
    gui.shot("dialog-dimension-reduction-first-open");
    assert!(gui.shows("⚠ Choose at least one column"));
    gui.click("UMAP");
    for g in 1..=6 {
        tick(&mut gui, &format!("Gene {g:02}"));
    }
    gui.choose(MAP, 0, "Cell type");
    let summary = "UMAP map of 6 columns · 300 rows";
    assert!(gui.shows(summary), "{:?}", gui.painted_text());
    // As a distance matrix the columns would need one per row.
    gui.click("a distance matrix");
    assert!(gui.shows("⚠ A distance matrix needs one column per row: 300 rows, 6 columns chosen"));
    gui.click("variables");
    gui.shot("dialog-dimension-reduction-ready");
    gui.click("Run");
    assert!(gui.harness.query_by_role_and_label(Role::Window, MAP).is_none(), "the dialog closes");
    let saved = reports(&gui);
    let app_model::SavedReport::Embedding { request } = &saved[0] else { panic!("not an embedding: {saved:?}") };
    assert_eq!(request.method, app_model::EmbeddingMethod::Umap);
    assert!(request.color.is_some());
    gui.results();
    assert!(gui.shows("Map (coloured and marked by Cell type)"), "{:?}", gui.painted_text());
    edit_and_update(&mut gui, summary);
}

#[test]
fn town_distances_map_from_a_distance_matrix() {
    let mut gui = empty_sample("Town distances");
    gui.menu(&["Multivariate", "Dimension reduction (MDS, t-SNE, UMAP)…"]);
    gui.click("Metric scaling (SMACOF)");
    gui.click("a distance matrix");
    gui.click("All");
    // "Town" is text, so only the twelve distance columns are ticked.
    let summary = "Metric scaling (SMACOF) map of a 12-row distance matrix · 12 rows";
    assert!(gui.shows(summary), "{:?}", gui.painted_text());
    gui.click("Run");
    gui.results();
    assert!(gui.shows("Metric Scaling (SMACOF) of distances: Ashford, Brookley, Calder, … (12 columns)"));
    assert!(gui.shows("Shepard Diagram (every pair)"), "{:?}", gui.painted_text());
}
